//! Filesystem boundary. Production paths are selected in `paths`, never by IPC.
use kwel_protocol::{Code, Error, Result};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path},
    sync::atomic::{AtomicU64, Ordering},
};
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut value, "{byte:02x}").expect("hex formatting");
    }
    value
}

#[cfg(unix)]
pub fn uid() -> u32 {
    rustix::process::getuid().as_raw()
}

fn unsafe_path<T>() -> Result<T> {
    Err(Error(Code::UnsafePath))
}
/// Unlike Path::exists, a dangling symlink remains present and is rejected by
/// the caller's regular-file validation instead of being treated as absent.
pub fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
pub fn no_symlink_components(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return unsafe_path();
    }
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return unsafe_path();
    }
    // ancestors() stops at the complete root. A manually accumulated Windows
    // verbatim drive prefix (\\?\C:) is not itself a directory to query.
    for current in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        match fs::symlink_metadata(current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return unsafe_path();
                }
                #[cfg(windows)]
                crate::windows::reject_reparse(current)?;
                #[cfg(unix)]
                {
                    // Ancestors may be root-owned, including the root-owned sticky /tmp.
                    if metadata.uid() != uid() && metadata.uid() != 0 {
                        return unsafe_path();
                    }
                    if metadata.is_dir()
                        && metadata.mode() & 0o022 != 0
                        && !(metadata.uid() == 0 && metadata.mode() & 0o1000 != 0)
                    {
                        return unsafe_path();
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
pub fn ensure_dir(path: &Path) -> Result<()> {
    no_symlink_components(path)?;
    if !path.exists() {
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            ensure_dir(parent)?;
        }
        let builder = fs::DirBuilder::new();
        #[cfg(unix)]
        let mut builder = builder;
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        #[cfg(windows)]
        crate::windows::make_private(path)?;
    }
    #[cfg(unix)]
    {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.uid() != uid() {
            return unsafe_path();
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    {
        crate::windows::verify_owner(path)?;
        crate::windows::make_private(path)?;
    }
    verify_dir(path)
}
pub fn verify_dir(path: &Path) -> Result<()> {
    no_symlink_components(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return unsafe_path();
    }
    #[cfg(unix)]
    if metadata.uid() != uid() || metadata.mode() & 0o777 != 0o700 {
        return unsafe_path();
    }
    #[cfg(windows)]
    crate::windows::verify_private(path)?;
    Ok(())
}
pub fn verify_regular(path: &Path, source: bool) -> Result<()> {
    no_symlink_components(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return unsafe_path();
    }
    #[cfg(unix)]
    if (metadata.uid() != uid() && !(source && metadata.uid() == 0)) || metadata.mode() & 0o022 != 0
    {
        return unsafe_path();
    }
    #[cfg(windows)]
    if !source {
        crate::windows::verify_private(path)?;
    }
    Ok(())
}
pub fn read(path: &Path) -> Result<Vec<u8>> {
    verify_regular(path, false)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(path)?;
    if file.metadata()?.len() > 1_000_000 {
        return unsafe_path();
    }
    Ok(std::io::read_to_string(file)?.into_bytes())
}
pub fn lock(path: &Path) -> Result<File> {
    verify_dir(path.parent().ok_or(Error(Code::UnsafePath))?)?;
    if path.exists() {
        verify_regular(path, false)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(path)?;
    #[cfg(windows)]
    crate::windows::make_private(path)?;
    verify_regular(path, false)?;
    Ok(file)
}
pub fn atomic_write(path: &Path, data: &[u8], executable: bool) -> Result<()> {
    #[cfg(windows)]
    let _ = executable; // Windows uses the owner-only DACL, not POSIX mode bits.
    let parent = path.parent().ok_or(Error(Code::UnsafePath))?;
    if exists(parent)? {
        verify_dir(parent)?;
    } else {
        ensure_dir(parent)?;
    }
    match fs::symlink_metadata(path) {
        Ok(_) => verify_regular(path, false)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let temporary = parent.join(format!(
        ".kwel-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options
            .mode(if executable { 0o700 } else { 0o600 })
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let mut file = options.open(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
        #[cfg(windows)]
        crate::windows::make_private(&temporary)?;
        // The parent is owner-only; replacing a reparse point is never permitted.
        if path.exists() {
            verify_regular(path, false)?;
        }
        #[cfg(windows)]
        if path.exists() {
            crate::windows::replace_file(&temporary, path)?;
        } else {
            fs::rename(&temporary, path)?;
        }
        #[cfg(unix)]
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    result
}
pub fn copy_executable(source: &Path, destination: &Path) -> Result<()> {
    verify_regular(source, true)?;
    if fs::metadata(source)?.len() < 4 {
        return Err(Error(Code::InstallConflict));
    }
    #[cfg(target_os = "macos")]
    {
        let parent = destination.parent().ok_or(Error(Code::UnsafePath))?;
        if exists(parent)? {
            verify_dir(parent)?;
        } else {
            ensure_dir(parent)?;
        }
        if exists(destination)? {
            verify_regular(destination, false)?;
        }
        let temporary = parent.join(format!(
            ".kwel-copy-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let status = std::process::Command::new("/usr/bin/ditto")
            .args(["--rsrc", "--extattr", "--acl"])
            .arg(source)
            .arg(&temporary)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            let _ = fs::remove_file(&temporary);
            return Err(Error(Code::InstallConflict));
        }
        let result = (|| {
            verify_regular(&temporary, false)?;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700))?;
            File::open(&temporary)?.sync_all()?;
            if destination.exists() {
                verify_regular(destination, false)?;
            }
            fs::rename(&temporary, destination)?;
            Ok(())
        })();
        let _ = fs::remove_file(&temporary);
        result
    }
    #[cfg(not(target_os = "macos"))]
    {
        atomic_write(destination, &fs::read(source)?, true)
    }
}
pub fn remove_regular(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        return Ok(());
    }
    verify_regular(path, false)?;
    fs::remove_file(path)?;
    Ok(())
}
