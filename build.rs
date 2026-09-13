fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").ok().as_deref() != Some("macos") {
        return;
    }
    println!("cargo:rerun-if-changed=src/macos/sparkle.m");
    cc::Build::new()
        .file("src/macos/sparkle.m")
        .flag("-fobjc-arc")
        .compile("folio_sparkle");
    println!("cargo:rustc-link-lib=framework=Foundation");
}
