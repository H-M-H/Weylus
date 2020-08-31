#![cfg_attr(feature = "bench", feature(test))]
#[cfg(feature = "bench")]
extern crate test;

#[macro_use]
extern crate bitflags;

use std::sync::mpsc;

use config::get_config;

mod config;
mod cerror;
mod log;
mod gui;
mod input;
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

    let mut conf = get_config();
    gui::run(&mut conf, receiver);

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

        let mut encoder = video::VideoEncoder::new(width, height, |_| {}).unwrap();
        b.iter(|| {
            sc.capture().unwrap();
            encoder.encode(sc.pixel_provider())
        });
    }
}
