fn main() {
    // Re-run the build script if the Windows resource script changes.
    println!("cargo:rerun-if-changed=resources/dizmo_editor.rc");

    #[cfg(target_os = "windows")]
    {
        let ico = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("dizmo_editor.ico");
        if ico.exists() {
            embed_resource::compile("resources/dizmo_editor.rc", embed_resource::NONE)
                .manifest_optional()
                .expect("failed to compile Windows icon resource");
        }
    }
}
