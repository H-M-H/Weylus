//! Keep the host's screen on while a client is connected.
//!
//! On Windows an idle desktop turns its display off, and a dark display gives the
//! desktop duplication nothing to capture; if a lock follows, the session cannot be
//! reached at all. Remote-desktop hosts hold a "display required" request while a
//! client is connected, and so does this: the guard lives on the client's own
//! thread, is released when the client leaves, and changes no setting on the
//! machine. Elsewhere it is a no-op.

#[cfg(target_os = "windows")]
mod imp {
    use tracing::{info, warn};
    use winapi::um::winbase::SetThreadExecutionState;
    use winapi::um::winnt::{ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED};
    use winapi::um::winuser::{SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT};

    pub struct KeepAwake;

    impl KeepAwake {
        /// Call on the thread that serves the client; drop it on that same thread.
        pub fn new() -> Self {
            let prev = unsafe {
                SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED)
            };
            if prev == 0 {
                warn!("Keep awake: SetThreadExecutionState failed; the display may still turn off.");
            } else {
                info!("Keep awake: display held on while this client is connected.");
            }
            wake_display();
            KeepAwake
        }
    }

    impl Drop for KeepAwake {
        fn drop(&mut self) {
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
            info!("Keep awake: client gone, the display may turn off again.");
        }
    }

    /// A display that is already dark does not come back for the request alone;
    /// a pointer move does. One pixel right and one back leaves the cursor where it was.
    fn wake_display() {
        let mut sent = 0;
        for dx in [1, -1] {
            let mut input: INPUT = unsafe { std::mem::zeroed() };
            input.type_ = INPUT_MOUSE;
            unsafe {
                *input.u.mi_mut() = MOUSEINPUT {
                    dx,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE,
                    time: 0,
                    dwExtraInfo: 0,
                };
                sent += SendInput(1, &mut input, std::mem::size_of::<INPUT>() as i32);
            }
        }
        if sent == 2 {
            info!("Keep awake: wake nudge sent.");
        } else {
            warn!("Keep awake: wake nudge sent {sent} of 2 moves (a locked desktop refuses input).");
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    pub struct KeepAwake;

    impl KeepAwake {
        pub fn new() -> Self {
            KeepAwake
        }
    }
}

pub use imp::KeepAwake;
