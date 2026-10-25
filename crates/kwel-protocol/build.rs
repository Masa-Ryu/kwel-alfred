fn main() {
    let path = std::path::Path::new("../../config/distribution.json");
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("distribution configuration"))
            .expect("valid distribution JSON");
    let extension = config["chromeExtensionId"]
        .as_str()
        .expect("configured extension ID");
    assert!(
        extension.len() == 32 && extension.bytes().all(|b| (b'a'..=b'p').contains(&b)),
        "invalid extension ID"
    );
    println!("cargo:rustc-env=KWEL_EXTENSION_ID={extension}");
    println!("cargo:rerun-if-changed={}", path.display());
}
