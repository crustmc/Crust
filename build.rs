fn main() {
    println!("cargo:rustc-env=BUILD_NUMBER={}", std::env::var("BUILD_NUMBER").unwrap());
    println!("cargo:rustc-env=GIT_COMMIT={}", std::env::var("GIT_COMMIT").unwrap());
}
