use std::collections::HashMap;
use std::error::Error;
use std::os::raw::c_int;
use std::os::unix::io::AsRawFd;
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::time::Duration;
use tracing::{debug, warn};

use dbus::{
    arg::{OwnedFd, PropMap, RefArg, Variant},
    blocking::{Connection, Proxy},
    message::{MatchRule, MessageType},
    Message,
};

use crate::capturable::pipewire_dbus::{
    OrgFreedesktopPortalRequestResponse, OrgFreedesktopPortalScreenCast,
};

extern "C" {
    fn init_pipewire(fd: c_int);
}

#[derive(Debug, Clone, Copy)]
pub struct PwStream {
    id: u64,
    source_type: u64,
}

#[derive(Debug)]
pub struct DBusError(String);

impl std::fmt::Display for DBusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl Error for DBusError {}

fn handle_response<F>(
    conn: &Connection,
    path: dbus::Path<'static>,
    mut f: F,
    failure_out: Arc<AtomicBool>,
) -> Result<dbus::channel::Token, dbus::Error>
where
    F: FnMut(
            OrgFreedesktopPortalRequestResponse,
            &Connection,
            &Message,
        ) -> Result<(), Box<dyn Error>>
        + Send
        + 'static,
{
    let mut m = MatchRule::new();
    m.path = Some(path);
    m.msg_type = Some(MessageType::Signal);
    m.sender = Some("org.freedesktop.portal.Desktop".into());
    m.interface = Some("org.freedesktop.portal.Request".into());
    conn.add_match(m, move |r: OrgFreedesktopPortalRequestResponse, c, m| {
        debug!("Response from DBus: response: {:?}, message: {:?}", r, m);
        match r.response {
            0 => {}
            1 => {
                warn!("DBus response: User cancelled interaction.");
                failure_out.store(true, std::sync::atomic::Ordering::Relaxed);
                return true;
            }
            c => {
                warn!("DBus response: Unknown error, code: {}.", c);
                failure_out.store(true, std::sync::atomic::Ordering::Relaxed);
                return true;
            }
        }
        if let Err(err) = f(r, c, m) {
            warn!("Error requesting screen capture via dbus: {}", err);
            failure_out.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        true
    })
}

fn get_portal(conn: &Connection) -> Proxy<&Connection> {
    conn.with_proxy(
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        Duration::from_millis(1000),
    )
}

fn streams_from_response(response: OrgFreedesktopPortalRequestResponse) -> Vec<PwStream> {
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
                    let id = itr.next()?.as_u64()?;
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
                    Some(PwStream {
                        id,
                        source_type: attributes.get("source_type")?.as_u64()?,
                    })
                })
                .collect::<Vec<PwStream>>(),
        )
    })()
    .unwrap_or_default()
}

// mostly inspired by https://gitlab.gnome.org/snippets/19
pub fn request_screen_cast() -> Result<(OwnedFd, Vec<PwStream>), Box<dyn Error>> {
    let conn = Connection::new_session()?;
    let portal = get_portal(&conn);
    let mut args: PropMap = HashMap::new();
    let fd: Arc<Mutex<Option<OwnedFd>>> = Arc::new(Mutex::new(None));
    let fd_res = fd.clone();
    let streams: Arc<Mutex<Vec<PwStream>>> = Arc::new(Mutex::new(Vec::new()));
    let streams_res = streams.clone();
    let failure = Arc::new(AtomicBool::new(false));
    let failure_res = failure.clone();
    args.insert(
        "session_handle_token".to_string(),
        Variant(Box::new("u1".to_string())),
    );
    args.insert(
        "handle_token".to_string(),
        Variant(Box::new("u1".to_string())),
    );
    let path = portal.create_session(args)?;
    handle_response(
        &conn,
        path,
        move |r: OrgFreedesktopPortalRequestResponse, c, _| {
            let portal = get_portal(c);
            let mut args: PropMap = HashMap::new();
            args.insert(
                "handle_token".to_string(),
                Variant(Box::new("u2".to_string())),
            );
            args.insert("multiple".into(), Variant(Box::new(true)));
            args.insert("types".into(), Variant(Box::new(1u32 | 2u32)));
            let session: dbus::Path = r
                .results
                .get("session_handle")
                .ok_or_else(|| {
                    DBusError(format!(
                        "Failed to obtain session_handle from response: {:?}",
                        r
                    ))
                })?
                .as_str()
                .ok_or_else(|| DBusError("Failed to convert session_handle to string.".into()))?
                .to_string()
                .into();
            let path = portal.select_sources(session.clone(), args)?;
            let fd = fd.clone();
            let streams = streams.clone();
            let failure = failure.clone();
            let failure_out = failure.clone();
            handle_response(
                c,
                path,
                move |_: OrgFreedesktopPortalRequestResponse, c, _| {
                    let portal = get_portal(c);
                    let mut args: PropMap = HashMap::new();
                    args.insert(
                        "handle_token".to_string(),
                        Variant(Box::new("u3".to_string())),
                    );
                    let path = portal.start(session.clone(), "", args)?;
                    let session = session.clone();
                    let fd = fd.clone();
                    let streams = streams.clone();
                    let failure = failure.clone();
                    let failure_out = failure.clone();
                    handle_response(
                        c,
                        path,
                        move |r: OrgFreedesktopPortalRequestResponse, c, _| {
                            streams
                                .clone()
                                .lock()
                                .unwrap()
                                .append(&mut streams_from_response(r));
                            let portal = get_portal(c);
                            fd.clone().lock().unwrap().replace(
                                portal.open_pipe_wire_remote(session.clone(), HashMap::new())?,
                            );
                            Ok(())
                        },
                        failure_out,
                    )?;
                    Ok(())
                },
                failure_out,
            )?;
            Ok(())
        },
        failure_res.clone(),
    )?;
    // wait 3 minutes for user interaction
    for _ in 0..1800 {
        conn.process(Duration::from_millis(100))?;
        // Once we got a file descriptor we are done!
        if fd_res.lock().unwrap().is_some() {
            break;
        }

        if failure_res.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
    }
    let fd_res = fd_res.lock().unwrap();
    let streams_res = streams_res.lock().unwrap();
    if fd_res.is_some() && !streams_res.is_empty() {
        Ok((fd_res.clone().unwrap(), streams_res.clone()))
    } else {
        Err(Box::new(DBusError(
            "Failed to obtain screen capture.".into(),
        )))
    }
}

pub fn get_capturables() {
    let res = crate::capturable::pipewire::request_screen_cast();
    warn!("Res: {:?}", res);
    if let Ok((fd, streams)) = res {
        unsafe { init_pipewire(fd.as_raw_fd()) }
    }
}
