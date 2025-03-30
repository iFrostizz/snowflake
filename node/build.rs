use std::env;
use std::process::Command;

fn main() {
    let res = Command::new("rustc").arg("--version").output().expect("failed to run rustc --version");
    let stdout = String::from_utf8(res.stdout).expect("invalid stdout");
    let version = stdout
        .split_whitespace()
        .take(2)
        .fold(String::new(), |acc, x| acc + x);
    eprintln!("{}", &version);
    println!("cargo:rustc-env={}={}", "RUSTC_VERSION", version);
}