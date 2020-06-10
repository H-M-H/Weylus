#![cfg_attr(feature = "bench", feature(test))]
#[cfg(feature = "bench")]
extern crate test;

#[macro_use]
extern crate bitflags;

use std::io::Write;
use std::sync::mpsc;
use tracing_subscriber;
use tracing_subscriber::layer::SubscriberExt;

mod cerror;
mod gui;
mod input;
mod protocol;
mod screen_capture;
mod stream_handler;
mod video;
mod web;
mod websocket;
#[cfg(target_os = "linux")]
mod x11helper;

struct GuiTracingWriter {
    gui_sender: mpsc::SyncSender<String>,
}

impl Write for GuiTracingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.gui_sender
            .try_send(String::from_utf8_lossy(buf).trim_start().into())
            .ok();
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct GuiTracingWriterFactory {
    sender: mpsc::SyncSender<String>,
}

impl tracing_subscriber::fmt::MakeWriter for GuiTracingWriterFactory {
    type Writer = GuiTracingWriter;
    fn make_writer(&self) -> Self::Writer {
        Self::Writer {
            gui_sender: self.sender.clone(),
        }
    }
}

fn main() {
    let (sender, receiver) = mpsc::sync_channel::<String>(100);
    #[cfg(debug_assertions)]
    let mut level = tracing::Level::TRACE;

    #[cfg(not(debug_assertions))]
    let mut level = tracing::Level::INFO;

    if let Ok(var) = std::env::var("WEYLUS_LOG_LEVEL") {
        let l: Result<tracing::Level, _> = var.parse();
        if let Ok(l) = l {
            level = l;
        }
    }

    let logger = tracing_subscriber::fmt()
        .with_max_level(level)
        .finish()
        .with(
            tracing_subscriber::fmt::Layer::default()
                .with_ansi(false)
                .without_time()
                .with_target(false)
                .compact()
                .with_writer(GuiTracingWriterFactory { sender }),
        );
    tracing::subscriber::set_global_default(logger).expect("Failed to setup logger!");
    gui::run(receiver);
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
        let root = x11ctx.root_window();
        let mut sc = screen_capture::linux::ScreenCaptureX11::new(root).unwrap();
        b.iter(|| sc.capture());
    }

    #[cfg(target_os = "linux")]
    #[bench]
    fn bench_video_x11(b: &mut Bencher) {
        let mut x11ctx = x11helper::X11Context::new().unwrap();
        let root = x11ctx.root_window();
        use screen_capture::ScreenCapture;
        let mut sc = screen_capture::linux::ScreenCaptureX11::new(root).unwrap();
        sc.capture();
        let (width, height) = sc.size();

        let mut encoder = video::VideoEncoder::new(width, height, |_| {}).unwrap();
        b.iter(|| {
            sc.capture();
            encoder.encode(|y, u, v, y_linesize, u_linesize, v_linesize| {
                sc.fill_yuv(y, u, v, y_linesize, u_linesize, v_linesize)
            })
        });
    }
}
