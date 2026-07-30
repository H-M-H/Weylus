// Standalone PipeWire screen-cast probe extracted from weylus.
//
// Purpose: reproduce ONLY the "portal ScreenCast -> GStreamer pipeline" path so the
// GStreamer negotiation with the Wayland compositor (niri) can be debugged in isolation.
// The only interactive step is clicking the source (e.g. DP-1) in the portal dialog once;
// the probe then runs an automated pipeline/modifier test matrix against that single grant.
//
// Everything else (weylus web server, encoder, input) is intentionally omitted.
//
// Run:
//   cargo run --example pw_probe
//   GST_DEBUG=glupload:6,gleglimage:6 cargo run --example pw_probe   # GL import internals
//   GST_DEBUG=pipewiresrc:5           cargo run --example pw_probe   # niri<->src negotiation
//   PROBE_ALWAYS_COPY=1               cargo run --example pw_probe   # compare always-copy
//   PROBE_ONLY=<substring>            cargo run --example pw_probe   # run one matrix entry
// Edit the `matrix` in main() to change what gets tested.
//
// =============================================================================================
// INVESTIGATION LOG  --  why weylus showed a black screen when capturing DP-1 on niri/Wayland,
// and how this harness was used to find the fix. (Kept here so the reasoning survives with the
// tool that produced it.)
// =============================================================================================
//
// SYMPTOM
//   Weylus streamed a black screen from a niri monitor. The GStreamer pipeline either failed to
//   build, failed to reach PLAYING, or silently fell back to the X11/XWayland capturable (which
//   cannot see native Wayland windows -> black).
//
// METHODOLOGY
//   * GST_DEBUG *does* reach stderr from GStreamer here (an early wrong turn assumed it didn't --
//     it was simply not set on some runs). gst-launch/this probe with GST_DEBUG is the fast loop;
//     rebuilding all of weylus + driving a browser is not.
//   * This probe copies weylus's portal (DBus) dance verbatim and exposes the raw fd + node id,
//     then builds candidate pipelines directly. Reusing ONE portal grant to run a whole matrix
//     of (converter chain x drm modifier) combinations is what made progress cheap: one DP-1
//     click, ~8 hypotheses tested, each reporting negotiated caps + whether the resulting buffer
//     actually CPU-maps to real pixels.
//   * Evidence over guessing: every claim below was confirmed by a probe run or by reading the
//     niri / pipewiresrc source (../niri-wts/niri-26.04, ../pipewire), not assumed.
//
// DISCOVERIES (each proven, not assumed)
//   1. LINK BUG: weylus built the capsfilter with `drm-format=(list)< ... >`. `< >` is a
//      GstValueArray (ordered tuple); a list of alternatives needs `{ }` (GstValueList). An
//      array value does not intersect a list-valued `drm-format` sink pad, so `Element::link`
//      failed with the generic "Failed to link elements". Verified with Caps::from_str +
//      Element::link in isolation: `< >` -> GstValueArray -> FAIL, `{ }` -> GstValueList -> OK.
//   2. FORMAT: for a MONITOR/output cast niri offers exactly ONE fourcc, XRGB8888 = `XR24`
//      (niri mod.rs:409 passes alpha=false for outputs); ARGB8888 = `AR24` is window-only
//      (alpha=true). vapostproc's DMABuf sink imports AB24/AR24/XB24 but NOT XR24 -> vapostproc
//      can never work for a niri monitor. The intended importer is glupload (EGL dmabuf import).
//   3. MODIFIER / MULTI-PLANE: niri defaults to a CCS modifier (Y_TILED_GEN12_MC_CCS,
//      0x0100000000000008). CCS is multi-plane (colour buffer + a control-surface plane) but the
//      screen-cast stream carries a SINGLE plane, so glupload's `eglCreateImage` fails with
//      EGL_BAD_MATCH and it falls through to a CPU path that cannot map the buffer. Restricting
//      the capsfilter to single-plane, non-CCS modifiers (LINEAR / X_TILED / Y_TILED) makes the
//      EGL import succeed; the probe then gets full-size BGRx frames with real pixels. (LINEAR is
//      written as the BARE fourcc `XR24`, not `XR24:0x0`; the ":0x0" spelling does not match.)
//   4. always-copy: niri deliberately sets chunk->size = maxsize = 1 on its dmabufs (the real
//      data lives in the fd; the size fields are dummy -- pw_utils.rs:743,1427). weylus set
//      pipewiresrc `always-copy=true`, which makes pipewiresrc gst_memory_copy that "1 byte" into
//      a 1-byte SYSTEM buffer, discarding the fd, so the GL importer has nothing to import. It
//      MUST be false for the dmabuf path. (It was originally true for a teardown hang, pw#982;
//      not observed on pipewire 1.7 across many probe teardowns.)
//
// WORKING PIPELINE (verified here: 5120x2160 BGRx, 44236800 bytes, buffer maps to real pixels)
//   pipewiresrc(always-copy=false)
//     -> capsfilter( video/x-raw(memory:DMABuf), DMA_DRM,
//                    drm-format={ XR24, XR24:0x..01, XR24:0x..02,
//                                 AR24, AR24:0x..01, AR24:0x..02 } )   // single-plane, no CCS
//     -> glupload -> glcolorconvert -> gldownload -> videoconvert
//     -> appsink(BGRx/RGBx)
//   niri picks XR24@Y_TILED from that list (fast tiled path); glupload imports it via EGL.
//   This is what src/capturable/pipewire.rs implements. vapostproc was a dead end (discovery 2).
//
// Intel DRM modifier reference (fourcc_mod_code(INTEL=0x01, n)):
//   LINEAR = bare fourcc (mod 0)   X_TILED = 0x..01   Y_TILED = 0x..02
//   RC_CCS = 0x..06 (multi-plane)  MC_CCS  = 0x..08 (multi-plane)  <- CCS ones break single-plane EGL import

#[path = "../src/capturable/remote_desktop_dbus.rs"]
mod remote_desktop_dbus;

use std::collections::HashMap;
use std::error::Error;
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dbus::{
    arg::{OwnedFd, PropMap, RefArg, Variant},
    blocking::{Proxy, SyncConnection},
    message::{MatchRule, MessageType},
    Message,
};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSink;

use remote_desktop_dbus::{
    OrgFreedesktopPortalRemoteDesktop, OrgFreedesktopPortalRequestResponse,
    OrgFreedesktopPortalScreenCast,
};

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
// Pipeline construction — the part under investigation.
// ------------------------------------------------------------------------------------------------

// A single test configuration: an optional capsfilter after pipewiresrc (`caps`,
// empty = none) followed by a converter chain selected by `mode`.
struct TestCfg {
    label: &'static str,
    mode: &'static str, // "convert" | "glupload" | "vpp"
    caps: String,
}

// niri (monitor cast) offers ONLY XRGB8888 (XR24). Build a dmabuf capsfilter for XR24
// restricted to the given modifier(s). `mods` is a list of "XR24:<modifier>" fourcc:mod
// strings joined into a GstValueList.
fn xr24_caps(mods: &[&str]) -> String {
    let list = mods
        .iter()
        .map(|m| format!("(string){m}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "video/x-raw(memory:DMABuf), format=(string)DMA_DRM, width=(int)[1,32767], \
         height=(int)[1,32767], framerate=(fraction)[0/1,2147483647/1], \
         drm-format=(list){{ {list} }}"
    )
}

fn build_pipeline(
    fd: i32,
    path: u64,
    cfg: &TestCfg,
) -> Result<(gst::Pipeline, AppSink), Box<dyn Error>> {
    let pipeline = gst::Pipeline::new();

    let src = gst::ElementFactory::make("pipewiresrc").build()?;
    src.set_property("fd", &fd);
    src.set_property("path", &format!("{}", path));
    let always_copy = std::env::var("PROBE_ALWAYS_COPY").map(|v| v == "1" || v == "true").unwrap_or(false);
    src.set_property("always-copy", &always_copy);

    let sink = gst::ElementFactory::make("appsink").build()?;
    sink.set_property("drop", &true);
    sink.set_property("max-buffers", &1u32);

    let make = |n: &str| gst::ElementFactory::make(n).build();

    let mut elements: Vec<gst::Element> = vec![src.clone()];
    if !cfg.caps.is_empty() {
        let cf = make("capsfilter")?;
        cf.set_property("caps", &cfg.caps.parse::<gst::Caps>()?);
        elements.push(cf);
    }
    match cfg.mode {
        "glupload" => {
            elements.push(make("glupload")?);
            elements.push(make("glcolorconvert")?);
            elements.push(make("gldownload")?);
            elements.push(make("videoconvert")?);
        }
        "vpp" => {
            elements.push(make("vapostproc")?);
            elements.push(make("videoconvert")?);
        }
        // "dmabuf": no converter at all — appsink receives the dmabuf directly and we
        // mmap it ourselves (works only for LINEAR: a tiled/CCS buffer maps to garbage).
        "dmabuf" => {}
        // "convert": rely on videoconvert to consume the (dmabuf) buffer directly
        _ => {
            elements.push(make("videoconvert")?);
        }
    }
    elements.push(sink.clone());

    pipeline.add_many(&elements)?;
    for w in elements.windows(2) {
        w[0].link(&w[1]).map_err(|e| {
            DBusError(format!("link {} -> {} failed: {e}", w[0].name(), w[1].name()))
        })?;
    }

    let appsink = sink
        .dynamic_cast::<AppSink>()
        .map_err(|_| DBusError("sink is not an appsink".into()))?;
    if cfg.mode == "dmabuf" {
        // Accept the dmabuf straight through; format stays DMA_DRM in caps.
        let c: gst::Caps = "video/x-raw(memory:DMABuf)".parse()?;
        appsink.set_caps(Some(&c));
    } else {
        let mut caps = gst::Caps::new_empty();
        caps.merge_structure(gst::structure::Structure::from_iter(
            "video/x-raw",
            [("format", "BGRx".into())],
        ));
        caps.merge_structure(gst::structure::Structure::from_iter(
            "video/x-raw",
            [("format", "RGBx".into())],
        ));
        appsink.set_caps(Some(&caps));
    }

    Ok((pipeline, appsink))
}

// Build+run one config against an already-open portal fd/node. Returns a short verdict.
fn run_one(fd: i32, path: u64, cfg: &TestCfg) -> String {
    eprintln!("\n================ TEST: {} (mode={}) ================", cfg.label, cfg.mode);
    if !cfg.caps.is_empty() {
        eprintln!("  capsfilter: {}", cfg.caps);
    }
    let (pipeline, appsink) = match build_pipeline(fd, path, cfg) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("  build/link error: {e}");
            return format!("{}: BUILD-FAIL ({e})", cfg.label);
        }
    };

    let start = pipeline.set_state(gst::State::Playing);
    let failed = match start {
        Err(_) => true,
        Ok(gst::StateChangeSuccess::Async) => {
            pipeline.state(gst::ClockTime::from_seconds(4)).0.is_err()
        }
        Ok(_) => false,
    };

    if failed {
        let err = drain_bus_errors(&pipeline);
        eprintln!("  FAILED to start:\n  {err}");
        let _ = pipeline.set_state(gst::State::Null);
        let short = err.lines().next().unwrap_or("").to_string();
        return format!("{}: START-FAIL ({short})", cfg.label);
    }

    let mut negotiated = String::from("?");
    if let Some(srcel) = pipeline.by_name("pipewiresrc0") {
        if let Some(pad) = srcel.static_pad("src") {
            if let Some(c) = pad.current_caps() {
                negotiated = c.to_string();
            }
        }
    }
    eprintln!("  negotiated (pipewiresrc src): {negotiated}");

    let mut got = 0;
    let mut sample_info = String::new();
    for _ in 0..40 {
        if let Some(sample) = appsink.try_pull_sample(gst::ClockTime::from_mseconds(100)) {
            let caps = sample.caps().unwrap();
            let st = caps.structure(0).unwrap();
            let w: i32 = st.value("width").ok().and_then(|v| v.get().ok()).unwrap_or(-1);
            let h: i32 = st.value("height").ok().and_then(|v| v.get().ok()).unwrap_or(-1);
            let fmt: String = st.value("format").ok().and_then(|v| v.get().ok()).unwrap_or_else(|| "?".into());
            let drm: String = st.value("drm-format").ok().and_then(|v| v.get().ok()).unwrap_or_else(|| "-".into());
            let expect = w as usize * h as usize * 4;
            // Actually try to CPU-map the buffer (this is what weylus does).
            let buf = sample.buffer_owned().unwrap();
            let bufsz = buf.size();
            // Introspect memory layout.
            let nmem = buf.n_memory();
            let mut mem_desc = Vec::new();
            for mi in 0..nmem {
                if let Some(m) = buf.memory(mi) {
                    mem_desc.push(format!("mem{mi}:size={}", m.size()));
                }
            }
            eprintln!("  buffer: n_memory={nmem} [{}]", mem_desc.join(", "));
            let map_report = match buf.into_mapped_buffer_readable() {
                Ok(mapped) => {
                    let s = mapped.as_slice();
                    let head: Vec<String> = s.iter().take(12).map(|b| format!("{b:02x}")).collect();
                    // crude "is it real pixels" check: not all identical
                    let all_same = s.iter().take(4096).all(|&b| b == s[0]);
                    format!(
                        "MAPPED {} bytes, head=[{}]{}",
                        s.len(),
                        head.join(" "),
                        if all_same { " (looks uniform/blank)" } else { " (varied -> real pixels)" }
                    )
                }
                Err(_) => "MAP FAILED (not CPU-accessible)".to_string(),
            };
            sample_info = format!("{w}x{h} fmt={fmt} drm={drm} buf={bufsz} (expect {expect}); {map_report}");
            eprintln!("  frame: {sample_info}");
            got += 1;
            if got >= 3 {
                break;
            }
        }
    }

    let _ = pipeline.set_state(gst::State::Null);
    // Give pipewiresrc a moment to fully release the node before the next test reconnects.
    std::thread::sleep(Duration::from_millis(300));

    if got > 0 {
        eprintln!("  ==> SUCCESS ({got} frames)");
        format!("{}: OK ({sample_info})", cfg.label)
    } else {
        let err = drain_bus_errors(&pipeline);
        eprintln!("  ==> started but NO frames. {err}");
        format!("{}: NO-FRAMES", cfg.label)
    }
}

fn drain_bus_errors(pipeline: &gst::Pipeline) -> String {
    let bus = match pipeline.bus() {
        Some(b) => b,
        None => return "no bus".into(),
    };
    let mut msgs = Vec::new();
    while let Some(msg) =
        bus.timed_pop_filtered(gst::ClockTime::from_mseconds(800), &[gst::MessageType::Error])
    {
        if let gst::MessageView::Error(err) = msg.view() {
            let src = err
                .src()
                .map(|s| s.path_string().to_string())
                .unwrap_or_else(|| "?".into());
            msgs.push(format!(
                "[{src}] {} (debug: {})",
                err.error(),
                err.debug().unwrap_or_else(|| "none".into())
            ));
        }
    }
    if msgs.is_empty() {
        "no error message on bus".into()
    } else {
        msgs.join("\n  ")
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    gst::init()?;

    // EXACT caps weylus now uses (single-plane, non-CCS; XR24 for monitor, AR24 for window).
    let weylus_caps = "video/x-raw(memory:DMABuf), format=(string)DMA_DRM, \
         width=(int)[1,32767], height=(int)[1,32767], \
         framerate=(fraction)[0/1,2147483647/1], drm-format=(list){ \
         (string)XR24, (string)XR24:0x0100000000000001, (string)XR24:0x0100000000000002, \
         (string)AR24, (string)AR24:0x0100000000000001, (string)AR24:0x0100000000000002 }"
        .to_string();

    // Validate the exact weylus pipeline (one DP-1 click).
    let matrix = vec![
        TestCfg { label: "WEYLUS caps -> glupload chain", mode: "glupload", caps: weylus_caps },
    ];

    let (conn, fd, streams) = request_remote_desktop(false)?;
    eprintln!("[portal] got fd={} and {} stream(s):", fd.as_raw_fd(), streams.len());
    for s in &streams {
        eprintln!("    node path={} source_type={}", s.path, s.source_type);
    }
    let stream = streams
        .iter()
        .find(|s| s.source_type == 1)
        .copied()
        .unwrap_or(streams[0]);
    eprintln!("[probe] using node path={}", stream.path);

    // Allow running a single named test via PROBE_ONLY=<substring> for focused reruns.
    let only = std::env::var("PROBE_ONLY").ok();

    let mut verdicts = Vec::new();
    for cfg in &matrix {
        if let Some(f) = &only {
            if !cfg.label.contains(f.as_str()) {
                continue;
            }
        }
        verdicts.push(run_one(fd.as_raw_fd(), stream.path, cfg));
    }

    eprintln!("\n\n================ SUMMARY ================");
    for v in &verdicts {
        eprintln!("  {v}");
    }

    drop(conn);
    Ok(())
}
