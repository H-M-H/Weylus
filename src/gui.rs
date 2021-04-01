use std::cell::RefCell;
use std::iter::Iterator;
use std::net::{IpAddr, SocketAddr};
use std::rc::Rc;

use std::sync::{mpsc, Arc, Mutex};
use tokio::sync::mpsc as mpsc_tokio;
use tracing::{error, info};

use fltk::{
    app::App,
    button::{Button, CheckButton},
    frame::Frame,
    input::{Input, IntInput},
    output::Output,
    prelude::*,
    text::{TextBuffer, TextDisplay},
    window::Window,
};

#[cfg(not(target_os = "windows"))]
use pnet::datalink;

use crate::config::{write_config, Config};
use crate::video::EncoderOptions;
use crate::web::{Gui2WebMessage, Web2GuiMessage};
use crate::websocket::{Gui2WsMessage, Ws2GuiMessage, WsConfig};

pub fn run(config: &Config, log_receiver: mpsc::Receiver<String>) {
    // this makes sure XInitThreads is called before any threading is done
    fltk::app::lock().unwrap();
    fltk::app::unlock();

    let width = 200;
    let height = 30;
    let padding = 10;

    let app = App::default().with_scheme(fltk::app::AppScheme::Gtk);
    let mut wind = Window::default()
        .with_size(660, 620)
        .center_screen()
        .with_label(&format!("Weylus - {}", env!("CARGO_PKG_VERSION")));

    let mut input_access_code = Input::default()
        .with_pos(130, 30)
        .with_size(width, height)
        .with_label("Access code");
    input_access_code.set_tooltip(
        "Restrict who can control your computer with an access code. Note that this does NOT do \
        any kind of encryption and it is advised to only run Weylus inside trusted networks! Do \
        NOT reuse any of your passwords! If left blank, no code is required to access Weylus \
        remotely.",
    );
    if let Some(code) = config.access_code.as_ref() {
        input_access_code.set_value(code);
    }

    let input_bind_addr = Input::default()
        .with_size(width, height)
        .below_of(&input_access_code, padding)
        .with_label("Bind Address");
    input_bind_addr.set_value(&config.bind_address.to_string());

    let input_port = IntInput::default()
        .with_size(width, height)
        .below_of(&input_bind_addr, padding)
        .with_label("Port");
    input_port.set_value(&config.web_port.to_string());

    let input_ws_port = IntInput::default()
        .with_size(width, height)
        .below_of(&input_port, padding)
        .with_label("Websocket Port");
    input_ws_port.set_value(&config.websocket_port.to_string());

    let mut check_auto_start = CheckButton::default()
        .with_size(70, height)
        .below_of(&input_ws_port, padding)
        .with_label("Auto Start");
    check_auto_start.set_tooltip("Start Weylus server immediately on program start.");
    check_auto_start.set_checked(config.auto_start);

    #[cfg(target_os = "linux")]
    let mut check_wayland = CheckButton::default()
        .with_size(70, height)
        .right_of(&check_auto_start, 3 * padding)
        .with_label("Wayland/\nPipeWire\nSupport");
    #[cfg(target_os = "linux")]
    {
        check_wayland.set_tooltip(
            "EXPERIMENTAL! This may crash your desktop! Enables screen \
        capturing for Wayland using PipeWire and GStreamer.",
        );
        check_wayland.set_checked(config.wayland_support);
    }

    let mut label_hw_accel = Frame::default()
        .with_size(width, height)
        .below_of(&check_auto_start, padding)
        .with_label("Try Hardware acceleration");
    label_hw_accel.set_tooltip(
        "On many systems video encoding can be done with hardware \
        acceleration. By default this is disabled as the quality and stability of video encoding \
        varies greatly among hardware and drivers. Currently this is only supported on Linux.",
    );

    let mut check_native_hw_accel = CheckButton::default()
        .with_size(70, height)
        .below_of(&label_hw_accel, 0);

    #[cfg(target_os = "linux")]
    {
        check_native_hw_accel.set_label("VAAPI");
        check_native_hw_accel
            .set_tooltip("Try to use hardware acceleration through the Video Acceleration API.");
        check_native_hw_accel.set_checked(config.try_vaapi);
    }

    #[cfg(target_os = "macos")]
    {
        check_native_hw_accel.set_label("VideoToolbox");
        check_native_hw_accel
            .set_tooltip("Try to use hardware acceleration through the VideoToolbox API.");
        check_native_hw_accel.set_checked(config.try_videotoolbox);
    }

    #[cfg(target_os = "windows")]
    {
        check_native_hw_accel.set_label("MediaFoundation");
        check_native_hw_accel
            .set_tooltip("Try to use hardware acceleration through the MediaFoundation API.");
        check_native_hw_accel.set_checked(config.try_mediafoundation);
    }

    let mut check_nvenc = CheckButton::default()
        .with_size(70, height)
        .right_of(&check_native_hw_accel, padding)
        .with_label("NVENC");
    check_nvenc.set_tooltip("Try to use Nvidia's NVENC to encode the video via GPU.");

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    check_nvenc.set_checked(config.try_nvenc);

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        check_nvenc.deactivate();
        check_nvenc.hide();
    }

    let but_toggle = Button::default()
        .with_size(width, height)
        .below_of(&check_native_hw_accel, 2 * padding)
        .with_label("Start");

    let mut output_server_addr = Output::default()
        .with_size(500, height)
        .below_of(&but_toggle, 3 * padding)
        .with_label("Connect your\ntablet to:");
    output_server_addr.hide();

    let output_buf = TextBuffer::default();
    let mut output = TextDisplay::default().with_size(600, 6 * height).with_pos(
        30,
        output_server_addr.y() + output_server_addr.height() + padding,
    );
    output.set_buffer(output_buf);
    let output_buf = output.buffer().unwrap();

    let mut qr_frame = Frame::default()
        .with_size(240, 240)
        .right_of(&input_access_code, padding);

    qr_frame.hide();

    wind.make_resizable(true);
    wind.end();
    wind.show();

    let but_toggle_ref = Rc::new(RefCell::new(but_toggle));
    let but_toggle_ref2 = but_toggle_ref.clone();
    let output_server_addr = Arc::new(Mutex::new(output_server_addr));
    let output_buf = Arc::new(Mutex::new(output_buf));

    let (sender_ws2gui, receiver_ws2gui) = mpsc::channel();
    let (sender_web2gui, receiver_web2gui) = mpsc::channel();

    std::thread::spawn(move || {
        while let Ok(log_message) = log_receiver.recv() {
            let mut output_buf = output_buf.lock().unwrap();
            output_buf.append(&log_message);
        }
    });

    {
        let output_server_addr = output_server_addr.clone();
        std::thread::spawn(move || {
            while let Ok(message) = receiver_web2gui.recv() {
                match message {
                    Web2GuiMessage::Shutdown => {
                        let mut output_server_addr = output_server_addr.lock().unwrap();
                        output_server_addr.hide();
                    }
                }
            }
        });
    }

    {
        std::thread::spawn(move || {
            while let Ok(message) = receiver_ws2gui.recv() {
                match message {
                    Ws2GuiMessage::UInputInaccessible => {
                        let w = 500;
                        let h = 300;
                        let mut pop_up = Window::default()
                            .with_size(w, h)
                            .center_screen()
                            .with_label("Weylus - UInput inaccessible!");

                        let buf = TextBuffer::default();
                        let mut pop_up_text = TextDisplay::default().with_size(w, h);
                        pop_up_text.set_buffer(buf);
                        pop_up_text.wrap_mode(fltk::text::WrapMode::AtBounds, 5);
                        let mut buf = pop_up_text.buffer().unwrap();
                        buf.set_text(std::include_str!("strings/uinput_error.txt"));

                        pop_up.end();
                        pop_up.make_modal(true);
                        pop_up.show();
                    }
                }
            }
        });
    }

    let mut sender_gui2ws: Option<mpsc::Sender<Gui2WsMessage>> = None;
    let mut sender_gui2web: Option<mpsc_tokio::Sender<Gui2WebMessage>> = None;

    let mut is_server_running = false;

    let mut toggle_server = move || {
        if let Err(err) = || -> Result<(), Box<dyn std::error::Error>> {
            let but_toggle_ref = but_toggle_ref.clone();
            let mut but = but_toggle_ref.try_borrow_mut()?;

            if !is_server_running {
                let access_code_string = input_access_code.value();
                let access_code = match access_code_string.as_str() {
                    "" => None,
                    code => Some(code),
                };
                let bind_addr: IpAddr = input_bind_addr.value().parse()?;
                let web_port: u16 = input_port.value().parse()?;
                let ws_port: u16 = input_ws_port.value().parse()?;

                let (sender_gui2ws_tmp, receiver_gui2ws) = mpsc::channel();
                sender_gui2ws = Some(sender_gui2ws_tmp);
                let encoder_options = EncoderOptions {
                    #[cfg(target_os = "linux")]
                    try_vaapi: check_native_hw_accel.is_checked(),
                    #[cfg(not(target_os = "linux"))]
                    try_vaapi: false,

                    #[cfg(any(target_os = "linux", target_os = "windows"))]
                    try_nvenc: check_nvenc.is_checked(),
                    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
                    try_nvenc: false,

                    #[cfg(target_os = "macos")]
                    try_videotoolbox: check_native_hw_accel.is_checked(),
                    #[cfg(not(target_os = "macos"))]
                    try_videotoolbox: false,

                    #[cfg(target_os = "windows")]
                    try_mediafoundation: check_native_hw_accel.is_checked(),
                    #[cfg(not(target_os = "windows"))]
                    try_mediafoundation: false,
                };
                let ws_config = WsConfig {
                    address: SocketAddr::new(bind_addr, ws_port),
                    access_code: access_code.map(|s| s.into()),
                    encoder_options,
                    #[cfg(target_os = "linux")]
                    wayland_support: check_wayland.is_checked(),
                };
                crate::websocket::run(sender_ws2gui.clone(), receiver_gui2ws, ws_config);

                let (sender_gui2web_tmp, receiver_gui2web) = mpsc_tokio::channel(100);
                sender_gui2web = Some(sender_gui2web_tmp);
                let mut web_sock = SocketAddr::new(bind_addr, web_port);
                crate::web::run(
                    sender_web2gui.clone(),
                    receiver_gui2web,
                    &web_sock,
                    ws_port,
                    access_code,
                );

                #[cfg(not(target_os = "windows"))]
                {
                    if web_sock.ip().is_unspecified() {
                        // try to guess an ip
                        let mut ips = Vec::<IpAddr>::new();
                        for iface in datalink::interfaces()
                            .iter()
                            .filter(|iface| iface.is_up() && !iface.is_loopback())
                        {
                            for ipnetw in &iface.ips {
                                if (ipnetw.is_ipv4() && web_sock.ip().is_ipv4())
                                    || (ipnetw.is_ipv6() && web_sock.ip().is_ipv6())
                                {
                                    // filtering ipv6 unicast requires nightly or more fiddling,
                                    // lets wait for nightlies to stabilize...
                                    ips.push(ipnetw.ip())
                                }
                            }
                        }
                        if !ips.is_empty() {
                            web_sock.set_ip(ips[0]);
                        }
                        if ips.len() > 1 {
                            info!("Found more than one IP address for browsers to connect to,");
                            info!("other urls are:");
                            for ip in &ips[1..] {
                                info!("http://{}", SocketAddr::new(*ip, web_port));
                            }
                        }
                    }
                }
                let mut output_server_addr = output_server_addr.lock()?;

                #[cfg(not(target_os = "windows"))]
                {
                    use image::Luma;
                    use qrcode::QrCode;
                    let addr_string = format!("http://{}", web_sock.to_string());
                    output_server_addr.set_value(&addr_string);
                    let access_code = access_code.map(|s| s.to_string());
                    let mut url_string = addr_string;
                    if let Some(access_code) = &access_code {
                        url_string.push_str("?access_code=");
                        url_string.push_str(
                            &percent_encoding::utf8_percent_encode(
                                &access_code,
                                percent_encoding::NON_ALPHANUMERIC,
                            )
                            .to_string(),
                        );
                    }
                    let code = QrCode::new(&url_string).unwrap();
                    let img_buf = code.render::<Luma<u8>>().build();
                    let image = image::DynamicImage::ImageLuma8(img_buf);
                    let image = image.resize_exact(
                        qr_frame.width() as u32,
                        qr_frame.height() as u32,
                        image::imageops::FilterType::Nearest,
                    );
                    let mut buf = vec![];
                    image
                        .write_to(&mut buf, image::ImageOutputFormat::Png)
                        .unwrap();
                    let png = fltk::image::PngImage::from_data(&buf).unwrap();

                    qr_frame.set_image(Some(png));
                    qr_frame.show();
                }
                #[cfg(target_os = "windows")]
                {
                    if web_sock.ip().is_unspecified() {
                        output_server_addr.set_value("http://<your ip address>");
                    } else {
                        output_server_addr.set_value(&format!("http://{}", web_sock.to_string()));
                    }
                }
                output_server_addr.show();
                but.set_label("Stop");
                let config = Config {
                    access_code: access_code.map(|s| s.to_string()),
                    web_port,
                    websocket_port: ws_port,
                    bind_address: bind_addr,
                    #[cfg(target_os = "linux")]
                    try_vaapi: check_native_hw_accel.is_checked(),
                    #[cfg(any(target_os = "linux", target_os = "windows"))]
                    try_nvenc: check_nvenc.is_checked(),
                    auto_start: check_auto_start.is_checked(),
                    #[cfg(target_os = "linux")]
                    wayland_support: check_wayland.is_checked(),
                };
                write_config(&config);
            } else {
                if let Some(mut sender_gui2web) = sender_gui2web.clone() {
                    sender_gui2web.try_send(Gui2WebMessage::Shutdown)?;
                }

                if let Some(sender_gui2ws) = sender_gui2ws.clone() {
                    sender_gui2ws.send(Gui2WsMessage::Shutdown)?;
                }
                but.set_label("Start");
                qr_frame.hide();
            }
            is_server_running = !is_server_running;
            Ok(())
        }() {
            error!("{}", err);
        };
    };

    if config.auto_start {
        toggle_server();
    }

    but_toggle_ref2.borrow_mut().set_callback(toggle_server);

    #[cfg(target_os = "linux")]
    if let Err(err) = gstreamer::init() {
        error!(
            "Failed to initialize gstreamer, screen capturing will most likely not work \
            on Wayland: {}",
            err
        );
    }

    app.run().expect("Failed to run Gui!");
}
