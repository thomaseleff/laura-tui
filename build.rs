//! Best-effort git short-hash for journal provenance (#11). Empty off a tarball build.
fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=LAURA_COMMIT={hash}");
}
