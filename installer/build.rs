//! Builds in the device files when CGS2_BUNDLE names a tar of them (see scripts/bundle.sh).
fn main() {
    println!("cargo:rerun-if-env-changed=CGS2_BUNDLE");
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("bundle.tar");
    match std::env::var("CGS2_BUNDLE") {
        Ok(path) => {
            println!("cargo:rerun-if-changed={path}");
            std::fs::copy(&path, &out).unwrap_or_else(|e| panic!("{path}: {e}"));
        }
        Err(_) => std::fs::write(&out, b"").unwrap(),
    }
}
