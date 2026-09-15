fn main() {
    println!("cargo:rerun-if-changed=packaging/release.env");
    let version = release_var("VERSION");
    let build = release_var("BUILD");
    let public_base = release_var("PUBLIC_BASE");
    println!("cargo:rustc-env=FOLIO_VERSION={version}");
    println!("cargo:rustc-env=FOLIO_BUILD={build}");
    println!("cargo:rustc-env=FOLIO_PUBLIC_BASE={public_base}");

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

fn release_var(name: &str) -> String {
    let text = std::fs::read_to_string("packaging/release.env").unwrap_or_else(|e| {
        panic!("could not read packaging/release.env: {e}");
    });
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{name}=")) {
            return rest.to_string();
        }
    }
    panic!("packaging/release.env is missing {name}");
}
