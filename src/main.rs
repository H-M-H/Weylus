#[macro_use]
extern crate bitflags;

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

fn main() {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();
    gui::run();
}
