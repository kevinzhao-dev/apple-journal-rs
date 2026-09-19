use std::{env, path::PathBuf, process::Command};
fn main() {
    assert_eq!(
        env::var("CARGO_CFG_TARGET_OS").unwrap(),
        "macos",
        "journal-rs requires macOS and Apple frameworks"
    );
    println!("cargo:rerun-if-changed=native");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    println!("cargo:rerun-if-env-changed=SDKROOT");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let compiler = Command::new("xcrun")
        .args(["--find", "swiftc"])
        .output()
        .expect("Xcode command line tools required");
    assert!(compiler.status.success());
    let compiler = PathBuf::from(String::from_utf8(compiler.stdout).unwrap().trim());
    let runtime = compiler
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("lib/swift/macosx");
    let sdk = Command::new("xcrun")
        .arg("--show-sdk-path")
        .output()
        .unwrap();
    assert!(
        sdk.status.success(),
        "cannot locate macOS SDK: {}",
        String::from_utf8_lossy(&sdk.stderr)
    );
    let sdk = String::from_utf8(sdk.stdout).unwrap();
    let status = Command::new(&compiler)
        .args(["-sdk", sdk.trim()])
        .args([
            "-emit-library",
            "-static",
            "-O",
            "-module-name",
            "JournalBridge",
            "native/Bridge.swift",
            "-o",
        ])
        .arg(out.join("libJournalBridge.a"))
        .arg("-module-cache-path")
        .arg(out.join("swift-cache"))
        .env("CLANG_MODULE_CACHE_PATH", out.join("clang-cache"))
        .status()
        .expect("compile macOS bridge");
    assert!(status.success(), "macOS bridge compilation failed");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-search=native={}", runtime.display());
    println!("cargo:rustc-link-lib=static=JournalBridge");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    for framework in [
        "Foundation",
        "AppKit",
        "ImageIO",
        "UniformTypeIdentifiers",
        "LinkPresentation",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
