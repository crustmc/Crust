fn main() {
    let bn = std::env::var("BUILD_NUMBER").unwrap_or("".to_string());
    let gc = std::env::var("GIT_COMMIT").unwrap_or("".to_string());
    println!("cargo:rustc-env=BUILD_NUMBER={}", bn);
    println!("cargo:rustc-env=GIT_COMMIT={}", gc);

    let mut full_name = "Crust".to_string();
    if gc.is_empty() {
        full_name += ":unknown";
    } else {
        full_name += &format!(":{}", gc);
    }
    if !bn.is_empty() {
        full_name += &format!(":{}", bn);
    }
    println!("cargo:rustc-env=FULL_NAME={}", full_name);
}
