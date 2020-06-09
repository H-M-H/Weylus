use std::cell::RefCell;
use std::iter::Iterator;
use std::net::{IpAddr, SocketAddr};
use std::rc::Rc;
use std::time::Duration;

use std::sync::{mpsc, Arc, Mutex};
use tokio::sync::mpsc as mpsc_tokio;
use tracing::{info, error};

use fltk::{
    app::App,
    button::{Button, CheckButton},
    enums::Shortcut,
    frame::Frame,
    input::{Input, IntInput},
    menu::{Choice, MenuFlag},
    output::Output,
    prelude::*,
    text::{TextBuffer, TextDisplay},
    window::Window,
};

#[cfg(not(target_os = "windows"))]
use pnet::datalink;

use crate::web::{Gui2WebMessage, Web2GuiMessage};
use crate::websocket::Gui2WsMessage;

#[cfg(target_os = "linux")]
use crate::x11helper::X11Context;

pub fn run(log_receiver: mpsc::Receiver<String>) {
    fltk::app::lock().unwrap();
    fltk::app::unlock();
    let width = 200;
    let height = 30;
    let padding = 10;

    let app = App::default();
    let mut wind = Window::default()
        .with_size(660, 600)
        .center_screen()
        .with_label(&format!("Weylus - {}", env!("CARGO_PKG_VERSION")));

    let input_password = Input::default()
        .with_pos(200, 30)
        .with_size(width, height)
        .with_label("Password");

    let input_bind_addr = Input::default()
        .with_size(width, height)
        .below_of(&input_password, padding)
        .with_label("Bind Address");
    input_bind_addr.set_value("0.0.0.0");

    let input_port = IntInput::default()
        .with_size(width, height)
        .below_of(&input_bind_addr, padding)
        .with_label("Port");
    input_port.set_value("1701");

    let input_ws_pointer_port = IntInput::default()
        .with_size(width, height)
        .below_of(&input_port, padding)
        .with_label("Websocket Pointer Port");
    input_ws_pointer_port.set_value("9001");

    let input_ws_video_port = IntInput::default()
        .with_size(width, height)
        .below_of(&input_ws_pointer_port, padding)
        .with_label("Websocket Video Port");
    input_ws_video_port.set_value("9002");

    let input_limit_screen_updates = IntInput::default()
        .with_size(width, height)
        .below_of(&input_ws_video_port, padding)
        .with_label("Limit screen updates\n(milliseconds)");
    input_limit_screen_updates.set_value("0");

    let but_toggle = Button::default()
        .with_size(width, height)
        .below_of(&input_limit_screen_updates, 3 * padding)
        .with_label("Start");

    let mut check_stylus = CheckButton::default()
        .with_pos(430, 30 + (padding + height))
        .with_size(width, 2 * height);
    #[cfg(target_os = "linux")]
    {
        check_stylus.set_label("Stylus/Pen support\nRequires access to\n/dev/uinput");
        check_stylus.set_checked(true);
    }
    #[cfg(not(target_os = "linux"))]
    {
        check_stylus.set_label("Stylus/Pen support\n(works only on\nLinux)");
        check_stylus.deactivate();
    }

    let mut check_faster_screencapture = CheckButton::default()
        .with_size(width, 2 * height)
        .below_of(&check_stylus, 2 * padding);
    #[cfg(target_os = "linux")]
    {
        check_faster_screencapture.set_checked(true);
        check_faster_screencapture.set_label("Faster screencapture");
    }
    #[cfg(not(target_os = "linux"))]
    {
        check_faster_screencapture.set_label("Faster screencapture\n(works only on\nLinux)");
        check_faster_screencapture.deactivate();
    }

    #[cfg(target_os = "linux")]
    let label_window_choice = Frame::default()
        .with_size(width, height)
        .below_of(&check_faster_screencapture, padding)
        .with_label("Window to capture:");

    #[cfg(not(target_os = "linux"))]
    let label_window_choice = Frame::default()
        .with_size(width, height)
        .below_of(&check_faster_screencapture, padding)
        .with_label("Capturing windows is\nonly supported on Linux!");

    #[allow(unused_mut)]
    let mut choice_window = Choice::default()
        .with_size(width, height)
        .below_of(&label_window_choice, 0);
    #[cfg(not(target_os = "linux"))]
    choice_window.deactivate();

    let mut but_update_windows = Button::default()
        .with_size(width, height)
        .below_of(&choice_window, padding)
        .with_label("Refresh");
    #[cfg(not(target_os = "linux"))]
    but_update_windows.deactivate();

    let mut output_buf = TextBuffer::default();
    let output = TextDisplay::default(&mut output_buf)
        .with_size(600, 6 * height)
        .with_pos(30, 600 - 30 - 6 * height);

    let mut output_server_addr = Output::default()
        .with_size(500, height)
        .with_pos(130, 600 - 30 - 7 * height - 3 * padding)
        .with_label("Connect your\ntablet to:");
    output_server_addr.hide();

    wind.make_resizable(true);
    wind.end();
    wind.show();

    let but_toggle_ref = Rc::new(RefCell::new(but_toggle));
    let choice_window_ref = Rc::new(RefCell::new(choice_window));
    let check_faster_screencapture_ref = Rc::new(RefCell::new(check_faster_screencapture));
    let output_server_addr = Arc::new(Mutex::new(output_server_addr));
    let output = Arc::new(Mutex::new(output));

    let (sender_ws2gui, _receiver_ws2gui) = mpsc::channel();
    let (sender_web2gui, receiver_web2gui) = mpsc::channel();

    {
        let output = output.clone();
        std::thread::spawn(move || {
            while let Ok(log_message) = log_receiver.recv() {
                let output = output.lock().unwrap();
                output.insert(&log_message);
            }
        });
    }

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

    #[cfg(target_os = "linux")]
    let mut x11_context = X11Context::new().unwrap();
    #[cfg(target_os = "linux")]
    let capture_window = Rc::new(RefCell::new(x11_context.root_window()));

    #[cfg(target_os = "linux")]
    {
        let capture_window = capture_window.clone();

        {
            let choice_window_ref = choice_window_ref.clone();
            but_update_windows.set_callback(Box::new(move || {
                let mut choice_window = choice_window_ref.borrow_mut();
                choice_window.clear();
                {
                    let root_window = x11_context.root_window();
                    let capture_window = capture_window.clone();
                    choice_window.add(
                        &root_window.name(),
                        Shortcut::None,
                        MenuFlag::Normal,
                        Box::new(move || {
                            let capture_window = capture_window.clone();
                            capture_window.replace(root_window);
                        }),
                    );
                }
                for window in x11_context.windows().unwrap() {
                    let capture_window = capture_window.clone();
                    let chars = window
                        .name()
                        .replace("\\", "\\\\")
                        .replace("/", "\\/")
                        .replace("_", "\\_")
                        .replace("&", "\\&");
                    let chars = chars.chars();
                    let mut name = String::new();
                    for (i, c) in chars.enumerate() {
                        if i >= 32 {
                            name.push_str("...");
                            break;
                        }
                        name.push(c);
                    }
                    choice_window.add(
                        &name,
                        Shortcut::None,
                        MenuFlag::Normal,
                        Box::new(move || {
                            capture_window.replace(window);
                        }),
                    );
                }
            }));
        }

        but_update_windows.do_callback();

        let check_faster_screencapture_ref = check_faster_screencapture_ref.clone();

        check_faster_screencapture_ref
            .clone()
            .borrow_mut()
            .set_callback(Box::new(move || {
                let checked = !check_faster_screencapture_ref.borrow().is_checked();
                let mut choice_window = choice_window_ref.borrow_mut();
                if checked {
                    choice_window.deactivate();
                    but_update_windows.deactivate();
                } else {
                    choice_window.activate();
                    but_update_windows.activate();
                }
            }));
    }

    let mut sender_gui2ws: Option<mpsc::Sender<Gui2WsMessage>> = None;
    let mut sender_gui2web: Option<mpsc_tokio::Sender<Gui2WebMessage>> = None;

    let mut is_server_running = false;

    but_toggle_ref
        .clone()
        .borrow_mut()
        .set_callback(Box::new(move || {
            if let Err(err) = || -> Result<(), Box<dyn std::error::Error>> {
                let but_toggle_ref = but_toggle_ref.clone();
                let mut but = but_toggle_ref.try_borrow_mut()?;

                if !is_server_running {
                    let password_string = input_password.value();
                    let password = match password_string.as_str() {
                        "" => None,
                        pw => Some(pw),
                    };
                    let bind_addr: IpAddr = input_bind_addr.value().parse()?;
                    let web_port: u16 = input_port.value().parse()?;
                    let ws_pointer_port: u16 = input_ws_pointer_port.value().parse()?;
                    let ws_video_port: u16 = input_ws_video_port.value().parse()?;
                    let screen_update_interval: u64 = input_limit_screen_updates.value().parse()?;
                    let screen_update_interval = Duration::from_millis(screen_update_interval);

                    let (sender_gui2ws_tmp, receiver_gui2ws) = mpsc::channel();
                    sender_gui2ws = Some(sender_gui2ws_tmp);
                    #[cfg(target_os = "linux")]
                    crate::websocket::run(
                        sender_ws2gui.clone(),
                        receiver_gui2ws,
                        SocketAddr::new(bind_addr, ws_pointer_port),
                        SocketAddr::new(bind_addr, ws_video_port),
                        password,
                        screen_update_interval,
                        check_stylus.is_checked(),
                        check_faster_screencapture_ref.borrow().is_checked(),
                        capture_window.clone().borrow().clone(),
                    );
                    #[cfg(not(target_os = "linux"))]
                    crate::websocket::run(
                        sender_ws2gui.clone(),
                        receiver_gui2ws,
                        SocketAddr::new(bind_addr, ws_pointer_port),
                        SocketAddr::new(bind_addr, ws_video_port),
                        password,
                        screen_update_interval,
                    );

                    let (sender_gui2web_tmp, receiver_gui2web) = mpsc_tokio::channel(100);
                    sender_gui2web = Some(sender_gui2web_tmp);
                    let mut web_sock = SocketAddr::new(bind_addr, web_port);
                    crate::web::run(
                        sender_web2gui.clone(),
                        receiver_gui2web,
                        &web_sock,
                        ws_pointer_port,
                        ws_video_port,
                        password,
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
                            if ips.len() > 0 {
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
                    output_server_addr.set_value(&format!("http://{}", web_sock.to_string()));
                    #[cfg(target_os = "windows")]
                    {
                        if web_sock.ip().is_unspecified() {
                            output_server_addr.set_value("http://<your ip address>");
                        } else {
                            output_server_addr
                                .set_value(&format!("http://{}", web_sock.to_string()));
                        }
                    }
                    output_server_addr.show();
                    but.set_label("Stop");
                } else {
                    if let Some(mut sender_gui2web) = sender_gui2web.clone() {
                        sender_gui2web.try_send(Gui2WebMessage::Shutdown)?;
                    }

                    if let Some(sender_gui2ws) = sender_gui2ws.clone() {
                        sender_gui2ws.send(Gui2WsMessage::Shutdown)?;
                    }
                    but.set_label("Start");
                }
                is_server_running = !is_server_running;
                Ok(())
            }() {
                error!("{}", err);
            };
        }));

    app.run().expect("Failed to run Gui!");
}
