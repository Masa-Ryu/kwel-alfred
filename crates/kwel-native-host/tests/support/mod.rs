pub fn directory() -> tempfile::TempDir {
    // macOS sockaddr_un is short. Use the same short, sticky-root location as
    // production, with a unique owned directory and no production profile IDs.
    #[cfg(target_os = "macos")]
    let root = std::path::PathBuf::from("/private/tmp");
    #[cfg(target_os = "linux")]
    let root = std::path::PathBuf::from("/tmp");
    #[cfg(windows)]
    let root = std::env::temp_dir();
    tempfile::Builder::new()
        .prefix("kwct-")
        .tempdir_in(root)
        .unwrap()
}
