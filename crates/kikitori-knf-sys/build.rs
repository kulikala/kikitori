use cmake::Config;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create destination directory");
    for entry in fs::read_dir(src).expect("read source directory") {
        let entry = entry.expect("read directory entry");
        let target = dst.join(entry.file_name());
        if entry.file_type().expect("read file type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy file");
        }
    }
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let knf_dst = out_dir.join("knf");
    let knfc_dst = out_dir.join("knfc");
    copy_dir(&manifest_dir.join("knf"), &knf_dst);
    copy_dir(&manifest_dir.join("knfc"), &knfc_dst);

    println!("cargo:rerun-if-changed=./knf");
    println!("cargo:rerun-if-changed=./knfc");
    println!("cargo:rerun-if-env-changed=KNF_STATIC_CRT");
    println!("cargo:rerun-if-env-changed=PROFILE");

    let profile = if env::var("PROFILE").as_deref() == Ok("release") {
        "Release"
    } else {
        "Debug"
    };
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_features = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    // The C++ runtime must match the one Rust links, or MSVC rejects the mix (LNK2038).
    let static_crt = env::var("KNF_STATIC_CRT").map_or_else(
        |_| {
            target_features
                .split(',')
                .any(|feature| feature == "crt-static")
        },
        |value| value == "1",
    );
    let mut config = Config::new(&knfc_dst);
    if target_os == "windows" {
        config.static_crt(static_crt);
    }
    config
        .profile(profile)
        .define("CMAKE_POLICY_VERSION_MINIMUM", "3.5")
        .define("CMAKE_CXX_FLAGS", "-Dknf=knf_rs")
        .very_verbose(env::var("CMAKE_VERBOSE").is_ok())
        .build();

    println!("cargo:rustc-link-search={}", out_dir.join("lib").display());
    if matches!(target_os.as_str(), "macos" | "ios" | "freebsd") {
        println!("cargo:rustc-link-lib=c++");
    } else if target_os == "linux" {
        println!("cargo:rustc-link-lib=stdc++");
    }
    println!("cargo:rustc-link-lib=static=knfc");
    println!("cargo:rustc-link-lib=static=kaldi-native-fbank-core");
}
