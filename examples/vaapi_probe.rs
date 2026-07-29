// Minimal VAAPI encoder probe — no portal, no browser, no capture.
// Just creates a VideoEncoder with try_vaapi=true, feeds synthetic
// BGR0 frames, and lets the ffmpeg log tell us which encoder was selected.
//
// Run:
//   cargo run --example vaapi_probe 2>&1 | grep -E '(Video:|ERROR|WARN|Scale filter|VAAPI|hwupload|vaapi)'

// Pull in the needed modules the same way pw_probe.rs does.
#[path = "../src/cerror.rs"]
mod cerror;
#[path = "../src/log.rs"]
mod log;
#[path = "../src/video.rs"]
mod video;

use std::sync::mpsc;

fn main() {
    let (tx, _rx) = mpsc::sync_channel::<String>(32);
    log::setup_logging(tx);

    const WIDTH: usize = 1920;
    const HEIGHT: usize = 1080;

    let mut buf = vec![0u8; WIDTH * HEIGHT * 4];
    for j in 0..(WIDTH * HEIGHT) {
        let off = j * 4;
        buf[off] = ((j * 3) % 256) as u8;
        buf[off + 1] = ((j * 5) % 256) as u8;
        buf[off + 2] = ((j * 7) % 256) as u8;
        buf[off + 3] = 0;
    }

    let opts = video::EncoderOptions {
        try_vaapi: true,
        try_nvenc: false,
        try_videotoolbox: false,
        try_mediafoundation: false,
    };

    let mut encoder = match video::VideoEncoder::new(WIDTH, HEIGHT, WIDTH, HEIGHT, |_| {}, opts) {
        Ok(enc) => enc,
        Err(e) => {
            eprintln!("FAILED to create VideoEncoder: {e}");
            std::process::exit(1);
        }
    };

    // Encode a few frames to exercise the pipeline.
    for _ in 0..5 {
        encoder.encode(video::PixelProvider::BGR0(WIDTH, HEIGHT, &buf));
    }

    eprintln!("\n--- vaapi_probe: done ---");
}
