#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate bitflags;

#[macro_use]
use rocket_contrib::serve::StaticFiles;

mod cerror;
mod input;
mod protocol;
mod screen_capture;
mod stream_handler;
mod websocket;

fn benchmarks() {
    let mut screen_capture = screen_capture::ScreenCapture::new().unwrap();
    let mut c = 0;
    screen_capture.capture();
    let mut base64_buf = Vec::<u8>::new();
    let t0 = std::time::Instant::now();
    while t0 + std::time::Duration::from_secs(10) > std::time::Instant::now() {
        let img = screen_capture.capture();
        let base64_size = img.len() * 4 / 3 + 4;
        if base64_buf.len() < base64_size {
            base64_buf.resize(base64_size * 2, 0);
        }
        let base64_size = base64::encode_config_slice(&img, base64::STANDARD, &mut base64_buf);
        c += 1;
        println!("{}", c);
    }
}

fn main() {
    websocket::run("0.0.0.0:9001");
    rocket::ignite()
        .mount("/", StaticFiles::from("www"))
        .launch();
}
