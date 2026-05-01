//! Unix domain socket server.
//!
//! Listens on `/tmp/yabai-id.socket` (and the legacy YabaiIndicator path) for
//! refresh commands sent from yabai signal handlers in `.yabairc`.
//!
//! Accepted messages (same as YabaiIndicator):
//!   "refresh"           – full refresh
//!   "refresh spaces"    – spaces only
//!   "refresh windows"   – windows (currently treated same as full refresh)
//!
//! Usage from .yabairc:
//!   yabai -m signal --add event=space_changed \
//!       action='echo "refresh" | nc -U /tmp/yabai-id.socket'

use std::fs;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::thread;

pub const SOCKET_PATH: &str = "/tmp/yabai-id.socket";
pub const LEGACY_SOCKET_PATH: &str = "/tmp/yabai-indicator.socket";

/// Spawn a background thread that accepts connections on `path` and calls
/// `on_refresh` on each valid message.  `on_refresh` must be `Send + 'static`
/// and will be called from the socket thread – callers are responsible for
/// dispatching any UI work to the main thread.
pub fn start(path: &'static str, on_refresh: impl Fn() + Send + 'static) {
    let _ = fs::remove_file(path);

    thread::Builder::new()
        .name(format!("yabai-id-socket:{path}"))
        .spawn(move || {
            let listener = match UnixListener::bind(path) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("socket bind {path}: {e}");
                    return;
                }
            };

            for mut stream in listener.incoming().flatten() {
                let mut msg = String::new();
                let _ = stream.read_to_string(&mut msg);
                match msg.trim() {
                    "refresh" | "refresh spaces" | "refresh windows" => on_refresh(),
                    _ => {}
                }
            }
        })
        .expect("failed to spawn socket thread");
}
