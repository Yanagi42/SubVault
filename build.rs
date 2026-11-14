fn main() {
    println!("cargo:rerun-if-env-changed=SUBVAULT_GIT_SHA");
    let sha = std::env::var("SUBVAULT_GIT_SHA").unwrap_or_else(|_| "unknown".into());
    println!("cargo:rustc-env=SUBVAULT_GIT_SHA={sha}");
}
