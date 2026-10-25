use crate::secure;
use kwel_protocol::{Code, Error, Result, id};
use std::path::{Path, PathBuf};
#[derive(Debug, Clone)]
pub struct RuntimePaths {
    pub directory: PathBuf,
    #[cfg(windows)]
    pub prefix: String,
}
impl RuntimePaths {
    pub fn current() -> Result<Self> {
        #[cfg(target_os = "macos")]
        let directory = PathBuf::from(format!("/private/tmp/kwel-connect-{}", secure::uid()));
        #[cfg(target_os = "linux")]
        let directory = match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(root) => {
                let root = PathBuf::from(root);
                secure::verify_dir(&root)?;
                root.join("kwel-connect")
            }
            None => PathBuf::from(format!("/tmp/kwel-connect-{}", secure::uid())),
        };
        #[cfg(windows)]
        let directory = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or(Error(Code::UnsafePath))?
            .join("Kwel/NativeBridge/runtime");
        Self::at(directory)
    }
    /// Explicit dependency injection for isolated tests and embedders. The CLI
    /// always calls current(); no IPC request can choose a runtime directory.
    pub fn at(directory: PathBuf) -> Result<Self> {
        secure::ensure_dir(&directory)?;
        let directory = std::fs::canonicalize(directory)?;
        Ok(Self {
            directory,
            #[cfg(windows)]
            prefix: crate::windows::pipe_prefix()?,
        })
    }
    pub fn endpoint(&self, profile: &str, control: bool) -> Result<PathBuf> {
        if !id(profile) {
            return Err(Error(Code::InvalidRequest));
        }
        #[cfg(unix)]
        {
            Ok(self.directory.join(format!(
                "{}{}",
                profile.to_lowercase(),
                if control { ".ctl.sock" } else { ".sock" }
            )))
        }
        #[cfg(windows)]
        {
            use sha2::{Digest, Sha256};
            let namespace = secure::hex(&Sha256::digest(
                self.directory.as_os_str().to_string_lossy().as_bytes(),
            ));
            Ok(PathBuf::from(format!(
                r"\\.\pipe\kwel-{}-{}-{}{}",
                self.prefix,
                &namespace[..16],
                profile.to_lowercase(),
                if control { "-ctl" } else { "" }
            )))
        }
    }
    pub fn file(&self, profile: &str, suffix: &str) -> Result<PathBuf> {
        if !id(profile) || !matches!(suffix, "lock" | "token" | "endpoint") {
            return Err(Error(Code::InvalidRequest));
        }
        Ok(self
            .directory
            .join(format!("{}.{suffix}", profile.to_lowercase())))
    }
    pub fn profiles(&self) -> Result<Vec<String>> {
        secure::verify_dir(&self.directory)?;
        let mut profiles = Vec::new();
        for entry in std::fs::read_dir(&self.directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            #[cfg(unix)]
            let suffix = ".sock";
            #[cfg(windows)]
            let suffix = ".endpoint";
            if let Some(profile) = name.strip_suffix(suffix)
                && id(profile)
            {
                profiles.push(profile.to_owned());
            }
        }
        profiles.sort();
        profiles.dedup();
        Ok(profiles)
    }
}
pub fn home() -> Result<PathBuf> {
    #[cfg(unix)]
    let key = "HOME";
    #[cfg(windows)]
    let key = "USERPROFILE";
    let path = std::env::var_os(key)
        .map(PathBuf::from)
        .ok_or(Error(Code::UnsafePath))?;
    secure::no_symlink_components(&path)?;
    Ok(path)
}
pub fn native_binary_name() -> &'static str {
    if cfg!(windows) {
        "kwel-native-host.exe"
    } else {
        "kwel-native-host"
    }
}
pub fn client_binary_name() -> &'static str {
    if cfg!(windows) {
        "kwel-bridge.exe"
    } else {
        "kwel-bridge"
    }
}
pub fn legacy_directory() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Some(PathBuf::from(format!("/private/tmp/kwa-{}", secure::uid())))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
pub fn valid_absolute(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
}
