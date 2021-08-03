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
use tracing::{debug, error, info, warn};

pub const INDEX_HTML: &str = std::include_str!("../www/templates/index.html");
pub const ACCESS_HTML: &str = std::include_str!("../www/static/access_code.html");
pub const STYLE_CSS: &str = std::include_str!("../www/static/style.css");
pub const LIB_JS: &str = std::include_str!("../www/static/lib.js");

#[derive(Serialize)]
struct WebConfig {
    access_code: Option<String>,
    websocket_port: u16,
    uinput_enabled: bool,
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

fn response_from_path_or_default(
    path: Option<&String>,
    default: &str,
    content_type: &str,
) -> Response<Body> {
    match path {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(s) => response_from_str(&s, content_type),
            Err(err) => {
                warn!("Failed to load file: {}", err);
                response_from_str(default, content_type)
            }
        },
        None => response_from_str(default, content_type),
    }
}

async fn serve(
    addr: SocketAddr,
    req: Request<Body>,
    context: Arc<Context<'_>>,
    _sender: mpsc::Sender<Web2UiMessage>,
) -> Result<Response<Body>, hyper::Error> {
    debug!("Got request: {:?}", req);
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
                return Ok(response_from_path_or_default(
                    context.custom_access_html.as_ref(),
                    ACCESS_HTML,
                    "text/html; charset=utf-8",
                ));
            }
            info!("Client connected: {}", &addr);
            let config = WebConfig {
                access_code: context.access_code.clone(),
                websocket_port: context.ws_port,
                uinput_enabled: cfg!(target_os = "linux"),
                capture_cursor_enabled: cfg!(not(target_os = "windows")),
                log_level: crate::log::get_log_level().to_string(),
            };

            let html = if let Some(path) = context.custom_index_html.as_ref() {
                let mut reg = Handlebars::new();
                if let Err(err) = reg.register_template_file("index", path) {
                    warn!("Failed to register template from path: {}", err);
                    context.templates.render("index", &config)
                } else {
                    reg.render("index", &config)
                }
            } else {
                context.templates.render("index", &config)
            };

            match html {
                Ok(html) => Ok(response_from_str(&html, "text/html; charset=utf-8")),
                Err(err) => {
                    error!("Failed to render index template: {}", err);
                    Ok(response_not_found())
                }
            }
        }
        "/style.css" => Ok(response_from_path_or_default(
            context.custom_style_css.as_ref(),
            STYLE_CSS,
            "text/css; charset=utf-8",
        )),
        "/lib.js" => Ok(response_from_path_or_default(
            context.custom_lib_js.as_ref(),
            LIB_JS,
            "text/javascript; charset=utf-8",
        )),
        _ => Ok(response_not_found()),
    }
}

#[derive(Debug)]
pub enum Ui2WebMessage {
    Shutdown,
}
pub enum Web2UiMessage {
    Start,
    Error(String),
}

fn log_send_error<T>(res: Result<(), SendError<T>>) {
    if let Err(err) = res {
        warn!("Webserver: Failed to send message to gui: {}", err);
    }
}

struct Context<'a> {
    bind_addr: SocketAddr,
    ws_port: u16,
    access_code: Option<String>,
    custom_index_html: Option<String>,
    custom_access_html: Option<String>,
    custom_style_css: Option<String>,
    custom_lib_js: Option<String>,
    templates: Handlebars<'a>,
}

pub fn run(
    sender: mpsc::Sender<Web2UiMessage>,
    receiver: mpsc_tokio::Receiver<Ui2WebMessage>,
    bind_addr: &SocketAddr,
    ws_port: u16,
    access_code: Option<&str>,
    custom_index_html: Option<String>,
    custom_access_html: Option<String>,
    custom_style_css: Option<String>,
    custom_lib_js: Option<String>,
) -> std::thread::JoinHandle<()> {
    let mut templates = Handlebars::new();
    templates
        .register_template_string("index", INDEX_HTML)
        .unwrap();

    let access_code = match access_code {
        Some(access_code) => Some(access_code.to_string()),
        None => None,
    };

    let context = Context {
        bind_addr: *bind_addr,
        ws_port,
        access_code,
        custom_index_html,
        custom_access_html,
        custom_style_css,
        custom_lib_js,
        templates,
    };
    std::thread::spawn(move || run_server(context, sender, receiver))
}

#[tokio::main]
async fn run_server(
    context: Context<'static>,
    sender: mpsc::Sender<Web2UiMessage>,
    mut receiver: mpsc_tokio::Receiver<Ui2WebMessage>,
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
    let server = match Server::try_bind(&addr) {
        Ok(builder) => builder.serve(service),
        Err(err) => {
            log_send_error(sender2.send(Web2UiMessage::Error(format!(
                "Failed to start webserver: {}",
                err
            ))));
            return;
        }
    };
    let server = server.with_graceful_shutdown(async move {
        loop {
            match receiver.recv().await {
                Some(Ui2WebMessage::Shutdown) => break,
                None => break,
            }
        }
    });
    info!("Webserver listening at {}...", addr);
    log_send_error(sender2.send(Web2UiMessage::Start));
    if let Err(err) = server.await {
        error!("Webserver exited error: {}", err)
    };
}
