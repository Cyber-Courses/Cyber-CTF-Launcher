fn main() {
    let mut attributes = tauri_build::Attributes::new();
    // On Windows, tauri-build embeds the app manifest (Common Controls v6) in the app binary only,
    // so the unit-test executable ran without it and failed to load (STATUS_ENTRYPOINT_NOT_FOUND:
    // comctl32's TaskDialogIndirect needs v6). Embed the same manifest in every linked target.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        attributes = attributes.windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::try_build(attributes).expect("tauri-build failed");
}
