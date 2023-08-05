use std::cmp::min;
use std::iter::Iterator;
use std::net::{IpAddr, SocketAddr};

use std::sync::{mpsc, Arc, Mutex};
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
use pnet_datalink as datalink;

use crate::config::{write_config, Config};
use crate::websocket::Ws2UiMessage;

pub fn run(config: &Config, log_receiver: mpsc::Receiver<String>) {
    let width = 200;
    let height = 30;
    let padding = 10;

    let app = App::default().with_scheme(fltk::app::AppScheme::Gtk);
    let mut wind = Window::default()
        .with_size(660, 620)
        .center_screen()
        .with_label(&format!("Weylus - {}", env!("CARGO_PKG_VERSION")));
    wind.set_xclass("weylus");

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

    let mut input_bind_addr = Input::default()
        .with_size(width, height)
        .below_of(&input_access_code, padding)
        .with_label("Bind Address");
    input_bind_addr.set_value(&config.bind_address.to_string());

    let mut input_port = IntInput::default()
        .with_size(width, height)
        .below_of(&input_bind_addr, padding)
        .with_label("Port");
    input_port.set_value(&config.web_port.to_string());

    let mut input_ws_port = IntInput::default()
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
        check_native_hw_accel.set_label("Media-\nFoundation");
        check_native_hw_accel
            .set_tooltip("Try to use hardware acceleration through the MediaFoundation API.");
        check_native_hw_accel.set_checked(config.try_mediafoundation);
    }

    let mut check_nvenc = CheckButton::default()
        .with_size(70, height)
        .right_of(&check_native_hw_accel, 2 * padding)
        .with_label("NVENC");
    check_nvenc.set_tooltip("Try to use Nvidia's NVENC to encode the video via GPU.");

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    check_nvenc.set_checked(config.try_nvenc);

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        check_nvenc.deactivate();
        check_nvenc.hide();
    }

    let mut but_toggle = Button::default()
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

    let output_buf = Arc::new(Mutex::new(output_buf));

    std::thread::spawn(move || {
        while let Ok(log_message) = log_receiver.recv() {
            let mut output_buf = output_buf.lock().unwrap();
            output_buf.append(&log_message);
        }
    });

    let mut weylus = crate::weylus::Weylus::new();
    let mut is_server_running = false;
    let auto_start = config.auto_start;
    let mut config = config.clone();

    let mut toggle_server = move |but: &mut Button| {
        if let Err(err) = || -> Result<(), Box<dyn std::error::Error>> {
            if !is_server_running {
                {
                    let access_code_string = input_access_code.value();
                    let access_code = match access_code_string.as_str() {
                        "" => None,
                        code => Some(code),
                    };
                    let bind_addr: IpAddr = input_bind_addr.value().parse()?;
                    let web_port: u16 = input_port.value().parse()?;
                    let ws_port: u16 = input_ws_port.value().parse()?;

                    config.access_code = access_code.map(|s| s.to_string());
                    config.web_port = web_port;
                    config.websocket_port = ws_port;
                    config.bind_address = bind_addr;
                    config.auto_start = check_auto_start.is_checked();
                    #[cfg(target_os = "linux")]
                    {
                        config.try_vaapi = check_native_hw_accel.is_checked();
                        config.wayland_support = check_wayland.is_checked();
                    }
                    #[cfg(any(target_os = "linux", target_os = "windows"))]
                    {
                        config.try_nvenc = check_nvenc.is_checked();
                    }
                    #[cfg(target_os = "macos")]
                    {
                        config.try_videotoolbox = check_native_hw_accel.is_checked();
                    }
                    #[cfg(target_os = "windows")]
                    {
                        config.try_mediafoundation = check_native_hw_accel.is_checked();
                    }
                }
                if !weylus.start(
                    &config,
                    |_| {},
                    |message| match message {
                        Ws2UiMessage::UInputInaccessible => {
                            let w = 500;
                            let h = 300;
                            let mut pop_up = Window::default()
                                .with_size(w, h)
                                .center_screen()
                                .with_label("Weylus - UInput inaccessible!");
                            pop_up.set_xclass("weylus");

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
                        _ => {}
                    },
                ) {
                    return Ok(());
                }
                is_server_running = true;

                write_config(&config);

                let mut web_sock = SocketAddr::new(config.bind_address, config.web_port);

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
                                info!("http://{}", SocketAddr::new(*ip, config.web_port));
                            }
                        }
                    }
                }

                #[cfg(not(target_os = "windows"))]
                {
                    use image::Luma;
                    use qrcode::QrCode;
                    let addr_string = format!("http://{}", web_sock);
                    output_server_addr.set_value(&addr_string);
                    let mut url_string = addr_string;
                    if let Some(access_code) = &config.access_code {
                        url_string.push_str("?access_code=");
                        url_string.push_str(
                            &percent_encoding::utf8_percent_encode(
                                access_code,
                                percent_encoding::NON_ALPHANUMERIC,
                            )
                            .to_string(),
                        );
                    }
                    let code = QrCode::new(&url_string).unwrap();
                    let img_buf = code.render::<Luma<u8>>().build();
                    let image = image::DynamicImage::ImageLuma8(img_buf);
                    let dims = min(qr_frame.width(), qr_frame.height()) as u32;
                    let image = image.resize_exact(dims, dims, image::imageops::FilterType::Nearest);
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
            } else {
                weylus.stop();
                but.set_label("Start");
                output_server_addr.hide();
                qr_frame.hide();
                is_server_running = false;
            }
            Ok(())
        }() {
            error!("{}", err);
        };
    };

    if auto_start {
        toggle_server(&mut but_toggle);
    }

    but_toggle.set_callback(toggle_server);

    app.run().expect("Failed to run Gui!");
}
