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

/// Per-monitor DPI awareness (v2) on Windows. Without it Weylus is DPI-unaware: on a monitor scaled
/// above 100% Windows virtualizes the coordinates it hands to a DPI-unaware process, while the
/// capture and input APIs it uses (DXGI desktop duplication, SetCursorPos) work in physical pixels,
/// so a pointer sent to the captured picture lands in the wrong place on any scaled monitor. Falls
/// back to per-monitor v1 on older Windows, then logs the awareness actually in effect so a mismatch
/// is visible instead of silently wrong input. A failure here is logged and Weylus runs as before.
#[cfg(target_os = "windows")]
fn set_dpi_awareness() {
    use winapi::shared::windef::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use winapi::um::errhandlingapi::GetLastError;
    use winapi::um::winuser::{
        AreDpiAwarenessContextsEqual, GetAwarenessFromDpiAwarenessContext,
        GetThreadDpiAwarenessContext, SetProcessDpiAwarenessContext,
    };
    unsafe {
        let ok = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let err = if ok == 0 { GetLastError() } else { 0 };
        let mut ok_v1 = 0;
        if ok == 0 {
            // Windows before 1703 has no v2; per-monitor v1 still gives physical pixels.
            ok_v1 = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE);
        }
        let ctx = GetThreadDpiAwarenessContext();
        // 0 = unaware, 1 = system aware, 2 = per-monitor aware (DPI_AWARENESS).
        let awareness = GetAwarenessFromDpiAwarenessContext(ctx);
        let is_v2 = AreDpiAwarenessContextsEqual(ctx, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) != 0;
        if awareness == 2 {
            info!(
                "DPI awareness: per-monitor (v2={}, set_v2={}, set_v1={}, error={}): capture and input in physical pixels",
                is_v2, ok != 0, ok_v1 != 0, err
            );
        } else {
            // error 5 = already set before main (a manifest or a compatibility setting on the exe).
            warn!(
                "DPI awareness: NOT per-monitor (awareness={}, set_v2={}, set_v1={}, error={}); on a scaled monitor input may land off target",
                awareness, ok != 0, ok_v1 != 0, err
            );
        }
    }
}

fn main() {
    let (sender, receiver) = mpsc::sync_channel::<String>(100);

    log::setup_logging(sender);

    // Before any capture or input call: on Windows, work in physical pixels on every monitor.
    #[cfg(target_os = "windows")]
    set_dpi_awareness();

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
