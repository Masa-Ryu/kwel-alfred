use crate::{
    VERSION,
    paths::{self, RuntimePaths},
    secure,
};
use kwel_protocol::{Code, EXTENSION_ID, Error, HOST_NAME, LEGACY_HOST_NAME, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Layout {
    pub root: PathBuf,
    pub manifests: PathBuf,
    pub legacy_binary: PathBuf,
    #[cfg(windows)]
    pub registry: bool,
}
impl Layout {
    pub fn current() -> Result<Self> {
        let home = paths::home()?;
        #[cfg(target_os = "macos")]
        return Ok(Self::for_home(&home));
        #[cfg(target_os = "linux")]
        return Ok(Self::for_home(&home));
        #[cfg(windows)]
        {
            let base = std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .ok_or(Error(Code::UnsafePath))?;
            Ok(Self {
                root: base.join("Kwel/NativeBridge"),
                manifests: base.join("Kwel/NativeBridge/manifests"),
                legacy_binary: home
                    .join("Library/Application Support/Kwel/AlfredBridge/bin/kwel-bridge"),
                registry: true,
            })
        }
    }
    pub fn for_home(home: &Path) -> Self {
        #[cfg(target_os = "macos")]
        let (root, manifests) = (
            home.join("Library/Application Support/Kwel/NativeBridge"),
            home.join("Library/Application Support/Google/Chrome/NativeMessagingHosts"),
        );
        #[cfg(not(target_os = "macos"))]
        let (root, manifests) = (
            home.join(".local/share/kwel-connect"),
            home.join(".config/google-chrome/NativeMessagingHosts"),
        );
        Self {
            root,
            manifests,
            legacy_binary: home
                .join("Library/Application Support/Kwel/AlfredBridge/bin/kwel-bridge"),
            #[cfg(windows)]
            registry: false,
        }
    }
    pub fn binary(&self) -> PathBuf {
        self.root.join("bin").join(paths::native_binary_name())
    }
    pub fn manifest(&self, name: &str) -> PathBuf {
        self.manifests.join(format!("{name}.json"))
    }
    fn record(&self) -> PathBuf {
        self.root.join("install.json")
    }
    fn pending(&self) -> PathBuf {
        self.root.join("pending-install.json")
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    #[serde(rename = "type")]
    pub kind: String,
    pub allowed_origins: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub product: String,
    pub version: String,
    pub protocol: u32,
    pub binaries: BTreeMap<String, String>,
}
fn digest(path: &Path) -> Result<String> {
    secure::verify_regular(path, true)?;
    Ok(secure::hex(&Sha256::digest(fs::read(path)?)))
}
fn record(path: &Path) -> Result<Option<Record>> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let record: Record = serde_json::from_slice(&secure::read(path)?)?;
            if record.product != "kwel-connect"
                || record.protocol != 1
                || semver::Version::parse(&record.version).is_err()
                || record.binaries.len() != 2
                || [paths::native_binary_name(), paths::client_binary_name()]
                    .iter()
                    .any(|name| {
                        !record.binaries.get(*name).is_some_and(|digest| {
                            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())
                        })
                    })
            {
                return Err(Error(Code::InstallConflict));
            }
            Ok(Some(record))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn preflight_manifest(layout: &Layout, name: &str) -> Result<()> {
    let path = layout.manifest(name);
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    let current: Manifest = serde_json::from_slice(&secure::read(&path)?)?;
    let path_matches = current.path == layout.binary()
        || (name == LEGACY_HOST_NAME
            && current.path == layout.legacy_binary
            && current.description == "Kwel Alfred Integration");
    if current.name != name
        || current.kind != "stdio"
        || !path_matches
        || !matches!(
            current.description.as_str(),
            "Kwel Local Integration" | "Kwel Alfred Integration"
        )
        || current.allowed_origins != [format!("chrome-extension://{EXTENSION_ID}/")]
    {
        return Err(Error(Code::InstallConflict));
    }
    Ok(())
}
pub fn validate_registration(layout: &Layout) -> Result<()> {
    for name in [HOST_NAME, LEGACY_HOST_NAME] {
        preflight_manifest(layout, name)?;
        #[cfg(windows)]
        if layout.registry {
            registry_preflight(name, &layout.manifest(name))?;
        }
    }
    Ok(())
}
#[cfg(windows)]
fn registry_preflight(name: &str, path: &Path) -> Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let root = RegKey::predef(HKEY_CURRENT_USER);
    match root.open_subkey(format!(
        r"Software\Google\Chrome\NativeMessagingHosts\{name}"
    )) {
        Ok(key) => {
            let existing: String = key.get_value("")?;
            if Path::new(&existing) != path {
                return Err(Error(Code::InstallConflict));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
/// dpkg invokes these executable maintainer hooks as root. They inspect only
/// package-owned system files/process executable identities, never user homes.
#[cfg(target_os = "linux")]
pub fn system_hook(hook: &str, action: &str) -> Result<()> {
    let binary = Path::new("/usr/lib/kwel-connect/kwel-native-host");
    if hook == "preinst" {
        for name in [HOST_NAME, LEGACY_HOST_NAME] {
            let path = PathBuf::from(format!(
                "/etc/opt/chrome/native-messaging-hosts/{name}.json"
            ));
            if secure::exists(&path)? {
                let manifest: Manifest = serde_json::from_slice(&secure::read(&path)?)?;
                if manifest.path != binary
                    || manifest.name != name
                    || manifest.kind != "stdio"
                    || manifest.allowed_origins != [format!("chrome-extension://{EXTENSION_ID}/")]
                {
                    return Err(Error(Code::InstallConflict));
                }
            }
        }
        let record_path = Path::new("/usr/lib/kwel-connect/release.json");
        if let Some(record) = record(record_path)? {
            let previous = semver::Version::parse(&record.version)
                .map_err(|_| Error(Code::InstallConflict))?;
            if previous
                > semver::Version::parse(VERSION).map_err(|_| Error(Code::InstallConflict))?
            {
                return Err(Error(Code::DowngradeRefused));
            }
        }
    } else if hook == "prerm" && matches!(action, "remove" | "deconfigure") {
        for entry in fs::read_dir("/proc")?.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|byte| byte.is_ascii_digit())
                && fs::read_link(entry.path().join("exe")).is_ok_and(|path| path == binary)
            {
                return Err(Error(Code::HostInUse));
            }
        }
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod registry_tests {
    use super::*;
    #[test]
    fn hkcu_registration_is_exact_and_foreign_values_are_preserved() {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("manifest.json");
        let name = format!("com.kwel.test-{}", uuid::Uuid::new_v4());
        registry_register(&name, &path).unwrap();
        registry_register(&name, &path).unwrap();
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let key_path = format!(r"Software\Google\Chrome\NativeMessagingHosts\{name}");
        let key = root.open_subkey(&key_path).unwrap();
        let actual: String = key.get_value("").unwrap();
        assert_eq!(Path::new(&actual), path);
        drop(key);
        let other = temporary.path().join("other.json");
        assert!(registry_register(&name, &other).is_err());
        assert!(registry_unregister(&name, &other).is_err());
        registry_unregister(&name, &path).unwrap();
        assert!(root.open_subkey(key_path).is_err());
    }
}
fn write_registration(layout: &Layout) -> Result<()> {
    for name in [HOST_NAME, LEGACY_HOST_NAME] {
        let path = layout.manifest(name);
        let manifest = Manifest {
            name: name.into(),
            description: "Kwel Local Integration".into(),
            path: layout.binary(),
            kind: "stdio".into(),
            allowed_origins: vec![format!("chrome-extension://{EXTENSION_ID}/")],
        };
        secure::atomic_write(&path, &serde_json::to_vec_pretty(&manifest)?, false)?;
        #[cfg(windows)]
        if layout.registry {
            registry_register(name, &path)?;
        }
    }
    Ok(())
}
pub fn active(paths: &RuntimePaths) -> Result<bool> {
    for profile in paths.profiles()? {
        let lock = secure::lock(&paths.file(&profile, "lock")?)?;
        if lock.try_lock().is_err() {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn installed_version(layout: &Layout) -> Result<Option<String>> {
    Ok(record(&layout.record())?.map(|r| r.version))
}
/// The explicit installer rejects in-use updates. Alfred may reuse an existing
/// protocol-v1 Host instead; an older Workflow never overwrites a newer Host.
pub fn install(
    source: &Path,
    layout: &Layout,
    runtime: &RuntimePaths,
    allow_reuse: bool,
) -> Result<bool> {
    secure::ensure_dir(&layout.root)?;
    let guard = secure::lock(&layout.root.join("install.lock"))?;
    guard.try_lock().map_err(|_| Error(Code::HostInUse))?;
    let current = record(&layout.record())?;
    let pending = record(&layout.pending())?;
    let desired = semver::Version::parse(VERSION).map_err(|_| Error(Code::InstallConflict))?;
    if pending.as_ref().is_some_and(|record| {
        semver::Version::parse(&record.version).is_ok_and(|version| version > desired)
    }) {
        return Err(Error(Code::DowngradeRefused));
    }
    validate_registration(layout)?;
    for name in [paths::native_binary_name(), paths::client_binary_name()] {
        let target = layout.root.join("bin").join(name);
        if secure::exists(&target)? {
            let hash = digest(&target)?;
            if !current
                .as_ref()
                .and_then(|record| record.binaries.get(name))
                .is_some_and(|value| *value == hash)
                && !pending
                    .as_ref()
                    .and_then(|record| record.binaries.get(name))
                    .is_some_and(|value| *value == hash)
            {
                return Err(Error(Code::InstallConflict));
            }
        } else if current.is_some() && pending.is_none() {
            return Err(Error(Code::InstallConflict));
        }
    }
    if let Some(current) = &current {
        let existing =
            semver::Version::parse(&current.version).map_err(|_| Error(Code::InstallConflict))?;
        if existing > desired {
            write_registration(layout)?;
            return Ok(false);
        }
    }
    let mut next = Record {
        product: "kwel-connect".into(),
        version: VERSION.into(),
        protocol: 1,
        binaries: BTreeMap::new(),
    };
    for name in [paths::native_binary_name(), paths::client_binary_name()] {
        next.binaries
            .insert(name.into(), digest(&source.join(name))?);
    }
    if current
        .as_ref()
        .is_some_and(|current| current.version == next.version && current.binaries == next.binaries)
        && pending.is_none()
    {
        write_registration(layout)?;
        return Ok(false);
    }
    if active(runtime)? {
        if allow_reuse && current.is_some() {
            return Ok(false);
        }
        return Err(Error(Code::HostInUse));
    }
    // Journal only product/version/hash metadata. No profile or browsing data
    // is collected. A crash may leave mixed files; only either journalled hash
    // is accepted on retry, and an older installer cannot roll them back.
    secure::atomic_write(&layout.pending(), &serde_json::to_vec(&next)?, false)?;
    for name in [paths::native_binary_name(), paths::client_binary_name()] {
        secure::copy_executable(&source.join(name), &layout.root.join("bin").join(name))?;
    }
    write_registration(layout)?;
    secure::atomic_write(&layout.record(), &serde_json::to_vec_pretty(&next)?, false)?;
    secure::remove_regular(&layout.pending())?;
    Ok(true)
}
pub fn uninstall(layout: &Layout, runtime: &RuntimePaths) -> Result<()> {
    secure::verify_dir(&layout.root)?;
    let guard = secure::lock(&layout.root.join("install.lock"))?;
    guard.try_lock().map_err(|_| Error(Code::HostInUse))?;
    if active(runtime)? {
        return Err(Error(Code::HostInUse));
    }
    if record(&layout.pending())?.is_some() {
        return Err(Error(Code::InstallConflict));
    }
    let installed = record(&layout.record())?.ok_or(Error(Code::InstallConflict))?;
    validate_registration(layout)?;
    for (name, expected) in &installed.binaries {
        let path = layout.root.join("bin").join(name);
        if digest(&path)? != *expected {
            return Err(Error(Code::InstallConflict));
        }
        #[cfg(windows)]
        if std::env::current_exe().is_ok_and(|current| current == path) {
            return Err(Error(Code::HostInUse));
        }
    }
    for name in [HOST_NAME, LEGACY_HOST_NAME] {
        let path = layout.manifest(name);
        if secure::exists(&path)? {
            let manifest: Manifest = serde_json::from_slice(&secure::read(&path)?)?;
            if manifest.path == layout.binary() {
                #[cfg(windows)]
                if layout.registry {
                    registry_unregister(name, &path)?;
                }
                secure::remove_regular(&path)?;
            }
        }
    }
    for name in installed.binaries.keys() {
        secure::remove_regular(&layout.root.join("bin").join(name))?;
    }
    secure::remove_regular(&layout.record())?;
    // Keep runtime/legacy data and unrelated files. The Workflow never calls
    // this function on its removal, so other launcher installations remain.
    Ok(())
}
#[cfg(windows)]
fn registry_register(name: &str, path: &Path) -> Result<()> {
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, KEY_WRITE},
    };
    let root = RegKey::predef(HKEY_CURRENT_USER);
    let key_path = format!(r"Software\Google\Chrome\NativeMessagingHosts\{name}");
    registry_preflight(name, path)?;
    let (key, _) = root.create_subkey_with_flags(key_path, KEY_WRITE)?;
    key.set_value("", &path.as_os_str().to_string_lossy().as_ref())?;
    Ok(())
}
#[cfg(windows)]
fn registry_unregister(name: &str, path: &Path) -> Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let root = RegKey::predef(HKEY_CURRENT_USER);
    let key_path = format!(r"Software\Google\Chrome\NativeMessagingHosts\{name}");
    match root.open_subkey(&key_path) {
        Ok(key) => {
            let existing: String = key.get_value("")?;
            if Path::new(&existing) != path {
                return Err(Error(Code::InstallConflict));
            }
            drop(key);
            root.delete_subkey(key_path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
