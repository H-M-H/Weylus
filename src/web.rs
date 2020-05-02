use handlebars::Handlebars;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use serde::Serialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::mpsc;
use std::sync::mpsc::SendError;
use std::sync::Arc;
use tokio::sync::mpsc as mpsc_tokio;
use tracing::warn;

#[derive(Serialize)]
struct WebConfig {
    password: Option<String>,
    websocket_pointer_port: u16,
    websocket_video_port: u16,
}

fn response_from_str(s: &str, content_type: &str) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .body(s.to_string().into())
        .unwrap()
}

fn response_not_found() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("content-type", "text/html; charset=utf-8")
        .body("Not found!".into())
        .unwrap()
}

async fn serve<'a>(
    req: Request<Body>,
    context: Arc<Context<'a>>,
    sender: mpsc::Sender<Web2GuiMessage>,
) -> Result<Response<Body>, hyper::Error> {
    let context = &*context;
    let mut authed = false;
    if let Some(password) = &context.password {
        if req.method() == Method::GET && req.uri().path() == "/" {
            use url::form_urlencoded;
            if let Some(query) = req.uri().query() {
                let params = form_urlencoded::parse(query.as_bytes())
                    .into_owned()
                    .collect::<HashMap<String, String>>();
                if let Some(pass) = params.get("password") {
                    if pass == password {
                        authed = true;
                        log_gui_send_error(sender.send(Web2GuiMessage::Info(format!("User authed."))));
                    }
                }
            }
        }
    } else {
        authed = true;
    }
    if req.method() != Method::GET {
        return Ok(response_not_found());
    }
    match req.uri().path() {
        "/" => {
            if !authed {
                return Ok(response_from_str(
                    std::include_str!("../www/static/password.html"),
                    "text/html; charset=utf-8",
                ));
            }
            let config = WebConfig {
                password: context.password.clone(),
                websocket_pointer_port: context.ws_pointer_port,
                websocket_video_port: context.ws_video_port,
            };

            Ok(response_from_str(
                &context.templates.render("index", &config).unwrap(),
                "text/html; charset=utf-8",
            ))
        }
        "/style.css" => Ok(response_from_str(
            std::include_str!("../www/static/style.css"),
            "text/css; charset=utf-8",
        )),
        "/lib.js" => Ok(response_from_str(
            std::include_str!("../www/static/lib.js"),
            "text/javascript; charset=utf-8",
        )),
        _ => Ok(response_not_found()),
    }
}

#[derive(Debug)]
pub enum Gui2WebMessage {
    Shutdown,
}
pub enum Web2GuiMessage {
    Info(String),
    Error(String),
    Shutdown,
}

fn log_gui_send_error<T>(res: Result<(), SendError<T>>) {
    if let Err(err) = res {
        warn!("Webserver: Failed to send message to gui: {}", err);
    }
}

struct Context<'a> {
    bind_addr: SocketAddr,
    ws_pointer_port: u16,
    ws_video_port: u16,
    password: Option<String>,
    templates: Handlebars<'a>,
}

pub fn run(
    sender: mpsc::Sender<Web2GuiMessage>,
    receiver: mpsc_tokio::Receiver<Gui2WebMessage>,
    bind_addr: &SocketAddr,
    ws_pointer_port: u16,
    ws_video_port: u16,
    password: Option<&str>,
) {
    let mut templates = Handlebars::new();
    templates
        .register_template_string("index", std::include_str!("../www/templates/index.html"))
        .unwrap();

    let password = match password {
        Some(password) => Some(password.to_string()),
        None => None,
    };

    let context = Context {
        bind_addr: *bind_addr,
        ws_pointer_port: ws_pointer_port,
        ws_video_port: ws_video_port,
        password: password,
        templates: templates,
    };
    std::thread::spawn(move || run_server(context, sender, receiver));
}

#[tokio::main]
async fn run_server(
    context: Context<'static>,
    sender: mpsc::Sender<Web2GuiMessage>,
    mut receiver: mpsc_tokio::Receiver<Gui2WebMessage>,
) {
    let addr = context.bind_addr;
    let context = Arc::new(context);

    let sender = sender.clone();
    let sender2 = sender.clone();
    let service = make_service_fn(move |_| {
        let context = context.clone();
        let sender = sender.clone();
        async move {
            Ok::<_, hyper::Error>(service_fn(move |req| {
                let context = context.clone();
                serve(req, context, sender.clone())
            }))
        }
    });
    let server = Server::bind(&addr).serve(service);
    let server = server.with_graceful_shutdown(async move {
        loop {
            match receiver.recv().await {
                Some(Gui2WebMessage::Shutdown) => return,
                None => return,
            }
        }
    });
    log_gui_send_error(sender2.send(Web2GuiMessage::Info(format!(
        "Webserver listening at {}...",
        addr
    ))));
    match server.await {
        Ok(_) => log_gui_send_error(sender2.send(Web2GuiMessage::Info("Webserver shutdown!".into()))),
        Err(err) => log_gui_send_error(sender2.send(Web2GuiMessage::Error(format!(
            "Webserver exited error: {}",
            err
        )))),
    };
    log_gui_send_error(sender2.send(Web2GuiMessage::Shutdown));
}
