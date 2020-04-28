#[macro_use]
extern crate bitflags;

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

fn main() {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();
    gui::run();
}
