use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn directory_contains_file<F>(dir: &Path, matcher: F) -> bool
where
    F: Fn(&str) -> bool,
{
    fs::read_dir(dir)
        .ok()
        .map(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                entry.file_type().map(|ft| ft.is_file()).unwrap_or(false)
                    && entry.file_name().to_str().map(&matcher).unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn has_linux_soia_utils_runtime(dir: &Path) -> bool {
    directory_contains_file(dir, |name| name.starts_with("libsoia_utils.so"))
}

fn has_android_soia_utils_runtime(dir: &Path) -> bool {
    directory_contains_file(dir, |name| name == "libsoia_utils.so")
}

fn android_abi_for_target(target_triple: &str) -> Option<&'static str> {
    match target_triple {
        "aarch64-linux-android" => Some("arm64-v8a"),
        "armv7-linux-androideabi" => Some("armeabi-v7a"),
        "i686-linux-android" => Some("x86"),
        "x86_64-linux-android" => Some("x86_64"),
        _ => None,
    }
}

fn has_windows_soia_utils_runtime(dir: &Path) -> bool {
    directory_contains_file(dir, |name| {
        let lower = name.to_ascii_lowercase();
        lower.ends_with(".dll") && lower.contains("soia_utils")
    })
}

fn has_windows_ffmpeg_import_library(dir: &Path, name: &str) -> bool {
    [
        format!("{name}.lib"),
        format!("lib{name}.dll.a"),
        format!("{name}.dll.a"),
    ]
    .iter()
    .any(|candidate| dir.join(candidate).exists())
}

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let manifest_path = PathBuf::from(manifest_dir.clone());
    let desktop_mpv_lib_dir = manifest_path.join("libs").join("mpv");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_triple = env::var("TARGET").unwrap_or_default();
    let mpv_lib_dir = if target_os == "android" {
        let abi = android_abi_for_target(&target_triple).unwrap_or_else(|| {
            panic!(
                "\n[!] Unsupported Android target '{}'. Expected one of: aarch64-linux-android, armv7-linux-androideabi, i686-linux-android, x86_64-linux-android\n",
                target_triple
            );
        });
        manifest_path
            .join("libs")
            .join("mpv")
            .join("android")
            .join(abi)
    } else {
        desktop_mpv_lib_dir
    };
    let config_file = mpv_lib_dir.join("config.data");

    let windows_link_name = if mpv_lib_dir.join("mpv.lib").exists() {
        Some("mpv")
    } else if mpv_lib_dir.join("mpv-2.lib").exists() {
        Some("mpv-2")
    } else if mpv_lib_dir.join("libmpv.lib").exists() {
        Some("libmpv")
    } else if mpv_lib_dir.join("libmpv-2.lib").exists() {
        Some("libmpv-2")
    } else if mpv_lib_dir.join("libmpv.dll.a").exists() || mpv_lib_dir.join("mpv.dll.a").exists() {
        Some("mpv")
    } else if mpv_lib_dir.join("libmpv-2.dll.a").exists()
        || mpv_lib_dir.join("mpv-2.dll.a").exists()
    {
        Some("mpv-2")
    } else {
        None
    };

    let has_runtime = match target_os.as_str() {
        "macos" => {
            mpv_lib_dir.join("libmpv.dylib").exists() || mpv_lib_dir.join("libmpv.2.dylib").exists()
        }
        "windows" => windows_link_name.is_some(),
        "linux" => {
            mpv_lib_dir.join("libmpv.so").exists()
                || mpv_lib_dir.join("libmpv.so.2").exists()
                || mpv_lib_dir.join("libmpv.so.1").exists()
        }
        "android" => mpv_lib_dir.join("libmpv.so").exists(),
        _ => mpv_lib_dir.join("libmpv.dylib").exists(),
    };

    if !has_runtime {
        panic!(
            "\n[!] Cannot find libmpv runtime/import library for target '{}'. Please run: pnpm setup:libs\n",
            target_triple
        );
    }

    println!("cargo:rerun-if-env-changed=SOIA_API");
    println!("cargo:rerun-if-changed={}", config_file.display());
    let api = env::var("SOIA_API")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| {
            fs::read(&config_file).ok().and_then(|bytes| {
                let data: Vec<u8> = bytes
                    .into_iter()
                    .enumerate()
                    .map(|(i, b)| b ^ b"HTUA_AI0S"[i % 9])
                    .collect();
                String::from_utf8(data).ok()
            })
        })
        .unwrap_or_else(|| String::new())
        .replace('\r', "");
    println!("cargo:rustc-env=SOIA_API={}", api);

    println!("cargo:rustc-link-search=native={}", mpv_lib_dir.display());
    if target_os == "windows" {
        let link_name = windows_link_name.unwrap_or("mpv");
        println!("cargo:rustc-link-lib=dylib={}", link_name);
    } else {
        println!("cargo:rustc-link-lib=mpv");
    }

    // The casting pipeline calls libavformat/libavcodec directly. Keep these links next to
    // libmpv so both APIs always use the exact FFmpeg runtime shipped with the application.
    // macOS bundles versioned dylibs, while the other targets expose the usual linker names.
    match target_os.as_str() {
        "macos" => {
            println!("cargo:rustc-link-lib=dylib=avformat.62.12.100");
            println!("cargo:rustc-link-lib=dylib=avcodec.62.28.100");
            println!("cargo:rustc-link-lib=dylib=avutil.60.26.100");
            println!("cargo:rustc-link-lib=dylib=swscale.9.5.100");
        }
        _ => {
            println!("cargo:rustc-link-lib=dylib=avformat");
            println!("cargo:rustc-link-lib=dylib=avcodec");
            println!("cargo:rustc-link-lib=dylib=avutil");
            println!("cargo:rustc-link-lib=dylib=swscale");
        }
    }

    match target_os.as_str() {
        "macos" => {
            let soia_utils_dylib = mpv_lib_dir.join("libsoia_utils.dylib");
            if !soia_utils_dylib.exists() {
                panic!(
                    "\n[!] Cannot find libsoia_utils.dylib for target '{}'.\n    Expected in libs/mpv: {}\n    Please run: pnpm setup:libs\n",
                    target_triple,
                    soia_utils_dylib.display()
                );
            }
            println!("cargo:rustc-link-search=native={}", mpv_lib_dir.display());
            println!("cargo:rustc-link-lib=dylib=soia_utils");
        }
        "linux" => {
            if !has_linux_soia_utils_runtime(&mpv_lib_dir) {
                panic!(
                    "\n[!] Cannot find libsoia_utils.so* for target '{}'.\n    Expected under libs/mpv: {}\n    Please run: pnpm setup:libs\n",
                    target_triple,
                    mpv_lib_dir.display()
                );
            }
            println!("cargo:rustc-link-search=native={}", mpv_lib_dir.display());
            println!("cargo:rustc-link-lib=dylib=soia_utils");
        }
        "android" => {
            if !has_android_soia_utils_runtime(&mpv_lib_dir) {
                panic!(
                    "\n[!] Cannot find libsoia_utils.so for target '{}'.\n    Expected under libs/mpv/android: {}\n",
                    target_triple,
                    mpv_lib_dir.display()
                );
            }
            println!("cargo:rustc-link-search=native={}", mpv_lib_dir.display());
            println!("cargo:rustc-link-lib=dylib=soia_utils");
            println!("cargo:rustc-link-lib=dylib=avutil");
        }
        "windows" => {
            for library in ["avformat", "avcodec", "avutil", "swscale"] {
                if !has_windows_ffmpeg_import_library(&mpv_lib_dir, library) {
                    panic!(
                        "\n[!] Cannot find FFmpeg import library for '{}'.\n    Expected '{}.lib' or 'lib{}.dll.a' under: {}\n    Please run: pnpm setup:libs\n",
                        library,
                        library,
                        library,
                        mpv_lib_dir.display()
                    );
                }
            }

            if !has_windows_soia_utils_runtime(&mpv_lib_dir) {
                panic!(
                    "\n[!] Cannot find soia_utils.dll for target '{}'.\n    Expected under libs/mpv: {}\n    Please run: pnpm setup:libs\n",
                    target_triple,
                    mpv_lib_dir.display()
                );
            }

            if !has_windows_ffmpeg_import_library(&mpv_lib_dir, "soia_utils") {
                panic!(
                    "\n[!] Cannot find soia_utils import library (.lib/.dll.a) for target '{}'.\n    Expected under libs/mpv: {}\n    Please run: pnpm setup:libs\n",
                    target_triple,
                    mpv_lib_dir.display()
                );
            }

            println!("cargo:rustc-link-search=native={}", mpv_lib_dir.display());
            println!("cargo:rustc-link-lib=dylib=soia_utils");
        }
        _ => {}
    }

    println!("cargo:rerun-if-changed=../scripts/setup_runtime_libs_macos.sh");
    println!("cargo:rerun-if-changed=../scripts/setup_runtime_libs_linux.sh");
    println!("cargo:rerun-if-changed=../scripts/setup_runtime_libs_windows.mjs");
    println!("cargo:rerun-if-changed=../scripts/setup_runtime_libs.mjs");
    println!("cargo:rerun-if-changed=libs/mpv");
    println!("cargo:rerun-if-env-changed=TARGET");

    tauri_build::build();
}
