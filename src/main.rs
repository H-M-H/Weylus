#![cfg_attr(feature = "bench", feature(test))]
#[cfg(feature = "bench")]
extern crate test;

#[macro_use]
extern crate bitflags;

use std::sync::mpsc;

use config::get_config;

mod cerror;
mod config;
mod gui;
mod input;
mod log;
mod protocol;
mod screen_capture;
mod video;
mod web;
mod websocket;
#[cfg(target_os = "linux")]
mod x11helper;

fn main() {
    let (sender, receiver) = mpsc::sync_channel::<String>(100);

    log::setup_logging(sender);

    let conf = get_config();
    gui::run(&conf, receiver);
}

#[cfg(feature = "bench")]
#[cfg(test)]
mod tests {
    use super::*;
    use screen_capture::ScreenCapture;
    use test::Bencher;

    #[cfg(target_os = "linux")]
    #[bench]
    fn bench_capture_x11(b: &mut Bencher) {
        let mut x11ctx = x11helper::X11Context::new().unwrap();
        let root = x11ctx.capturables().unwrap()[0].clone();
        let mut sc = screen_capture::linux::ScreenCaptureX11::new(root, false).unwrap();
        b.iter(|| sc.capture().unwrap());
    }

    #[cfg(target_os = "linux")]
    #[bench]
    fn bench_video_x11(b: &mut Bencher) {
        let mut x11ctx = x11helper::X11Context::new().unwrap();
        let root = x11ctx.capturables().unwrap()[0].clone();
        use screen_capture::ScreenCapture;
        let mut sc = screen_capture::linux::ScreenCaptureX11::new(root, false).unwrap();
        sc.capture().unwrap();
        let (width, height) = sc.size();

        let mut encoder =
            video::VideoEncoder::new(width, height, width, height, |_| {}, true, true).unwrap();
        b.iter(|| {
            sc.capture().unwrap();
            encoder.encode(sc.pixel_provider())
        });
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
                bufs[i][j] = ((i*SIZE + j)%256) as u8;
            }
        }

        let mut encoder =
            video::VideoEncoder::new(WIDTH, HEIGHT, WIDTH, HEIGHT, |_| {}, true, false).unwrap();
        const SIZE: usize = WIDTH * HEIGHT * 4;
        let mut i = 0;
        b.iter(|| {
            encoder.encode(video::PixelProvider::BGR0(
                &bufs[i%N],
            ));
            i += 1;
        });
    }
}
