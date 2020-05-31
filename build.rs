use std::fs;
use std::path::Path;
use std::process::Command;

static FFMPEG_PATH_STR: &str = "deps/ffmpeg";
static FFMPEG_DIST_PATH_STR: &str = "deps/ffmpeg/dist";

static X264_PATH_STR: &str = "deps/x264";
static X264_DIST_PATH_STR: &str = "deps/x264/dist";

fn build_x264() {
    let x264_path = Path::new(X264_PATH_STR);
    let x264_dist_path = Path::new(X264_DIST_PATH_STR);
    if x264_dist_path.exists() {
        return;
    }

    if !x264_path.exists() {
        fs::create_dir_all("deps").expect("Could not create deps directory!");
        if let Err(err) = Command::new("git")
            .arg("clone")
            .arg("-b")
            .arg("stable")
            .arg("https://code.videolan.org/videolan/x264.git")
            .arg(&x264_path)
            .status()
        {
            println!("cargo:warning=Failed to clone libx264: {}", err);
            std::process::exit(1);
        }
        fs::create_dir_all(x264_dist_path).expect("Could not create x264 dist directory!");
    }

    let mut configure_cmd = Command::new("bash");
    configure_cmd
        .current_dir(&x264_path)
        .arg("configure")
        .arg("--prefix=dist")
        .arg("--exec-prefix=dist")
        .arg("--enable-static")
        .arg("--enable-pic")
        .arg("--enable-strip")
        .arg("--disable-cli")
        .arg("--disable-opencl");

    if !configure_cmd
        .status()
        .expect("Failed to configure libx264!")
        .success()
    {
        println!("cargo:warning=Failed to configure libx264!");
        std::process::exit(1);
    }

    if !Command::new("make")
        .current_dir(&x264_path)
        .arg("-j")
        .arg(num_cpus::get().to_string())
        .status()
        .expect("Failed to call make!")
        .success()
    {
        println!("cargo:warning=Failed to make libx264!");
        std::process::exit(1);
    }

    if !Command::new("make")
        .current_dir(&x264_path)
        .arg("install")
        .status()
        .expect("Failed to call make!")
        .success()
    {
        println!("cargo:warning=Failed to make install libx264!");
        std::process::exit(1);
    }
}

fn build_ffmpeg() {
    let ffmpeg_path = Path::new(FFMPEG_PATH_STR);
    let ffmpeg_dist_path = Path::new(FFMPEG_DIST_PATH_STR);

    if ffmpeg_dist_path.exists() {
        return;
    }

    if !ffmpeg_path.exists() {
        fs::create_dir_all("deps").expect("Could not create deps directory!");
        if let Err(err) = Command::new("git")
            .arg("clone")
            .arg("-b")
            .arg("n4.2.3")
            .arg("https://git.ffmpeg.org/ffmpeg.git")
            .arg(&ffmpeg_path)
            .status()
        {
            println!("cargo:warning=Failed to clone ffmpeg: {}", err);
            std::process::exit(1);
        }
        fs::create_dir_all(ffmpeg_dist_path).expect("Could not create ffmpeg dist directory!");
    }

    let mut configure_cmd = Command::new("bash");
    configure_cmd
        .current_dir(&ffmpeg_path)
        .arg("configure")
        .arg("--prefix=dist")
        .arg("--disable-debug")
        .arg("--enable-stripping")
        .arg("--enable-static")
        .arg("--disable-shared")
        .arg("--enable-pic")
        .arg("--disable-programs")
        .arg("--enable-gpl")
        .arg("--enable-libx264")
        .arg("--disable-bzlib")
        .arg("--disable-alsa")
        .arg("--disable-appkit")
        .arg("--disable-avfoundation")
        .arg("--disable-coreimage")
        .arg("--disable-iconv")
        .arg("--disable-libxcb")
        .arg("--disable-libxcb-shm")
        .arg("--disable-libxcb-xfixes")
        .arg("--disable-libxcb-shape")
        .arg("--disable-lzma")
        .arg("--disable-schannel")
        .arg("--disable-sdl2")
        .arg("--disable-securetransport")
        .arg("--disable-xlib")
        .arg("--disable-zlib")
        .arg("--disable-amf")
        .arg("--disable-audiotoolbox")
        .arg("--disable-cuda-llvm")
        .arg("--disable-cuvid")
        .arg("--disable-d3d11va")
        .arg("--disable-dxva2")
        .arg("--disable-ffnvcodec")
        .arg("--disable-nvdec")
        .arg("--disable-nvenc")
        .arg("--disable-vaapi")
        .arg("--disable-vdpau")
        .arg("--disable-videotoolbox")
        .arg("--extra-cflags=-I../x264/dist/include")
        .arg("--extra-ldflags=-L../x264/dist/lib");

    if !configure_cmd
        .status()
        .expect("Failed to configure ffmpeg!")
        .success()
    {
        println!("cargo:warning=Failed to configure ffmpeg!");
        std::process::exit(1);
    }

    if !Command::new("make")
        .current_dir(&ffmpeg_path)
        .arg("-j")
        .arg(num_cpus::get().to_string())
        .arg("VERBOSE=1")
        .status()
        .expect("Failed to call make!")
        .success()
    {
        println!("cargo:warning=Failed to make ffmpeg!");
        let s = fs::read_to_string("deps/ffmpeg/ffbuild/config.log").unwrap();
        println!("cargo:warning={}", s);
        std::process::exit(1);
    }

    if !Command::new("make")
        .current_dir(&ffmpeg_path)
        .arg("install")
        .status()
        .expect("Failed to call make!")
        .success()
    {
        println!("cargo:warning=Failed to make install ffmpeg!");
        std::process::exit(1);
    }
}

fn main() {
    build_x264();
    build_ffmpeg();

    println!("cargo:rerun-if-changed=ts/lib.ts");
    match Command::new("tsc").status() {
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
        .include(format!("{}/include", FFMPEG_DIST_PATH_STR))
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
    println!("cargo:rustc-link-search={}/lib", FFMPEG_DIST_PATH_STR);
    println!("cargo:rustc-link-search={}/lib", X264_DIST_PATH_STR);

    #[cfg(target_os = "linux")]
    linux();
}

#[cfg(target_os = "linux")]
fn linux() {
    println!("cargo:rerun-if-changed=lib/linux/uniput.c");
    println!("cargo:rerun-if-changed=lib/linux/capture.c");
    println!("cargo:rerun-if-changed=lib/linux/xwindows.c");
    println!("cargo:rerun-if-changed=lib/linux/xwindows.h");
    cc::Build::new()
        .file("lib/linux/uinput.c")
        .file("lib/linux/capture.c")
        .file("lib/linux/xwindows.c")
        .compile("linux");
    println!("cargo:rustc-link-lib=X11");
    println!("cargo:rustc-link-lib=Xext");
}
