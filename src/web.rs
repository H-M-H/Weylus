use handlebars::Handlebars;
use hyper::service::{make_service_fn, service_fn};
use hyper::{server::conn::AddrStream, Body, Method, Request, Response, Server, StatusCode};
use serde::Serialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::mpsc;
use std::sync::mpsc::SendError;
use std::sync::Arc;
use tokio::sync::mpsc as mpsc_tokio;
use tracing::{error, info, warn, debug};

#[derive(Serialize)]
struct WebConfig {
    access_code: Option<String>,
    websocket_port: u16,
    stylus_support_enabled: bool,
    faster_capture_enabled: bool,
    capture_cursor_enabled: bool,
    log_level: String,
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
    addr: SocketAddr,
    req: Request<Body>,
    context: Arc<Context<'a>>,
    _sender: mpsc::Sender<Web2GuiMessage>,
) -> Result<Response<Body>, hyper::Error> {
    debug!("Got request: {:?}", req);
    let context = &*context;
    let mut authed = false;
    if let Some(access_code) = &context.access_code {
        if req.method() == Method::GET && req.uri().path() == "/" {
            use url::form_urlencoded;
            if let Some(query) = req.uri().query() {
                let params = form_urlencoded::parse(query.as_bytes())
                    .into_owned()
                    .collect::<HashMap<String, String>>();
                if let Some(code) = params.get("access_code") {
                    if code == access_code {
                        authed = true;
                        info!("Web-Client authenticated: {}.", &addr);
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
                    std::include_str!("../www/static/access_code.html"),
                    "text/html; charset=utf-8",
                ));
            }
            info!("Client connected: {}", &addr);
            let config = WebConfig {
                access_code: context.access_code.clone(),
                websocket_port: context.ws_port,
                stylus_support_enabled: cfg!(target_os = "linux"),
                faster_capture_enabled: cfg!(target_os = "linux"),
                capture_cursor_enabled: cfg!(target_os = "linux"),
                log_level: crate::log::get_log_level().to_string(),
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
    Shutdown,
}

fn log_gui_send_error<T>(res: Result<(), SendError<T>>) {
    if let Err(err) = res {
        warn!("Webserver: Failed to send message to gui: {}", err);
    }
}

struct Context<'a> {
    bind_addr: SocketAddr,
    ws_port: u16,
    access_code: Option<String>,
    templates: Handlebars<'a>,
}

pub fn run(
    sender: mpsc::Sender<Web2GuiMessage>,
    receiver: mpsc_tokio::Receiver<Gui2WebMessage>,
    bind_addr: &SocketAddr,
    ws_port: u16,
    access_code: Option<&str>,
) {
    let mut templates = Handlebars::new();
    templates
        .register_template_string("index", std::include_str!("../www/templates/index.html"))
        .unwrap();

    let access_code = match access_code {
        Some(access_code) => Some(access_code.to_string()),
        None => None,
    };

    let context = Context {
        bind_addr: *bind_addr,
        ws_port,
        access_code,
        templates,
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
    let service = make_service_fn(move |s: &AddrStream| {
        let addr = s.remote_addr();
        let context = context.clone();
        let sender = sender.clone();
        async move {
            Ok::<_, hyper::Error>(service_fn(move |req| {
                let context = context.clone();
                serve(addr, req, context, sender.clone())
            }))
        }
    });
    let server = Server::bind(&addr).serve(service);
    let server = server.with_graceful_shutdown(async move {
        match receiver.recv().await {
            Some(Gui2WebMessage::Shutdown) => return,
            None => return,
        }
    });
    info!("Webserver listening at {}...", addr);
    match server.await {
        Ok(_) => info!("Webserver shutdown!"),
        Err(err) => error!("Webserver exited error: {}", err),
    };
    log_gui_send_error(sender2.send(Web2GuiMessage::Shutdown));
}
