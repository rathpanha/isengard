fn main() {
    // Embed the app icon into the Windows executable; other platforms need nothing here.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/windows/isengard.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("failed to embed Windows resources");
    }
    println!("cargo:rerun-if-changed=assets/windows/isengard.rc");
    println!("cargo:rerun-if-changed=assets/logo/Isengard.ico");
}
