// Standalone VAAPI zero-copy feasibility probe. Build/run:
//   devenv shell -- cargo run --example vaapi_pipe_probe --features vaapi-probe
//
// Reuses ONE portal ScreenCast grant (you pick a monitor once), then runs a
// (import-path x drm-modifier) matrix all the way to an encoded H264 frame to
// find out whether a GPU-resident niri-dmabuf -> VAAPI path is possible.

#[path = "../src/capturable/remote_desktop_dbus.rs"]
mod remote_desktop_dbus;

use std::collections::HashMap;
use std::error::Error;
use std::os::raw::c_int;
use std::os::unix::io::AsRawFd;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dbus::{
    arg::{OwnedFd, PropMap, RefArg, Variant},
    blocking::{Proxy, SyncConnection},
    message::{MatchRule, MessageType},
    Message,
};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSink;
use gstreamer_video::VideoMeta;

use remote_desktop_dbus::{
    OrgFreedesktopPortalRemoteDesktop, OrgFreedesktopPortalRequestResponse,
    OrgFreedesktopPortalScreenCast,
};

extern "C" {
    fn vaapi_probe_selftest() -> c_int;
    fn vaapi_probe_encode_dmabuf(
        dmabuf_fd: c_int,
        width: u32,
        height: u32,
        drm_fourcc: u32,
        drm_modifier: u64,
        stride: u32,
        offset: u32,
        reason: *mut u8,
        reason_len: c_int,
    ) -> c_int;
}

/// DRM fourcc for XR24 (DRM_FORMAT_XRGB8888) = 'X','R','2','4' little-endian.
fn xr24_fourcc() -> u32 {
    u32::from_le_bytes([b'X', b'R', b'2', b'4'])
}

// ------------------------------------------------------------------------------------------------
// Portal (DBus) dance — copied verbatim from src/capturable/pipewire.rs, with tracing macros
// replaced by eprintln! so this is a self-contained binary.
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct PwStreamInfo {
    path: u64,
    source_type: u64,
}

#[derive(Debug)]
struct DBusError(String);
impl std::fmt::Display for DBusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl Error for DBusError {}

fn handle_response<F>(
    portal: Proxy<&SyncConnection>,
    path: dbus::Path<'static>,
    context: Arc<Mutex<CallBackContext>>,
    mut f: F,
) -> Result<dbus::channel::Token, dbus::Error>
where
    F: FnMut(
            OrgFreedesktopPortalRequestResponse,
            Proxy<&SyncConnection>,
            &Message,
            Arc<Mutex<CallBackContext>>,
        ) -> Result<(), Box<dyn Error>>
        + Send
        + Sync
        + 'static,
{
    let mut m = MatchRule::new();
    m.path = Some(path);
    m.msg_type = Some(MessageType::Signal);
    m.sender = Some("org.freedesktop.portal.Desktop".into());
    m.interface = Some("org.freedesktop.portal.Request".into());
    portal
        .connection
        .add_match(m, move |r: OrgFreedesktopPortalRequestResponse, c, m| {
            let portal = get_portal(c);
            eprintln!("[portal] response: {:?}", r.response);
            let _ = m;
            match r.response {
                0 => {}
                1 => {
                    context.lock().unwrap().failure = true;
                    eprintln!("[portal] User cancelled interaction.");
                    return true;
                }
                c => {
                    context.lock().unwrap().failure = true;
                    eprintln!("[portal] Unknown error, code: {}.", c);
                    return true;
                }
            }
            if let Err(err) = f(r, portal, m, context.clone()) {
                context.lock().unwrap().failure = true;
                eprintln!("[portal] Error requesting screen capture via dbus: {}", err);
            }
            true
        })
}

fn get_portal(conn: &SyncConnection) -> Proxy<'_, &SyncConnection> {
    conn.with_proxy(
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        Duration::from_millis(1000),
    )
}

fn streams_from_response(response: &OrgFreedesktopPortalRequestResponse) -> Vec<PwStreamInfo> {
    (move || {
        Some(
            response
                .results
                .get("streams")?
                .as_iter()?
                .next()?
                .as_iter()?
                .filter_map(|stream| {
                    let mut itr = stream.as_iter()?;
                    let path = itr.next()?.as_u64()?;
                    let (keys, values): (Vec<(usize, &dyn RefArg)>, Vec<(usize, &dyn RefArg)>) =
                        itr.next()?
                            .as_iter()?
                            .enumerate()
                            .partition(|(i, _)| i % 2 == 0);
                    let attributes = keys
                        .iter()
                        .filter_map(|(_, key)| Some(key.as_str()?.to_owned()))
                        .zip(
                            values
                                .iter()
                                .map(|(_, arg)| *arg)
                                .collect::<Vec<&dyn RefArg>>(),
                        )
                        .collect::<HashMap<String, &dyn RefArg>>();
                    Some(PwStreamInfo {
                        path,
                        source_type: attributes
                            .get("source_type")
                            .map_or(Some(0), |v| v.as_u64())?,
                    })
                })
                .collect::<Vec<PwStreamInfo>>(),
        )
    })()
    .unwrap_or_default()
}

struct CallBackContext {
    capture_cursor: bool,
    session: dbus::Path<'static>,
    streams: Vec<PwStreamInfo>,
    fd: Option<OwnedFd>,
    has_remote_desktop: bool,
    failure: bool,
}

fn on_create_session_response(
    r: OrgFreedesktopPortalRequestResponse,
    portal: Proxy<&SyncConnection>,
    _msg: &Message,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    let session: dbus::Path = r
        .results
        .get("session_handle")
        .ok_or_else(|| DBusError(format!("Failed to obtain session_handle: {:?}", r)))?
        .as_str()
        .ok_or_else(|| DBusError("Failed to convert session_handle to string.".into()))?
        .to_string()
        .into();

    context.lock().unwrap().session = session.clone();
    if context.lock().unwrap().has_remote_desktop {
        select_devices(portal, context)
    } else {
        select_sources(portal, context)
    }
}

fn select_devices(
    portal: Proxy<&SyncConnection>,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    let mut args: PropMap = HashMap::new();
    let t: usize = rand::random();
    args.insert(
        "handle_token".to_string(),
        Variant(Box::new(format!("weylus{t}"))),
    );
    args.insert("persist_mode".to_string(), Variant(Box::new(2u32)));
    let device_types = portal.available_device_types()?;
    args.insert("types".to_string(), Variant(Box::new(device_types)));
    let path = portal.select_devices(context.lock().unwrap().session.clone(), args)?;
    handle_response(portal, path, context, |_, portal, _, context| {
        select_sources(portal, context)
    })?;
    Ok(())
}

fn select_sources(
    portal: Proxy<&SyncConnection>,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    let mut args: PropMap = HashMap::new();
    let t: usize = rand::random();
    args.insert(
        "handle_token".to_string(),
        Variant(Box::new(format!("weylus{t}"))),
    );
    args.insert("multiple".into(), Variant(Box::new(true)));
    let source_types = portal.available_source_types()?;
    args.insert("types".into(), Variant(Box::new(source_types)));
    let capture_cursor = context.lock().unwrap().capture_cursor;
    let cursor_mode = if capture_cursor { 2u32 } else { 1u32 };
    args.insert("cursor_mode".into(), Variant(Box::new(cursor_mode)));
    let path = portal.select_sources(context.lock().unwrap().session.clone(), args)?;
    handle_response(portal, path, context, on_select_sources_response)?;
    Ok(())
}

fn on_select_sources_response(
    _r: OrgFreedesktopPortalRequestResponse,
    portal: Proxy<&SyncConnection>,
    _msg: &Message,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    let mut args: PropMap = HashMap::new();
    let t: usize = rand::random();
    args.insert(
        "handle_token".to_string(),
        Variant(Box::new(format!("weylus{t}"))),
    );
    let path = if context.lock().unwrap().has_remote_desktop {
        OrgFreedesktopPortalRemoteDesktop::start(
            &portal,
            context.lock().unwrap().session.clone(),
            "",
            args,
        )?
    } else {
        OrgFreedesktopPortalScreenCast::start(
            &portal,
            context.lock().unwrap().session.clone(),
            "",
            args,
        )?
    };
    handle_response(portal, path, context, on_start_response)?;
    Ok(())
}

fn on_start_response(
    r: OrgFreedesktopPortalRequestResponse,
    portal: Proxy<&SyncConnection>,
    _msg: &Message,
    context: Arc<Mutex<CallBackContext>>,
) -> Result<(), Box<dyn Error>> {
    let mut context = context.lock().unwrap();
    context.streams.append(&mut streams_from_response(&r));
    let session = context.session.clone();
    context
        .fd
        .replace(portal.open_pipe_wire_remote(session.clone(), HashMap::new())?);
    eprintln!("[portal] Screen Cast Session started");
    Ok(())
}

fn request_remote_desktop(
    capture_cursor: bool,
) -> Result<(SyncConnection, OwnedFd, Vec<PwStreamInfo>), Box<dyn Error>> {
    let conn = SyncConnection::new_session()?;
    let portal = get_portal(&conn);
    let has_remote_desktop =
        std::env::var("DESKTOP_SESSION").map_or(false, |s| s.contains("gnome"));

    let context = CallBackContext {
        capture_cursor,
        session: Default::default(),
        streams: Default::default(),
        fd: None,
        has_remote_desktop,
        failure: false,
    };
    let context = Arc::new(Mutex::new(context));

    let mut args: PropMap = HashMap::new();
    let t1: usize = rand::random();
    let t2: usize = rand::random();
    args.insert(
        "session_handle_token".to_string(),
        Variant(Box::new(format!("weylus{t1}"))),
    );
    args.insert(
        "handle_token".to_string(),
        Variant(Box::new(format!("weylus{t2}"))),
    );
    let path = if has_remote_desktop {
        OrgFreedesktopPortalRemoteDesktop::create_session(&portal, args)?
    } else {
        OrgFreedesktopPortalScreenCast::create_session(&portal, args)?
    };
    handle_response(portal, path, context.clone(), on_create_session_response)?;

    eprintln!("[portal] waiting for you to pick a source in the portal dialog...");
    for _ in 0..1800 {
        conn.process(Duration::from_millis(100))?;
        let ctx = context.lock().unwrap();
        if ctx.fd.is_some() || ctx.failure {
            break;
        }
    }
    let ctx = context.lock().unwrap();
    if ctx.fd.is_some() && !ctx.streams.is_empty() {
        Ok((conn, ctx.fd.clone().unwrap(), ctx.streams.clone()))
    } else {
        Err(Box::new(DBusError("Failed to obtain screen capture.".into())))
    }
}

// ------------------------------------------------------------------------------------------------
// Matrix model: import-path x drm-modifier.
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ImportPath {
    DirectVa,
    GlToVa,
    FfmpegImport,
    CpuControl,
}

impl ImportPath {
    fn short(&self) -> &'static str {
        match self {
            ImportPath::DirectVa => "A/direct-va",
            ImportPath::GlToVa => "B/gl-to-va",
            ImportPath::FfmpegImport => "C/ffmpeg-import",
            ImportPath::CpuControl => "D/cpu-control",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Modifier {
    name: &'static str,
    drm: u64,
}

const MODIFIERS: [Modifier; 4] = [
    Modifier { name: "LINEAR", drm: 0x0 },
    Modifier { name: "X_TILED", drm: 0x0100000000000001 },
    Modifier { name: "Y_TILED", drm: 0x0100000000000002 },
    Modifier { name: "Y_TILED_CCS", drm: 0x0100000000000008 },
];

const PATHS: [ImportPath; 4] = [
    ImportPath::DirectVa,
    ImportPath::GlToVa,
    ImportPath::FfmpegImport,
    ImportPath::CpuControl,
];

#[derive(Clone, Copy)]
struct Cell {
    path: ImportPath,
    modifier: Modifier,
}

fn all_cells() -> Vec<Cell> {
    let mut v = Vec::with_capacity(16);
    for path in PATHS {
        for modifier in MODIFIERS {
            v.push(Cell { path, modifier });
        }
    }
    v
}

fn drm_fourcc_mod_string(m: &Modifier) -> String {
    if m.drm == 0 {
        "XR24".to_string()
    } else {
        format!("XR24:{:#018x}", m.drm)
    }
}

fn xr24_caps(mods: &[&str]) -> String {
    let list = mods
        .iter()
        .map(|m| format!("(string){m}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "video/x-raw(memory:DMABuf), format=(string)DMA_DRM, \
         width=(int)[1,32767], height=(int)[1,32767], \
         framerate=(fraction)[0/1,2147483647/1], \
         drm-format=(list){{ {list} }}"
    )
}

fn cell_label(c: &Cell) -> String {
    format!("{}/{}", c.path.short(), c.modifier.name)
}

fn cell_enabled(c: &Cell) -> bool {
    match std::env::var("PROBE_ONLY") {
        Ok(s) if !s.is_empty() => cell_label(c).contains(&s),
        _ => true,
    }
}

// ------------------------------------------------------------------------------------------------
// Per-cell runner: build a fresh pipeline, drive to first frame, time ~30 frames.
// ------------------------------------------------------------------------------------------------

#[derive(Default, Clone)]
struct CellResult {
    label: String,
    ok: bool,
    first_frame_bytes: usize,
    avg_ms: f64,
    first_latency_ms: f64,
    negotiated: String,
    reason: String,
}

fn mk(name: &str) -> Result<gst::Element, Box<dyn Error>> {
    gst::ElementFactory::make(name)
        .build()
        .map_err(|e| Box::new(DBusError(format!("make {name}: {e}"))) as Box<dyn Error>)
}

fn h264_appsink() -> Result<AppSink, Box<dyn Error>> {
    let s = mk("appsink")?;
    s.set_property("drop", &true);
    s.set_property("max-buffers", &1u32);
    let a = s
        .dynamic_cast::<AppSink>()
        .map_err(|_| DBusError("not appsink".into()))?;
    a.set_caps(Some(&"video/x-h264".parse::<gst::Caps>()?));
    Ok(a)
}

fn bgrx_appsink() -> Result<AppSink, Box<dyn Error>> {
    let s = mk("appsink")?;
    s.set_property("drop", &true);
    s.set_property("max-buffers", &1u32);
    let a = s
        .dynamic_cast::<AppSink>()
        .map_err(|_| DBusError("not appsink".into()))?;
    let mut caps = gst::Caps::new_empty();
    caps.merge_structure(gst::structure::Structure::from_iter(
        "video/x-raw",
        [("format", "BGRx".into())],
    ));
    caps.merge_structure(gst::structure::Structure::from_iter(
        "video/x-raw",
        [("format", "RGBx".into())],
    ));
    a.set_caps(Some(&caps));
    Ok(a)
}

// Returns (pipeline, appsink, is_h264_sink). Handles A/B/D; C is run elsewhere.
fn build_gst_pipeline(
    fd: i32,
    node: u64,
    cell: &Cell,
) -> Result<(gst::Pipeline, AppSink, bool), Box<dyn Error>> {
    let pipeline = gst::Pipeline::new();
    let src = mk("pipewiresrc")?;
    src.set_property("fd", &fd);
    src.set_property("path", &format!("{node}"));
    // A/B/D all consume the GPU dmabuf; never copy it to a 1-byte sysmem buffer.
    src.set_property("always-copy", &false);

    // A modifier-forcing capsfilter for the paths that sweep modifiers (B, D).
    // Path A deliberately omits it: we let pipewiresrc and vapostproc negotiate
    // freely, so a failure proves niri's format is fundamentally unacceptable to
    // the VA element (not just that we over-constrained the caps).
    let capsfilter = || -> Result<gst::Element, Box<dyn Error>> {
        let cf = mk("capsfilter")?;
        cf.set_property(
            "caps",
            &xr24_caps(&[&drm_fourcc_mod_string(&cell.modifier)]).parse::<gst::Caps>()?,
        );
        Ok(cf)
    };

    let mut chain: Vec<gst::Element> = vec![src];
    let is_h264;
    let sink;

    match cell.path {
        ImportPath::DirectVa => {
            // Free negotiation — no capsfilter.
            chain.push(mk("vapostproc")?);
            chain.push(mk("vah264enc").or_else(|_| mk("vah264lpenc"))?);
            sink = h264_appsink()?;
            is_h264 = true;
        }
        ImportPath::GlToVa => {
            chain.push(capsfilter()?);
            chain.push(mk("glupload")?);
            chain.push(mk("glcolorconvert")?);
            chain.push(mk("vapostproc")?);
            chain.push(mk("vah264enc").or_else(|_| mk("vah264lpenc"))?);
            sink = h264_appsink()?;
            is_h264 = true;
        }
        ImportPath::CpuControl => {
            chain.push(capsfilter()?);
            chain.push(mk("glupload")?);
            chain.push(mk("glcolorconvert")?);
            chain.push(mk("gldownload")?);
            chain.push(mk("videoconvert")?);
            sink = bgrx_appsink()?;
            is_h264 = false;
        }
        ImportPath::FfmpegImport => {
            return Err(Box::new(DBusError("path C handled by run_ffmpeg_cell".into())));
        }
    }

    chain.push(sink.clone().upcast::<gst::Element>());
    pipeline.add_many(&chain)?;
    for w in chain.windows(2) {
        w[0].link(&w[1]).map_err(|e| {
            DBusError(format!("link {} -> {}: {e}", w[0].name(), w[1].name()))
        })?;
    }
    Ok((pipeline, sink, is_h264))
}

fn drain_bus(pipeline: &gst::Pipeline) -> String {
    let bus = match pipeline.bus() {
        Some(b) => b,
        None => return "no bus".into(),
    };
    let mut msgs = Vec::new();
    while let Some(msg) =
        bus.timed_pop_filtered(gst::ClockTime::from_mseconds(300), &[gst::MessageType::Error])
    {
        if let gst::MessageView::Error(err) = msg.view() {
            msgs.push(format!(
                "{} ({})",
                err.error(),
                err.debug().unwrap_or_else(|| "none".into())
            ));
        }
    }
    if msgs.is_empty() {
        "no error on bus".into()
    } else {
        msgs.join("; ")
    }
}

fn run_gst_cell(fd: i32, node: u64, cell: &Cell) -> CellResult {
    let label = cell_label(cell);
    let mut r = CellResult {
        label: label.clone(),
        ..Default::default()
    };

    let (pipeline, appsink, is_h264) = match build_gst_pipeline(fd, node, cell) {
        Ok(t) => t,
        Err(e) => {
            r.reason = format!("build: {e}");
            return r;
        }
    };

    if pipeline.set_state(gst::State::Playing).is_err() {
        r.reason = format!("set_state(Playing) failed: {}", drain_bus(&pipeline));
        let _ = pipeline.set_state(gst::State::Null);
        return r;
    }
    // Wait for preroll / async state change.
    let _ = pipeline.state(gst::ClockTime::from_seconds(5));

    let t0 = Instant::now();
    let first = appsink.try_pull_sample(gst::ClockTime::from_seconds(5));
    let first_latency = t0.elapsed().as_secs_f64() * 1000.0;
    let sample = match first {
        Some(s) => s,
        None => {
            r.reason = format!("no frame in 5s: {}", drain_bus(&pipeline));
            let _ = pipeline.set_state(gst::State::Null);
            return r;
        }
    };

    r.negotiated = sample.caps().map(|c| c.to_string()).unwrap_or_default();
    r.first_frame_bytes = sample.buffer().map(|b| b.size()).unwrap_or(0);
    // For the D control (raw BGRx), a produced frame means capture works; the
    // encode side is exercised by A/B/C. For A/B this is already an H264 packet.
    let _ = is_h264;

    // Time ~30 more pulls.
    let mut n = 0u32;
    let tstart = Instant::now();
    while n < 30 {
        if appsink
            .try_pull_sample(gst::ClockTime::from_mseconds(200))
            .is_some()
        {
            n += 1;
        } else {
            break;
        }
    }
    if n > 0 {
        r.avg_ms = tstart.elapsed().as_secs_f64() * 1000.0 / n as f64;
    }
    r.first_latency_ms = first_latency;
    r.ok = true;

    let _ = pipeline.set_state(gst::State::Null);
    std::thread::sleep(Duration::from_millis(300)); // pw#982 node release
    r
}

fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().any(|a| a == "--dry-run") {
        for c in all_cells().into_iter().filter(cell_enabled) {
            println!(
                "cell {:22}  drm-format={}",
                cell_label(&c),
                drm_fourcc_mod_string(&c.modifier)
            );
        }
        return Ok(());
    }

    // Worker mode: run exactly ONE cell against an inherited pipewire fd, print a
    // machine-readable RESULT line, and exit. Each cell runs in its own process so
    // a GL/VA crash (libgstgl _gl_mem_create, seen under niri) kills only that cell
    // and cannot corrupt GL/VA global state for the others. A crashed worker is
    // reported as `worker crashed` — never mistaken for a negotiation verdict.
    if let Ok(id) = std::env::var("PROBE_RUN_ONE") {
        return run_worker(&id);
    }

    // Coordinator: acquire one portal grant, then spawn a worker per enabled cell.
    eprintln!("[portal] a desktop portal dialog will open — pick a MONITOR to cast.");
    let (_conn, fd, streams) = request_remote_desktop(false)?;
    let node = streams
        .iter()
        .find(|s| s.source_type == 1)
        .or_else(|| streams.first())
        .map(|s| s.path)
        .ok_or_else(|| DBusError("no stream".into()))?;
    let raw = fd.as_raw_fd();
    eprintln!("[portal] pipewire fd = {raw}, node = {node}");
    // Clear FD_CLOEXEC so the worker subprocesses inherit this fd across exec.
    unsafe {
        libc::fcntl(raw, libc::F_SETFD, 0);
    }
    let exe = std::env::current_exe()?;

    let mut results = Vec::new();
    for cell in all_cells().into_iter().filter(cell_enabled) {
        // Path A negotiates freely (format-agnostic), so its 4 modifier cells are
        // identical — run it only once.
        if cell.path == ImportPath::DirectVa && cell.modifier.name != "LINEAR" {
            continue;
        }
        let id = cell_label(&cell);
        let hdr = if cell.path == ImportPath::DirectVa {
            "A/direct-va/(free-negotiate)".to_string()
        } else {
            id.clone()
        };
        eprintln!("=== {hdr} ===");
        let r = spawn_cell(&exe, raw, node, &id);
        eprintln!(
            "  ok={} first_bytes={} first_latency={:.1}ms avg={:.2}ms\n  reason={}\n  caps={}",
            r.ok, r.first_frame_bytes, r.first_latency_ms, r.avg_ms, r.reason, r.negotiated
        );
        results.push(r);
    }
    print_summary(&results);
    Ok(())
}

fn print_summary(results: &[CellResult]) {
    println!("\n================ SUMMARY ================");
    println!(
        "{:<30} {:>3} {:>12} {:>10} {:>9}  {}",
        "cell", "ok", "first_bytes", "first_ms", "avg_ms", "note/reason"
    );
    for r in results {
        println!(
            "{:<30} {:>3} {:>12} {:>10.1} {:>9.2}  {}",
            r.label,
            if r.ok { "Y" } else { "n" },
            r.first_frame_bytes,
            r.first_latency_ms,
            r.avg_ms,
            if r.reason.is_empty() { "-" } else { &r.reason }
        );
    }
}

/// Coordinator side: run one cell in a fresh worker subprocess and collect its
/// RESULT. A worker that dies (segfault/abort) yields a `worker crashed` result
/// rather than aborting the whole matrix.
fn spawn_cell(exe: &std::path::Path, fd: i32, node: u64, id: &str) -> CellResult {
    match Command::new(exe)
        .env("PROBE_RUN_ONE", id)
        .env("PROBE_FD", fd.to_string())
        .env("PROBE_NODE", node.to_string())
        .stderr(Stdio::null())
        .output()
    {
        Ok(o) => parse_result(&o.stdout).unwrap_or_else(|| CellResult {
            label: id.to_string(),
            reason: format!("worker crashed/died ({}) — GL/VA crash, not a verdict", o.status),
            ..Default::default()
        }),
        Err(e) => CellResult {
            label: id.to_string(),
            reason: format!("spawn failed: {e}"),
            ..Default::default()
        },
    }
}

/// Worker side: build + run exactly one cell, print a tab-separated RESULT line.
fn run_worker(id: &str) -> Result<(), Box<dyn Error>> {
    gst::init()?;
    assert_eq!(unsafe { vaapi_probe_selftest() }, 42);
    let fd: i32 = std::env::var("PROBE_FD")?
        .parse()
        .map_err(|_| DBusError("bad PROBE_FD".into()))?;
    let node: u64 = std::env::var("PROBE_NODE")?
        .parse()
        .map_err(|_| DBusError("bad PROBE_NODE".into()))?;
    let cell = parse_cell(id).ok_or_else(|| DBusError(format!("bad cell id {id}")))?;
    let r = if cell.path == ImportPath::FfmpegImport {
        run_ffmpeg_cell(fd, node, &cell)
    } else {
        run_gst_cell(fd, node, &cell)
    };
    println!(
        "RESULT\t{}\t{}\t{}\t{:.3}\t{:.3}\t{}\t{}",
        r.label,
        r.ok as u8,
        r.first_frame_bytes,
        r.first_latency_ms,
        r.avg_ms,
        san(&r.reason),
        san(&r.negotiated)
    );
    Ok(())
}

fn parse_cell(label: &str) -> Option<Cell> {
    let (path_str, mod_name) = label.rsplit_once('/')?;
    let path = PATHS.into_iter().find(|p| p.short() == path_str)?;
    let modifier = MODIFIERS.into_iter().find(|m| m.name == mod_name)?;
    Some(Cell { path, modifier })
}

fn parse_result(out: &[u8]) -> Option<CellResult> {
    let s = String::from_utf8_lossy(out);
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("RESULT\t") {
            let f: Vec<&str> = rest.split('\t').collect();
            if f.len() >= 7 {
                return Some(CellResult {
                    label: f[0].to_string(),
                    ok: f[1] == "1",
                    first_frame_bytes: f[2].parse().unwrap_or(0),
                    first_latency_ms: f[3].parse().unwrap_or(0.0),
                    avg_ms: f[4].parse().unwrap_or(0.0),
                    reason: f[5].to_string(),
                    negotiated: f[6].to_string(),
                });
            }
        }
    }
    None
}

fn san(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn dmabuf_appsink() -> Result<AppSink, Box<dyn Error>> {
    let s = mk("appsink")?;
    s.set_property("drop", &true);
    s.set_property("max-buffers", &1u32);
    let a = s
        .dynamic_cast::<AppSink>()
        .map_err(|_| DBusError("not appsink".into()))?;
    // Keep the buffer as a DMA_DRM dmabuf — do NOT download to system memory.
    a.set_caps(Some(
        &"video/x-raw(memory:DMABuf), format=(string)DMA_DRM".parse::<gst::Caps>()?,
    ));
    Ok(a)
}

/// Path C: pull a DMA_DRM dmabuf from `pipewiresrc`, extract fd/stride/offset, and
/// hand it to `lib/vaapi_import_probe.c` which imports it into VAAPI via
/// `hwmap=derive_device=vaapi` and encodes one `h264_vaapi` frame.
fn run_ffmpeg_cell(fd: i32, node: u64, cell: &Cell) -> CellResult {
    let label = cell_label(cell);
    let mut r = CellResult {
        label: label.clone(),
        ..Default::default()
    };

    let pipeline = gst::Pipeline::new();
    let build = (|| -> Result<AppSink, Box<dyn Error>> {
        let src = mk("pipewiresrc")?;
        src.set_property("fd", &fd);
        src.set_property("path", &format!("{node}"));
        src.set_property("always-copy", &false);
        let capsfilter = mk("capsfilter")?;
        capsfilter.set_property(
            "caps",
            &xr24_caps(&[&drm_fourcc_mod_string(&cell.modifier)]).parse::<gst::Caps>()?,
        );
        let sink = dmabuf_appsink()?;
        let sink_el = sink.clone().upcast::<gst::Element>();
        pipeline.add_many([&src, &capsfilter, &sink_el])?;
        src.link(&capsfilter)?;
        capsfilter.link(&sink_el)?;
        Ok(sink)
    })();
    let appsink = match build {
        Ok(a) => a,
        Err(e) => {
            r.reason = format!("build: {e}");
            return r;
        }
    };

    if pipeline.set_state(gst::State::Playing).is_err() {
        r.reason = format!("set_state(Playing) failed: {}", drain_bus(&pipeline));
        let _ = pipeline.set_state(gst::State::Null);
        return r;
    }
    let _ = pipeline.state(gst::ClockTime::from_seconds(5));

    let sample = match appsink.try_pull_sample(gst::ClockTime::from_seconds(5)) {
        Some(s) => s,
        None => {
            r.reason = format!("no dmabuf frame in 5s: {}", drain_bus(&pipeline));
            let _ = pipeline.set_state(gst::State::Null);
            return r;
        }
    };
    r.negotiated = sample.caps().map(|c| c.to_string()).unwrap_or_default();

    let buffer = match sample.buffer() {
        Some(b) => b,
        None => {
            r.reason = "sample has no buffer".into();
            let _ = pipeline.set_state(gst::State::Null);
            return r;
        }
    };
    let st = sample.caps().unwrap().structure(0).unwrap().to_owned();
    let w = st.get::<i32>("width").unwrap_or(0) as u32;
    let h = st.get::<i32>("height").unwrap_or(0) as u32;
    let vmeta = buffer.meta::<VideoMeta>();
    let stride = vmeta
        .as_ref()
        .map(|m| m.stride()[0])
        .unwrap_or((w as i32) * 4) as u32;
    let offset = vmeta.as_ref().map(|m| m.offset()[0]).unwrap_or(0) as u32;
    let dfd = buffer
        .peek_memory(0)
        .downcast_memory_ref::<gstreamer_allocators::DmaBufMemory>()
        .map(|m| m.fd());
    let dfd = match dfd {
        Some(x) => x,
        None => {
            r.reason = "buffer memory is not a DmaBufMemory".into();
            let _ = pipeline.set_state(gst::State::Null);
            return r;
        }
    };

    let mut reason = vec![0u8; 256];
    let t0 = Instant::now();
    let bytes = unsafe {
        vaapi_probe_encode_dmabuf(
            dfd,
            w,
            h,
            xr24_fourcc(),
            cell.modifier.drm,
            stride,
            offset,
            reason.as_mut_ptr(),
            reason.len() as c_int,
        )
    };
    r.first_latency_ms = t0.elapsed().as_secs_f64() * 1000.0;

    if bytes >= 0 {
        r.ok = true;
        r.first_frame_bytes = bytes as usize;
    } else {
        let end = reason.iter().position(|&b| b == 0).unwrap_or(reason.len());
        r.reason = String::from_utf8_lossy(&reason[..end]).into_owned();
    }

    let _ = pipeline.set_state(gst::State::Null);
    std::thread::sleep(Duration::from_millis(300));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_has_16_cells_in_path_major_order() {
        let cells = all_cells();
        assert_eq!(cells.len(), 16);
        assert_eq!(cells[0].path, ImportPath::DirectVa);
        assert_eq!(cells[0].modifier.name, "LINEAR");
        assert_eq!(cells[3].modifier.name, "Y_TILED_CCS");
        assert_eq!(cells[4].path, ImportPath::GlToVa);
    }

    #[test]
    fn linear_uses_bare_fourcc_others_carry_modifier() {
        assert_eq!(drm_fourcc_mod_string(&MODIFIERS[0]), "XR24"); // LINEAR
        assert_eq!(
            drm_fourcc_mod_string(&MODIFIERS[3]),
            "XR24:0x0100000000000008" // Y_TILED_CCS
        );
    }

    #[test]
    fn caps_builder_emits_dmabuf_drm_list() {
        let caps = xr24_caps(&["XR24", "XR24:0x0100000000000002"]);
        assert!(caps.contains("memory:DMABuf"));
        assert!(caps.contains("DMA_DRM"));
        assert!(caps.contains("drm-format=(list){"));
        assert!(caps.contains("XR24:0x0100000000000002"));
    }

    #[test]
    fn probe_only_filter_matches_label_substring() {
        let cell = Cell {
            path: ImportPath::FfmpegImport,
            modifier: MODIFIERS[3],
        };
        assert!(cell_label(&cell).contains("Y_TILED_CCS"));
        assert!(cell_label(&cell).contains("ffmpeg"));
    }
}
