#![cfg_attr(feature = "bench", feature(test))]
#[cfg(feature = "bench")]
extern crate test;

#[macro_use]
extern crate bitflags;

use clap::CommandFactory;
use clap_complete::generate;
#[cfg(unix)]
use signal_hook::iterator::Signals;
#[cfg(unix)]
use signal_hook::{consts::TERM_SIGNALS, low_level::signal_name};
use tracing::{error, info, warn};

use std::sync::mpsc;

use config::{get_config, Config};

mod capturable;
mod cerror;
mod config;
mod gui;
mod input;
mod log;
mod protocol;
mod video;
mod web;
mod websocket;
mod weylus;

/// Frame pacing on Windows. The video loop waits with recv_timeout, and Windows rounds every wait
/// up to the default 15.6 ms timer tick, so a target frame rate that needs finer pacing gets far
/// fewer frames than requested (H-M-H/Weylus#564 measured 23 -> 52 fps after setting a 1 ms period).
/// Windows 11 can also put a windowless background process on EcoQoS (efficiency cores, timer
/// requests ignored); opt out of both, as a video server should. Failures are logged, never fatal.
#[cfg(target_os = "windows")]
fn set_windows_timing() {
    use std::ffi::c_void;
    #[repr(C)]
    struct PowerThrottlingState {
        version: u32,
        control_mask: u32,
        state_mask: u32,
    }
    #[link(name = "winmm")]
    extern "system" {
        fn timeBeginPeriod(period_ms: u32) -> u32;
    }
    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn SetProcessInformation(
            process: *mut c_void,
            class: i32,
            info: *const c_void,
            size: u32,
        ) -> i32;
        fn GetLastError() -> u32;
    }
    const PROCESS_POWER_THROTTLING: i32 = 4; // PROCESS_INFORMATION_CLASS::ProcessPowerThrottling
    const EXECUTION_SPEED: u32 = 0x1;
    const IGNORE_TIMER_RESOLUTION: u32 = 0x4; // Windows 11
    unsafe {
        let timer = timeBeginPeriod(1);
        let set = |mask: u32| {
            let state = PowerThrottlingState {
                version: 1,
                control_mask: mask,
                state_mask: 0,
            };
            SetProcessInformation(
                GetCurrentProcess(),
                PROCESS_POWER_THROTTLING,
                &state as *const _ as *const c_void,
                std::mem::size_of::<PowerThrottlingState>() as u32,
            ) != 0
        };
        let mut mask = EXECUTION_SPEED | IGNORE_TIMER_RESOLUTION;
        let mut ok = set(mask);
        let err = if ok { 0 } else { GetLastError() };
        if !ok {
            // Windows 10 does not know IGNORE_TIMER_RESOLUTION.
            mask = EXECUTION_SPEED;
            ok = set(mask);
        }
        if timer == 0 && ok {
            info!("Windows timing: 1 ms timer period, power throttling off (mask={mask:#x}).");
        } else {
            warn!(
                "Windows timing: timeBeginPeriod={timer} (0 = ok), power throttling off={ok} \
                 (mask={mask:#x}, first error={err}).",
            );
        }
    }
}

fn main() {
    let (sender, receiver) = mpsc::sync_channel::<String>(100);

    log::setup_logging(sender);

    #[cfg(target_os = "windows")]
    set_windows_timing();

    let conf = get_config();

    if let Some(shell) = conf.completions {
        generate(
            shell,
            &mut Config::command(),
            "weylus",
            &mut std::io::stdout(),
        );
        return;
    }

    if conf.print_index_html {
        print!("{}", web::INDEX_HTML);
        return;
    }
    if conf.print_access_html {
        print!("{}", web::ACCESS_HTML);
        return;
    }
    if conf.print_style_css {
        print!("{}", web::STYLE_CSS);
        return;
    }
    if conf.print_lib_js {
        print!("{}", web::LIB_JS);
        return;
    }

    #[cfg(target_os = "linux")]
    {
        // make sure XInitThreads is called before any threading is done
        crate::capturable::x11::x11_init();

        if let Err(err) = gstreamer::init() {
            error!(
                "Failed to initialize gstreamer, screen capturing will most likely not work \
                 on Wayland: {}",
                err
            );
        }
    }

    if conf.no_gui {
        let mut weylus = crate::weylus::Weylus::new();
        weylus.start(&conf, |msg| match msg {
            web::Web2UiMessage::UInputInaccessible => {
                warn!(std::include_str!("strings/uinput_error.txt"))
            }
        });
        #[cfg(unix)]
        {
            let mut signals = Signals::new(TERM_SIGNALS).unwrap();
            for sig in signals.forever() {
                info!(
                    "Shutting down after receiving signal {signame} ({sig})...",
                    signame = signal_name(sig).unwrap_or("UNKNOWN SIGNAL")
                );
                std::thread::spawn(move || {
                    for sig in signals.forever() {
                        warn!(
                            "Received second signal {signame} ({sig}) while shutting down \
                            gracefully, proceeding with forceful shutdown...",
                            signame = signal_name(sig).unwrap_or("UNKNOWN SIGNAL")
                        );
                        std::process::exit(1);
                    }
                });
                weylus.stop();
                break;
            }
        }
        #[cfg(not(unix))]
        {
            loop {
                std::thread::park();
            }
        }
    } else {
        gui::run(&conf, receiver);
    }
}

#[cfg(feature = "bench")]
#[cfg(test)]
mod tests {
    use super::*;
    use capturable::{Capturable, Recorder};
    use test::Bencher;

    #[cfg(target_os = "linux")]
    #[bench]
    fn bench_capture_x11(b: &mut Bencher) {
        let mut x11ctx = capturable::x11::X11Context::new().unwrap();
        let root = x11ctx.capturables().unwrap().remove(0);
        let mut r = root.recorder(false).unwrap();
        b.iter(|| {
            r.capture().unwrap();
        });
    }

    #[cfg(target_os = "linux")]
    #[bench]
    fn bench_video_x11(b: &mut Bencher) {
        let mut x11ctx = capturable::x11::X11Context::new().unwrap();
        let root = x11ctx.capturables().unwrap().remove(0);
        let mut r = root.recorder(false).unwrap();
        let (width, height) = r.capture().unwrap().size();

        let opts = video::EncoderOptions {
            try_vaapi: true,
            try_nvenc: true,
            try_videotoolbox: false,
            try_mediafoundation: false,
        };
        let mut encoder =
            video::VideoEncoder::new(width, height, width, height, |_| {}, opts).unwrap();
        b.iter(|| encoder.encode(r.capture().unwrap()));
    }

    #[cfg(target_os = "linux")]
    #[bench]
    fn bench_capture_wayland(b: &mut Bencher) {
        gstreamer::init().unwrap();
        let root = capturable::pipewire::get_capturables(false)
            .unwrap()
            .remove(0);
        let mut r = root.recorder(false).unwrap();
        let _ = r.capture();
        b.iter(|| {
            r.capture().unwrap();
        });
    }

    #[cfg(target_os = "linux")]
    #[bench]
    fn bench_video_wayland(b: &mut Bencher) {
        gstreamer::init().unwrap();
        let root = capturable::pipewire::get_capturables(false)
            .unwrap()
            .remove(0);
        let mut r = root.recorder(false).unwrap();
        let (width, height) = r.capture().unwrap().size();

        let opts = video::EncoderOptions {
            try_vaapi: true,
            try_nvenc: true,
            try_videotoolbox: false,
            try_mediafoundation: false,
        };
        let mut encoder =
            video::VideoEncoder::new(width, height, width, height, |_| {}, opts).unwrap();
        b.iter(|| encoder.encode(r.capture().unwrap()));
    }

    #[cfg(target_os = "linux")]
    #[bench]
    fn bench_video_vaapi(b: &mut Bencher) {
        const WIDTH: usize = 1920;
        const HEIGHT: usize = 1080;
        const N: usize = 60;
        let mut bufs = vec![vec![0u8; SIZE]; N];
        for i in 0..N {
            for j in 0..SIZE {
                bufs[i][j] = ((i * SIZE + j) % 256) as u8;
            }
        }

        let opts = video::EncoderOptions {
            try_vaapi: true,
            try_nvenc: false,
            try_videotoolbox: false,
            try_mediafoundation: false,
        };
        let mut encoder =
            video::VideoEncoder::new(WIDTH, HEIGHT, WIDTH, HEIGHT, |_| {}, opts).unwrap();
        const SIZE: usize = WIDTH * HEIGHT * 4;
        let mut i = 0;
        b.iter(|| {
            encoder.encode(video::PixelProvider::BGR0(WIDTH, HEIGHT, &bufs[i % N]));
            i += 1;
        });
    }

    #[cfg(target_os = "linux")]
    #[bench]
    fn bench_video_x264(b: &mut Bencher) {
        const WIDTH: usize = 1920;
        const HEIGHT: usize = 1080;
        const N: usize = 60;
        let mut bufs = vec![vec![0u8; SIZE]; N];
        for i in 0..N {
            for j in 0..SIZE {
                bufs[i][j] = ((i * SIZE + j) % 256) as u8;
            }
        }

        let opts = video::EncoderOptions {
            try_vaapi: false,
            try_nvenc: false,
            try_videotoolbox: false,
            try_mediafoundation: false,
        };
        let mut encoder =
            video::VideoEncoder::new(WIDTH, HEIGHT, WIDTH, HEIGHT, |_| {}, opts).unwrap();
        const SIZE: usize = WIDTH * HEIGHT * 4;
        let mut i = 0;
        b.iter(|| {
            encoder.encode(video::PixelProvider::BGR0(WIDTH, HEIGHT, &bufs[i % N]));
            i += 1;
        });
    }

    #[cfg(target_os = "linux")]
    #[bench]
    fn bench_video_nvenc(b: &mut Bencher) {
        const WIDTH: usize = 1920;
        const HEIGHT: usize = 1080;
        const N: usize = 60;
        let mut bufs = vec![vec![0u8; SIZE]; N];
        for i in 0..N {
            for j in 0..SIZE {
                bufs[i][j] = ((i * SIZE + j) % 256) as u8;
            }
        }

        let opts = video::EncoderOptions {
            try_vaapi: false,
            try_nvenc: true,
            try_videotoolbox: false,
            try_mediafoundation: false,
        };
        let mut encoder =
            video::VideoEncoder::new(WIDTH, HEIGHT, WIDTH, HEIGHT, |_| {}, opts).unwrap();
        const SIZE: usize = WIDTH * HEIGHT * 4;
        let mut i = 0;
        b.iter(|| {
            encoder.encode(video::PixelProvider::BGR0(WIDTH, HEIGHT, &bufs[i % N]));
            i += 1;
        });
    }
}
