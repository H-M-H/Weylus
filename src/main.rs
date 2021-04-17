#![cfg_attr(feature = "bench", feature(test))]
#[cfg(feature = "bench")]
extern crate test;

#[macro_use]
extern crate bitflags;

use std::sync::mpsc;

use config::get_config;

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

fn main() {
    let (sender, receiver) = mpsc::sync_channel::<String>(100);

    log::setup_logging(sender);

    let conf = get_config();
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
    gui::run(&conf, receiver);
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
        let root = capturable::pipewire::get_capturables(false).unwrap().remove(0);
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
        let root = capturable::pipewire::get_capturables(false).unwrap().remove(0);
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
