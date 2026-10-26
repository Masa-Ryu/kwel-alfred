use crate::{
    VERSION,
    paths::{self, RuntimePaths},
    secure,
};
use kwel_protocol::{Code, EXTENSION_ID, Error, HOST_NAME, Result};
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
    #[cfg(windows)]
    pub registry: bool,
}
impl Layout {
    pub fn current() -> Result<Self> {
        #[cfg(unix)]
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
            #[cfg(windows)]
            registry: false,
        }
    }
    pub fn binary(&self) -> PathBuf {
        self.root.join("bin").join(paths::native_binary_name())
    }
    pub fn manifest(&self) -> PathBuf {
        self.manifests.join(format!("{HOST_NAME}.json"))
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
fn preflight_manifest(layout: &Layout) -> Result<()> {
    let path = layout.manifest();
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    let current: Manifest = serde_json::from_slice(&secure::read(&path)?)?;
    if current.name != HOST_NAME
        || current.kind != "stdio"
        || current.path != layout.binary()
        || current.description != "Kwel Local Connection"
        || current.allowed_origins != [format!("chrome-extension://{EXTENSION_ID}/")]
    {
        return Err(Error(Code::InstallConflict));
    }
    Ok(())
}
pub fn validate_registration(layout: &Layout) -> Result<()> {
    preflight_manifest(layout)?;
    #[cfg(windows)]
    if layout.registry {
        registry_preflight(HOST_NAME, &layout.manifest())?;
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
    if hook == "preinst" || (hook == "prerm" && matches!(action, "remove" | "deconfigure")) {
        system_preflight(
            binary,
            Path::new("/usr/bin/kwel-bridge"),
            Path::new("/etc/opt/chrome/native-messaging-hosts"),
        )?;
    }
    if hook == "prerm" && matches!(action, "remove" | "deconfigure") {
        for entry in fs::read_dir("/proc")?.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|byte| byte.is_ascii_digit())
                && fs::read_link(entry.path().join("exe"))
                    .is_ok_and(|path| system_executable_matches(&path, binary))
            {
                return Err(Error(Code::HostInUse));
            }
        }
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn system_executable_matches(executable: &Path, installed: &Path) -> bool {
    // An upgrade atomically replaces the on-disk inode. Existing sessions keep
    // running and /proc marks their old executable as " (deleted)"; they still
    // protect the shared installation from removal.
    executable == installed
        || executable
            .as_os_str()
            .as_encoded_bytes()
            .strip_suffix(b" (deleted)")
            .is_some_and(|path| path == installed.as_os_str().as_encoded_bytes())
}
#[cfg(target_os = "linux")]
fn system_preflight(binary: &Path, client: &Path, manifests: &Path) -> Result<()> {
    let previous = record(
        &binary
            .parent()
            .ok_or(Error(Code::UnsafePath))?
            .join("release.json"),
    )?;
    if previous.as_ref().is_some_and(|record| {
        semver::Version::parse(&record.version).is_ok_and(|version| {
            version > semver::Version::parse(VERSION).expect("workspace version")
        })
    }) {
        return Err(Error(Code::DowngradeRefused));
    }
    for (name, path) in [
        (paths::native_binary_name(), binary),
        (paths::client_binary_name(), client),
    ] {
        if secure::exists(path)? {
            let actual = digest(path)?;
            if !previous
                .as_ref()
                .and_then(|record| record.binaries.get(name))
                .is_some_and(|expected| *expected == actual)
            {
                return Err(Error(Code::InstallConflict));
            }
        } else if previous.is_some() {
            return Err(Error(Code::InstallConflict));
        }
    }
    let path = manifests.join(format!("{HOST_NAME}.json"));
    if secure::exists(&path)? {
        let manifest: Manifest = serde_json::from_slice(&secure::read(&path)?)?;
        if previous.is_none()
            || manifest.path != binary
            || manifest.name != HOST_NAME
            || manifest.description != "Kwel Local Connection"
            || manifest.kind != "stdio"
            || manifest.allowed_origins != [format!("chrome-extension://{EXTENSION_ID}/")]
        {
            return Err(Error(Code::InstallConflict));
        }
    }
    Ok(())
}
#[cfg(all(test, target_os = "linux"))]
mod system_tests {
    use super::*;
    use std::os::unix::fs::symlink;
    #[test]
    fn dpkg_preflight_preserves_unmanaged_tampered_newer_and_redirected_files() {
        let installed = Path::new("/usr/lib/kwel-connect/kwel-native-host");
        assert!(system_executable_matches(installed, installed));
        assert!(system_executable_matches(
            Path::new("/usr/lib/kwel-connect/kwel-native-host (deleted)"),
            installed
        ));
        assert!(!system_executable_matches(
            Path::new("/usr/lib/different-product/kwel-native-host (deleted)"),
            installed
        ));
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap().join("private");
        secure::ensure_dir(&root).unwrap();
        let binary = root.join("lib/kwel-native-host");
        let client = root.join("bin/kwel-bridge");
        let manifests = root.join("manifests");
        system_preflight(&binary, &client, &manifests).unwrap();
        secure::atomic_write(&binary, b"native fixture", true).unwrap();
        secure::atomic_write(&client, b"client fixture", true).unwrap();
        assert!(system_preflight(&binary, &client, &manifests).is_err());
        let mut metadata = Record {
            product: "kwel-connect".into(),
            version: VERSION.into(),
            protocol: 1,
            binaries: BTreeMap::from([
                ("kwel-native-host".into(), digest(&binary).unwrap()),
                ("kwel-bridge".into(), digest(&client).unwrap()),
            ]),
        };
        let record = binary.parent().unwrap().join("release.json");
        secure::atomic_write(&record, &serde_json::to_vec(&metadata).unwrap(), false).unwrap();
        system_preflight(&binary, &client, &manifests).unwrap();
        secure::atomic_write(&client, b"changed fixture", true).unwrap();
        assert!(system_preflight(&binary, &client, &manifests).is_err());
        secure::atomic_write(&client, b"client fixture", true).unwrap();
        metadata.version = "9.0.0".into();
        secure::atomic_write(&record, &serde_json::to_vec(&metadata).unwrap(), false).unwrap();
        assert!(matches!(
            system_preflight(&binary, &client, &manifests),
            Err(Error(Code::DowngradeRefused))
        ));
        metadata.version = VERSION.into();
        secure::atomic_write(&record, &serde_json::to_vec(&metadata).unwrap(), false).unwrap();
        secure::ensure_dir(&manifests).unwrap();
        symlink(
            root.join("absent"),
            manifests.join(format!("{HOST_NAME}.json")),
        )
        .unwrap();
        assert!(system_preflight(&binary, &client, &manifests).is_err());
        assert_eq!(fs::read(client).unwrap(), b"client fixture");
    }
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
    secure::ensure_dir(&layout.manifests)?;
    let path = layout.manifest();
    let manifest = Manifest {
        name: HOST_NAME.into(),
        description: "Kwel Local Connection".into(),
        path: layout.binary(),
        kind: "stdio".into(),
        allowed_origins: vec![format!("chrome-extension://{EXTENSION_ID}/")],
    };
    secure::atomic_write(&path, &serde_json::to_vec_pretty(&manifest)?, false)?;
    #[cfg(windows)]
    if layout.registry {
        registry_register(HOST_NAME, &path)?;
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
/// Installed Hosts hold a shared lock for their entire Native session. This
/// closes the race between the in-use preflight and a new Chrome connection.
pub fn session_guard(layout: &Layout, executable: &Path) -> Result<Option<fs::File>> {
    if !secure::exists(&layout.binary())?
        || fs::canonicalize(executable)? != fs::canonicalize(layout.binary())?
    {
        return Ok(None);
    }
    secure::verify_dir(&layout.root)?;
    let guard = secure::lock(&layout.root.join("install.lock"))?;
    guard
        .try_lock_shared()
        .map_err(|_| Error(Code::HostInUse))?;
    Ok(Some(guard))
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
    let shared_reuse = if guard.try_lock().is_err() {
        if !allow_reuse || guard.try_lock_shared().is_err() {
            return Err(Error(Code::HostInUse));
        }
        true
    } else {
        false
    };
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
    if shared_reuse {
        return if current.is_some() && pending.is_none() {
            Ok(false)
        } else {
            Err(Error(Code::InstallConflict))
        };
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
    let path = layout.manifest();
    if secure::exists(&path)? {
        preflight_manifest(layout)?;
        #[cfg(windows)]
        if layout.registry {
            registry_unregister(HOST_NAME, &path)?;
        }
        secure::remove_regular(&path)?;
    }
    for name in installed.binaries.keys() {
        secure::remove_regular(&layout.root.join("bin").join(name))?;
    }
    secure::remove_regular(&layout.record())?;
    // Keep runtime data and unrelated files. The Workflow never calls
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
