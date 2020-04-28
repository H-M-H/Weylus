use std::rc::Rc;

use std::sync::mpsc;
use tokio::sync::mpsc as mpsc_tokio;

use fltk::{
    app::App,
    button::{Button, CheckButton},
    input::{Input, IntInput},
    menu::{Choice, MenuFlag},
    output::MultilineOutput,
    prelude::*,
    window::Window,
};
use pnet::datalink;

use crate::web::Gui2WebMessage;
use crate::websocket::Gui2WsMessage;

pub fn run() {
    let width = 200;
    let height = 30;
    let padding = 10;

    let app = App::default();
    let mut wind = Window::default()
        .with_size(660, 600)
        .center_screen()
        .with_label("WebTablet");

    let mut input_bind_addr = Input::default()
        .with_pos(200, 30)
        .with_size(width, height)
        .with_label("Bind Address");
    input_bind_addr.set_value("0.0.0.0");

    let mut input_port = IntInput::default()
        .with_size(width, height)
        .below_of(&input_bind_addr, padding)
        .with_label("Port");
    input_port.set_value("1701");

    let mut input_ws_pointer_port = IntInput::default()
        .with_size(width, height)
        .below_of(&input_port, padding)
        .with_label("Websocket Pointer Port");
    input_ws_pointer_port.set_value("9001");

    let mut input_ws_video_port = IntInput::default()
        .with_size(width, height)
        .below_of(&input_ws_pointer_port, padding)
        .with_label("Websocket Video Port");
    input_ws_video_port.set_value("9002");

    let mut check_custom_ip_addr = CheckButton::default()
        .with_size(height, height)
        .below_of(&input_ws_video_port, 2 * padding)
        .with_label("Custom IP-Address");

    let mut choice_ip_addr = Choice::default()
        .with_size(width, height)
        .below_of(&check_custom_ip_addr, padding)
        .with_label("IP-Address for Browser\nto connect to");

    for iface in datalink::interfaces() {
        for ip_net in iface.ips {
            choice_ip_addr.add(
                &format!("{} ({})", ip_net.ip(), iface.name),
                Shortcut::None,
                MenuFlag::Normal,
                Box::new(|| {}),
            );
        }
    }

    let mut input_custom_ip_addr = Input::default()
        .with_size(width, height)
        .with_label("IP-Address for Browser\nto connect to")
        .below_of(&check_custom_ip_addr, padding);
    input_custom_ip_addr.deactivate();
    input_custom_ip_addr.hide();

    let mut but_toggle = Button::default()
        .with_size(width, height)
        .below_of(&input_custom_ip_addr, 3 * padding)
        .with_label("Start");

    let mut output = MultilineOutput::default()
        .with_size(600, 7 * height)
        .with_pos(30, 600 - 30 - 7 * height);

    let but_toggle_ref = Rc::<*mut Button>::new(&mut but_toggle);

    let (sender_ws2gui, receiver_ws2gui) = mpsc::channel();
    let (sender_web2gui, receiver_web2gui) = mpsc::channel();
    let mut sender_gui2ws: Option<mpsc::Sender<Gui2WsMessage>> = None;
    let mut sender_gui2web: Option<mpsc_tokio::Sender<Gui2WebMessage>> = None;

    let mut is_server_running = false;

    but_toggle.set_callback(Box::new(move || {
        let but = unsafe { (*but_toggle_ref.clone()).as_mut().unwrap() };

        if !is_server_running {
            input_bind_addr.deactivate();
            let (sender_gui2ws_tmp, receiver_gui2ws) = mpsc::channel();
            sender_gui2ws = Some(sender_gui2ws_tmp);
            crate::websocket::run(
                sender_ws2gui.clone(),
                receiver_gui2ws,
                ([0, 0, 0, 0], 9001).into(),
                ([0, 0, 0, 0], 9002).into(),
                None,
            )
            .unwrap();

            let (sender_gui2web_tmp, receiver_gui2web) = mpsc_tokio::channel(100);
            sender_gui2web = Some(sender_gui2web_tmp);
            crate::web::run(
                sender_web2gui.clone(),
                receiver_gui2web,
                &([0, 0, 0, 0], 1701).into(),
                "dirac.freedesk.lan",
                9001,
                9002,
                None,
            );
            but.set_label("Stop");
        } else {
            if let Some(mut sender_gui2web) = sender_gui2web.clone() {
                sender_gui2web.try_send(Gui2WebMessage::Shutdown);
            }

            if let Some(mut sender_gui2ws) = sender_gui2ws.clone() {
                sender_gui2ws.send(Gui2WsMessage::Shutdown);
            }
            but.set_label("Start");
        }
        is_server_running = !is_server_running;
    }));

    let mut custom_ip = false;
    check_custom_ip_addr.set_callback(Box::new(move || {
        if !custom_ip {
            choice_ip_addr.deactivate();
            choice_ip_addr.hide();
            input_custom_ip_addr.activate();
            input_custom_ip_addr.show();
        } else {
            choice_ip_addr.activate();
            choice_ip_addr.show();
            input_custom_ip_addr.deactivate();
            input_custom_ip_addr.hide();
        }
        custom_ip = !custom_ip;
    }));

    wind.make_resizable(true);
    wind.end();
    wind.show();

    app.run();
}
