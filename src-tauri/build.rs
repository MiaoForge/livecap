fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_macos_audio();
    }
    tauri_build::build()
}

fn build_macos_audio() {
    use std::{env, path::PathBuf, process::Command};
    println!("cargo:rerun-if-changed=native/AudioCapture.swift");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        arch => panic!("Unsupported macOS architecture: {arch}"),
    };
    let target = format!("{arch}-apple-macosx13.0");
    let object = out.join("AudioCapture.o");
    let status = Command::new("xcrun")
        .args([
            "swiftc",
            "-swift-version",
            "5",
            "-parse-as-library",
            "-O",
            "-emit-object",
            "-target",
            &target,
        ])
        .arg("native/AudioCapture.swift")
        .arg("-o")
        .arg(&object)
        .status()
        .expect("Install Xcode command line tools");
    assert!(status.success(), "Failed to compile macOS audio capture");
    let status = Command::new("xcrun")
        .arg("ar")
        .arg("crs")
        .arg(out.join("liblivecap_audio.a"))
        .arg(&object)
        .status()
        .unwrap();
    assert!(status.success());
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=livecap_audio");
    let info = Command::new("xcrun")
        .args(["swiftc", "-print-target-info", "-target", &target])
        .output()
        .unwrap();
    assert!(info.status.success());
    let info: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap();
    for path in info["paths"]["runtimeLibraryPaths"].as_array().unwrap() {
        println!("cargo:rustc-link-search=native={}", path.as_str().unwrap());
    }
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    // Mach-O objects carry their Swift and framework autolink directives.
    // Apple's linker resolves those using the runtime search paths above.
}
