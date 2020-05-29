#[macro_use]
extern crate bitflags;

#[cfg(target_os = "linux")]
#[macro_use]
extern crate c_helper;

use tracing::Level;
use tracing_subscriber;

mod cerror;
mod gui;
mod input;
mod protocol;
mod screen_capture;
mod stream_handler;
mod web;
mod websocket;
#[cfg(target_os = "linux")]
mod x11helper;
mod video;

fn main() {
    #[cfg(debug_assertions)]
    let _subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();
    #[cfg(not(debug_assertions))]
    let _subscriber = tracing_subscriber::fmt().with_max_level(Level::WARN).init();
    gui::run();
}
