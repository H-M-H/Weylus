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
use tracing::{error, info, warn};

#[derive(Serialize)]
struct WebConfig {
    password: Option<String>,
    websocket_port: u16,
}

fn response_from_str(s: &str, content_type: &str) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .body(s.to_string().into())
        .unwrap()
}

fn response_from_bytes(b: &'static [u8], content_type: &str) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .body(b.into())
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
                        info!("Client authenticated: {}.", &addr);
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
            info!("Client connected: {}", &addr);
            let config = WebConfig {
                password: context.password.clone(),
                websocket_port: context.ws_port,
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
        "/background_paper.png" => Ok(response_from_bytes(
            std::include_bytes!("../www/static/background_paper.png"),
            "image/png",
        )),
        "/background_paper_invert.png" => Ok(response_from_bytes(
            std::include_bytes!("../www/static/background_paper_invert.png"),
            "image/png",
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
    password: Option<String>,
    templates: Handlebars<'a>,
}

pub fn run(
    sender: mpsc::Sender<Web2GuiMessage>,
    receiver: mpsc_tokio::Receiver<Gui2WebMessage>,
    bind_addr: &SocketAddr,
    ws_port: u16,
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
        ws_port,
        password,
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
