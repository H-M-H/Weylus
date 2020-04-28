use handlebars::Handlebars;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::mpsc;
use tokio::sync::mpsc as mpsc_tokio;
use std::sync::Arc;

#[derive(Serialize)]
struct WebConfig {
    websocket_pointer_addr: String,
    websocket_video_addr: String,
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
    if req.method() != Method::GET {
        return Ok(response_not_found());
    }
    match req.uri().path() {
        "/" => {
            let config = WebConfig {
                websocket_pointer_addr: format!(
                    "{}:{}",
                    context.connect_addr, context.ws_pointer_port
                ),
                websocket_video_addr: format!("{}:{}", context.connect_addr, context.ws_video_port),
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

/*#[get("/")]
fn index(state: State<Handlebars>) -> Content<String> {
    let context = WebConfig {
        websocket_pointer_addr: "dirac.freedesk.lan:9001".to_string(),
        websocket_video_addr: "dirac.freedesk.lan:9002".to_string(),
    };
    Content(
        ContentType::HTML,
        (*state).render("index", &context).unwrap(),
    )
}

#[get("/<file_name>")]
fn static_files(file_name: String) -> Option<Content<&'static str>> {
    match file_name.as_str() {
        "lib.js" => Some(Content(
            ContentType::JavaScript,
            std::include_str!("../www/static/lib.js"),
        )),
        "style.css" => Some(Content(
            ContentType::CSS,
            std::include_str!("../www/static/style.css"),
        )),
        _ => None,
    }
}
*/

pub enum Gui2WebMessage {
    Shutdown,
}
pub enum Web2GuiMessage {}

struct Context<'a> {
    bind_addr: SocketAddr,
    connect_addr: String,
    ws_pointer_port: u16,
    ws_video_port: u16,
    password: Option<String>,
    templates: Handlebars<'a>,
}

pub fn run(
    sender: mpsc::Sender<Web2GuiMessage>,
    receiver: mpsc_tokio::Receiver<Gui2WebMessage>,
    bind_addr: &SocketAddr,
    connect_addr: &str,
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
        connect_addr: connect_addr.into(),
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
    server.await.unwrap();
}
