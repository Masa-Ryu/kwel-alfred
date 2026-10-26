mod support;
#[cfg(unix)]
use kwel_native_host::ipc;
use kwel_native_host::{
    install::{self, Layout, Manifest},
    paths::{self, RuntimePaths},
    secure,
};
use kwel_protocol::{Code, EXTENSION_ID, Error, HOST_NAME, LEGACY_HOST_NAME};
use std::fs;
#[cfg(unix)]
use std::path::Path;
fn fixture() -> (tempfile::TempDir, Layout, RuntimePaths, std::path::PathBuf) {
    let temporary = support::directory();
    let base = temporary.path().canonicalize().unwrap();
    let home = base.join("home");
    secure::ensure_dir(&home).unwrap();
    let layout = Layout::for_home(&home);
    let runtime = RuntimePaths::at(base.join("runtime")).unwrap();
    let source = base.join("source");
    secure::ensure_dir(&source).unwrap();
    for name in [paths::native_binary_name(), paths::client_binary_name()] {
        secure::atomic_write(&source.join(name), b"executable test fixture", true).unwrap();
    }
    (temporary, layout, runtime, source)
}
#[test]
fn installation_is_idempotent_exact_origin_and_removes_only_owned_files() {
    let (_temp, layout, runtime, source) = fixture();
    assert!(install::install(&source, &layout, &runtime, false).unwrap());
    assert!(!install::install(&source, &layout, &runtime, false).unwrap());
    for name in [HOST_NAME, LEGACY_HOST_NAME] {
        let value: Manifest =
            serde_json::from_slice(&secure::read(&layout.manifest(name)).unwrap()).unwrap();
        assert_eq!(value.path, layout.binary());
        assert_eq!(
            value.allowed_origins,
            vec![format!("chrome-extension://{EXTENSION_ID}/")]
        );
    }
    let unrelated = layout.root.join("other-launcher-data");
    secure::atomic_write(&unrelated, b"retain", false).unwrap();
    install::uninstall(&layout, &runtime).unwrap();
    assert!(unrelated.exists());
    assert!(!layout.binary().exists());
    assert!(!layout.manifest(HOST_NAME).exists());
}
#[test]
fn session_lifetime_lock_prevents_install_or_uninstall_races_but_allows_reuse() {
    #[cfg(unix)]
    if std::env::var_os("KWEL_SESSION_LOCK_FIXTURE").is_none() {
        // A concurrent test's fork can briefly inherit our flock until exec,
        // even with CLOEXEC. Exercise this lifecycle in a child that runs only
        // this test, so closing a guard synchronizes with the last lock owner.
        // Keep every in-use/reuse/removal assertion, without retrying a lock.
        assert!(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "session_lifetime_lock_prevents_install_or_uninstall_races_but_allows_reuse"
                ])
                .env("KWEL_SESSION_LOCK_FIXTURE", "1")
                .status()
                .unwrap()
                .success()
        );
        return;
    }
    let (_temp, layout, runtime, source) = fixture();
    install::install(&source, &layout, &runtime, false).unwrap();
    let guard = install::session_guard(&layout, &layout.binary())
        .unwrap()
        .unwrap();
    assert!(!install::install(&source, &layout, &runtime, true).unwrap());
    assert!(matches!(
        install::install(&source, &layout, &runtime, false),
        Err(Error(Code::HostInUse))
    ));
    assert!(matches!(
        install::uninstall(&layout, &runtime),
        Err(Error(Code::HostInUse))
    ));
    drop(guard);
    install::uninstall(&layout, &runtime).unwrap();
}
#[test]
fn newer_host_is_reused_and_partial_newer_update_cannot_be_downgraded() {
    let (_temp, layout, runtime, source) = fixture();
    install::install(&source, &layout, &runtime, false).unwrap();
    let path = layout.root.join("install.json");
    let mut record: install::Record =
        serde_json::from_slice(&secure::read(&path).unwrap()).unwrap();
    record.version = "9.0.0".into();
    secure::atomic_write(&path, &serde_json::to_vec(&record).unwrap(), false).unwrap();
    assert!(!install::install(&source, &layout, &runtime, true).unwrap());
    assert_eq!(
        install::installed_version(&layout).unwrap().unwrap(),
        "9.0.0"
    );
    secure::atomic_write(
        &layout.root.join("pending-install.json"),
        &serde_json::to_vec(&record).unwrap(),
        false,
    )
    .unwrap();
    assert!(matches!(
        install::install(&source, &layout, &runtime, true),
        Err(Error(Code::DowngradeRefused))
    ));
}
#[test]
fn updated_workflow_and_interrupted_install_finish_only_journalled_binaries() {
    use sha2::{Digest, Sha256};
    let (_temp, layout, runtime, source) = fixture();
    install::install(&source, &layout, &runtime, false).unwrap();
    let mut next: install::Record =
        serde_json::from_slice(&secure::read(&layout.root.join("install.json")).unwrap()).unwrap();
    for name in [paths::native_binary_name(), paths::client_binary_name()] {
        secure::atomic_write(&source.join(name), b"updated executable fixture", true).unwrap();
        next.binaries.insert(
            name.into(),
            secure::hex(&Sha256::digest(fs::read(source.join(name)).unwrap())),
        );
    }
    // Emulate a power loss after the first binary's atomic replacement.
    secure::atomic_write(
        &layout.root.join("pending-install.json"),
        &serde_json::to_vec(&next).unwrap(),
        false,
    )
    .unwrap();
    secure::copy_executable(&source.join(paths::native_binary_name()), &layout.binary()).unwrap();
    assert!(install::install(&source, &layout, &runtime, false).unwrap());
    for name in [paths::native_binary_name(), paths::client_binary_name()] {
        assert_eq!(
            fs::read(layout.root.join("bin").join(name)).unwrap(),
            b"updated executable fixture"
        );
    }
    assert!(!layout.root.join("pending-install.json").exists());
    assert!(!install::install(&source, &layout, &runtime, false).unwrap());
}
#[test]
fn unrecognized_manifests_and_modified_binaries_are_never_overwritten_or_removed() {
    let (_temp, layout, runtime, source) = fixture();
    install::install(&source, &layout, &runtime, false).unwrap();
    let path = layout.manifest(HOST_NAME);
    let before = secure::read(&path).unwrap();
    let mut value: Manifest = serde_json::from_slice(&before).unwrap();
    value.path = layout.root.join("different-product");
    secure::atomic_write(&path, &serde_json::to_vec(&value).unwrap(), false).unwrap();
    assert!(install::install(&source, &layout, &runtime, false).is_err());
    assert!(install::uninstall(&layout, &runtime).is_err());
    assert!(layout.binary().exists());
    secure::atomic_write(&path, &before, false).unwrap();
    secure::atomic_write(&layout.binary(), b"unrecognized replacement", true).unwrap();
    assert!(install::install(&source, &layout, &runtime, false).is_err());
    assert!(install::uninstall(&layout, &runtime).is_err());
}
#[test]
fn legacy_registration_migrates_without_deleting_legacy_binary() {
    let (_temp, layout, runtime, source) = fixture();
    secure::atomic_write(&layout.legacy_binary, b"legacy bridge fixture", true).unwrap();
    let old = Manifest {
        name: LEGACY_HOST_NAME.into(),
        description: "Kwel Alfred Integration".into(),
        path: layout.legacy_binary.clone(),
        kind: "stdio".into(),
        allowed_origins: vec![format!("chrome-extension://{EXTENSION_ID}/")],
    };
    secure::atomic_write(
        &layout.manifest(LEGACY_HOST_NAME),
        &serde_json::to_vec(&old).unwrap(),
        false,
    )
    .unwrap();
    install::install(&source, &layout, &runtime, false).unwrap();
    let updated: Manifest =
        serde_json::from_slice(&secure::read(&layout.manifest(LEGACY_HOST_NAME)).unwrap()).unwrap();
    assert_eq!(updated.path, layout.binary());
    assert_eq!(
        fs::read(&layout.legacy_binary).unwrap(),
        b"legacy bridge fixture"
    );
}
#[tokio::test]
async fn in_use_host_is_never_deleted() {
    let (_temp, layout, runtime, source) = fixture();
    install::install(&source, &layout, &runtime, false).unwrap();
    let profile = "00000000-0000-4000-8000-00000000000a";
    let guard = secure::lock(&runtime.file(profile, "lock").unwrap()).unwrap();
    guard.try_lock().unwrap();
    #[cfg(unix)]
    let _listener = ipc::Listener::bind(&runtime.endpoint(profile, false).unwrap()).unwrap();
    #[cfg(windows)]
    secure::atomic_write(&runtime.file(profile, "endpoint").unwrap(), b"1", false).unwrap();
    assert!(matches!(
        install::uninstall(&layout, &runtime),
        Err(Error(Code::HostInUse))
    ));
    assert!(layout.binary().exists());
}
#[cfg(unix)]
#[tokio::test]
async fn socket_permissions_stale_recovery_and_replacement_ownership() {
    use std::os::unix::fs::PermissionsExt;
    if let Some(path) = std::env::var_os("KWEL_SOCKET_EXIT_FIXTURE") {
        let _socket = std::os::unix::net::UnixListener::bind(path).unwrap();
        // Simulate process death, leaving the socket pathname behind.
        std::process::exit(0);
    }
    let temp = support::directory();
    let runtime = RuntimePaths::at(temp.path().canonicalize().unwrap()).unwrap();
    let path = runtime
        .endpoint("00000000-0000-4000-8000-00000000000a", false)
        .unwrap();
    // Other tests concurrently spawn ditto. A socket briefly present in this
    // process can be inherited across fork until exec, even with CLOEXEC. Make
    // the stale socket in a dedicated child that never forks after binding;
    // wait for process exit instead of sleeping or weakening the Busy check.
    assert!(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "socket_permissions_stale_recovery_and_replacement_ownership"
            ])
            .env("KWEL_SOCKET_EXIT_FIXTURE", &path)
            .status()
            .unwrap()
            .success()
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let listener = ipc::Listener::bind(&path).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&runtime.directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let renamed = runtime.directory.join("old.sock");
    fs::rename(&path, &renamed).unwrap();
    let replacement = ipc::Listener::bind(&path).unwrap();
    drop(listener);
    assert!(path.exists());
    drop(replacement);
    assert!(!path.exists());
}
#[cfg(unix)]
#[test]
fn symlink_and_foreign_owner_checks_fail_closed() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let (_temp, layout, runtime, source) = fixture();
    secure::ensure_dir(&layout.manifests).unwrap();
    symlink(
        source.join(paths::native_binary_name()),
        layout.manifest(HOST_NAME),
    )
    .unwrap();
    assert!(install::install(&source, &layout, &runtime, false).is_err());
    let redirect = layout.root.join("redirect");
    symlink(&source, &redirect).unwrap();
    assert!(secure::ensure_dir(&redirect).is_err());
    let foreign = if cfg!(target_os = "macos") {
        Path::new("/private/etc/passwd")
    } else {
        Path::new("/etc/passwd")
    };
    assert!(secure::verify_regular(foreign, false).is_err());
    fs::set_permissions(&runtime.directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(secure::verify_dir(&runtime.directory).is_err());
}
#[cfg(unix)]
#[tokio::test]
async fn atomic_file_operations_reject_symlinks_unsafe_parents_and_socket_type_confusion() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let temp = support::directory();
    let root = temp.path().canonicalize().unwrap();
    let data = root.join("file");
    secure::atomic_write(&data, b"original", false).unwrap();
    assert_eq!(
        fs::metadata(&data).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let executable = root.join("executable");
    secure::copy_executable(&data, &executable).unwrap();
    assert_eq!(
        fs::metadata(&executable).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let link = root.join("link");
    symlink(&data, &link).unwrap();
    assert!(secure::atomic_write(&link, b"unsafe", false).is_err());
    assert!(secure::copy_executable(&link, &executable).is_err());
    assert!(secure::copy_executable(&data, &link).is_err());
    assert!(secure::remove_regular(&link).is_err());
    let dangling = root.join("dangling");
    symlink(root.join("missing"), &dangling).unwrap();
    assert!(secure::atomic_write(&dangling, b"unsafe", false).is_err());
    let socket = root.join("s");
    let _listener = ipc::Listener::bind(&socket).unwrap();
    assert!(secure::verify_regular(&socket, false).is_err());
    assert!(ipc::Listener::bind(&data).is_err());
    assert!(secure::no_symlink_components(&root.join("../escape")).is_err());
    let runtime = root.join("runtime");
    RuntimePaths::at(runtime.clone()).unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(RuntimePaths::at(runtime).is_err());
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(secure::atomic_write(&data, b"unsafe", false).is_err());
    assert!(secure::copy_executable(&data, &executable).is_err());
    assert_eq!(fs::read(&data).unwrap(), b"original");
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
}
#[cfg(target_os = "macos")]
#[test]
fn macos_copy_preserves_quarantine_extended_attributes_and_resource_forks() {
    use std::process::Command;
    let (_temp, layout, _runtime, source) = fixture();
    let original = source.join(paths::native_binary_name());
    for (key, value) in [
        ("com.apple.quarantine", "0081;00000000;KwelFixture;"),
        ("com.kwel.fixture", "metadata"),
        ("com.apple.ResourceFork", "fixture resource fork"),
    ] {
        assert!(
            Command::new("/usr/bin/xattr")
                .args(["-w", key, value])
                .arg(&original)
                .status()
                .unwrap()
                .success()
        );
    }
    secure::copy_executable(&original, &layout.binary()).unwrap();
    for key in [
        "com.apple.quarantine",
        "com.kwel.fixture",
        "com.apple.ResourceFork",
    ] {
        let before = Command::new("/usr/bin/xattr")
            .args(["-p", key])
            .arg(&original)
            .output()
            .unwrap();
        let after = Command::new("/usr/bin/xattr")
            .args(["-p", key])
            .arg(layout.binary())
            .output()
            .unwrap();
        assert!(before.status.success() && after.status.success());
        if key == "com.apple.quarantine" {
            // macOS may augment propagation flags and refresh the quarantine
            // timestamp/agent during ditto. Retain every source quarantine flag;
            // do not remove the attribute or reset OS-added protection flags.
            let flags = |value: &[u8]| {
                u32::from_str_radix(
                    std::str::from_utf8(value)
                        .unwrap()
                        .split(';')
                        .next()
                        .unwrap(),
                    16,
                )
                .unwrap()
            };
            assert_eq!(
                flags(&after.stdout) & flags(&before.stdout),
                flags(&before.stdout)
            );
        } else {
            assert_eq!(before.stdout, after.stdout);
        }
    }
}
