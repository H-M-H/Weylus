use std::path::Path;
use std::process::Command;

fn build_ffmpeg() {
    if Path::new("deps/dist").exists() {
        return;
    }

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
    build_ffmpeg();

    println!("cargo:rerun-if-changed=ts/lib.ts");

    #[cfg(not(target_os = "windows"))]
    let mut tsc_command = Command::new("tsc");

    #[cfg(target_os = "windows")]
    let mut tsc_command = Command::new("bash");
    #[cfg(target_os = "windows")]
    tsc_command.arg("tsc");

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

    println!("cargo:rerun-if-changed=lib/error.h");
    println!("cargo:rerun-if-changed=lib/error.c");
    cc::Build::new().file("lib/error.c").compile("error");

    println!("cargo:rerun-if-changed=lib/encode_video.c");
    cc::Build::new()
        .file("lib/encode_video.c")
        .include("deps/dist/include")
        .compile("video");
    println!("cargo:rustc-link-lib=static=avcodec");
    println!("cargo:rustc-link-lib=static=avdevice");
    println!("cargo:rustc-link-lib=static=avfilter");
    println!("cargo:rustc-link-lib=static=avformat");
    println!("cargo:rustc-link-lib=static=avutil");
    println!("cargo:rustc-link-lib=static=postproc");
    println!("cargo:rustc-link-lib=static=swresample");
    println!("cargo:rustc-link-lib=static=swscale");
    println!("cargo:rustc-link-lib=static=x264");
    println!("cargo:rustc-link-search=deps/dist/lib");

    #[cfg(target_os = "linux")]
    linux();
}

#[cfg(target_os = "linux")]
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
}
