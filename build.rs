use std::env;
use std::path::Path;
use std::process::Command;

fn build_ffmpeg() {
    if Path::new("deps/dist").exists() {
        return;
    }

    Command::new("bash")
        .arg(Path::new("clean.sh"))
        .current_dir("deps")
        .status()
        .expect("Failed to run clean ffmpeg build!");

    if !Command::new("bash")
        .arg(Path::new("build.sh"))
        .current_dir("deps")
        .status()
        .expect("Failed to run bash!")
        .success()
    {
        println!("cargo:warning=Failed to build ffmpeg!");
        std::process::exit(1);
    }
}

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    if env::var("CARGO_FEATURE_FFMPEG_SYSTEM").is_err() {
        build_ffmpeg();
    }

    println!("cargo:rerun-if-changed=ts/lib.ts");

    #[cfg(not(target_os = "windows"))]
    let mut tsc_command = Command::new("tsc");

    #[cfg(target_os = "windows")]
    let mut tsc_command = Command::new("bash");
    #[cfg(target_os = "windows")]
    tsc_command.args(&["-c", "tsc"]);

    let js_needs_update = || -> Result<bool, Box<dyn std::error::Error>> {
        Ok(Path::new("ts/lib.ts").metadata()?.modified()?
            > Path::new("www/static/lib.js").metadata()?.modified()?)
    }()
    .unwrap_or(true);

    if js_needs_update {
        match tsc_command.status() {
            Err(err) => {
                println!("cargo:warning=Failed to call tsc: {}", err);
                std::process::exit(1);
            }
            Ok(status) => {
                if !status.success() {
                    match status.code() {
                        Some(code) => println!("cargo:warning=tsc failed with exitcode: {}", code),
                        None => println!("cargo:warning=tsc terminated by signal."),
                    };
                    std::process::exit(2);
                }
            }
        }
    }

    println!("cargo:rerun-if-changed=lib/encode_video.c");
    let mut cc_video = cc::Build::new();
    cc_video.file("lib/encode_video.c");
    cc_video.include("deps/dist/include");
    if ["linux", "windows"].contains(&target_os.as_str()) {
        cc_video.define("HAS_NVENC", None);
    }
    if target_os == "linux" {
        cc_video.define("HAS_VAAPI", None);
    }
    if target_os == "macos" {
        cc_video.define("HAS_VIDEOTOOLBOX", None);
    }
    if target_os == "windows" {
        cc_video.define("HAS_MEDIAFOUNDATION", None);
    }
    cc_video.compile("video");

    println!("cargo:rerun-if-changed=lib/error.h");
    println!("cargo:rerun-if-changed=lib/error.c");
    println!("cargo:rerun-if-changed=lib/log.h");
    println!("cargo:rerun-if-changed=lib/log.c");
    cc::Build::new().file("lib/error.c").compile("error");
    cc::Build::new().file("lib/log.c").compile("log");

    let ffmpeg_link_kind =
        // https://github.com/rust-lang/rust/pull/72785
        // https://users.rust-lang.org/t/linking-on-windows-without-wholearchive/49846/3
        if cfg!(target_os = "windows") ||
            env::var("CARGO_FEATURE_FFMPEG_SYSTEM").is_ok() {
            "dylib"
        } else {
            "static"
        };
    println!("cargo:rustc-link-lib={}=avdevice", ffmpeg_link_kind);
    println!("cargo:rustc-link-lib={}=avformat", ffmpeg_link_kind);
    println!("cargo:rustc-link-lib={}=avfilter", ffmpeg_link_kind);
    println!("cargo:rustc-link-lib={}=avcodec", ffmpeg_link_kind);
    println!("cargo:rustc-link-lib={}=swresample", ffmpeg_link_kind);
    println!("cargo:rustc-link-lib={}=swscale", ffmpeg_link_kind);
    println!("cargo:rustc-link-lib={}=avutil", ffmpeg_link_kind);
    println!("cargo:rustc-link-lib={}=postproc", ffmpeg_link_kind);
    println!("cargo:rustc-link-lib={}=x264", ffmpeg_link_kind);
    if env::var("CARGO_FEATURE_FFMPEG_SYSTEM").is_err() {
        println!("cargo:rustc-link-search=deps/dist/lib");
    }

    if target_os == "linux" {
        linux();
    }

    if target_os == "macos" {
        println!("cargo:rustc-link-lib=framework=VideoToolbox");
        println!("cargo:rustc-link-lib=framework=CoreMedia");
    }

    if target_os == "windows" {
        println!("cargo:rustc-link-lib=dylib=mfplat");
        println!("cargo:rustc-link-lib=dylib=mfuuid");
        println!("cargo:rustc-link-lib=dylib=ole32");
        println!("cargo:rustc-link-lib=dylib=strmiids");
        println!("cargo:rustc-link-lib=dylib=vfw32");
        println!("cargo:rustc-link-lib=dylib=shlwapi");
        println!("cargo:rustc-link-lib=dylib=bcrypt");
    }
}

fn linux() {
    println!("cargo:rerun-if-changed=lib/linux/uniput.c");
    println!("cargo:rerun-if-changed=lib/linux/xcapture.c");
    println!("cargo:rerun-if-changed=lib/linux/xhelper.c");
    println!("cargo:rerun-if-changed=lib/linux/xhelper.h");

    cc::Build::new()
        .file("lib/linux/uinput.c")
        .file("lib/linux/xcapture.c")
        .file("lib/linux/xhelper.c")
        .compile("linux");

    println!("cargo:rustc-link-lib=X11");
    println!("cargo:rustc-link-lib=Xext");
    println!("cargo:rustc-link-lib=Xrandr");
    println!("cargo:rustc-link-lib=Xfixes");
    println!("cargo:rustc-link-lib=Xcomposite");
    println!("cargo:rustc-link-lib=Xi");
    let va_link_kind = if env::var("CARGO_FEATURE_VA_STATIC").is_ok() {
        "static"
    } else {
        "dylib"
    };
    println!("cargo:rustc-link-lib={}=va", va_link_kind);
    println!("cargo:rustc-link-lib={}=va-drm", va_link_kind);
    println!("cargo:rustc-link-lib={}=va-x11", va_link_kind);
    println!("cargo:rustc-link-lib=drm");
}
