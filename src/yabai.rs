//! Yabai IPC client.
//!
//! Communicates with the running yabai daemon via its Unix domain socket at
//! `/tmp/yabai_$USER.socket` using yabai's own wire protocol.

use std::env;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use serde::Deserialize;

const FAILURE_BYTE: u8 = 0x07;

// ── Wire protocol ─────────────────────────────────────────────────────────────

/// Build and send a yabai message, return the raw response string.
///
/// Wire format: [4-byte LE message_length][arg1 NUL][arg2 NUL]...[extra NUL]
pub fn call(args: &[&str]) -> Result<String, String> {
    if args.is_empty() {
        return Err("no yabai arguments provided".into());
    }

    // Build payload: null-terminated args followed by an extra NUL
    let mut payload: Vec<u8> = Vec::new();
    for arg in args {
        payload.extend_from_slice(arg.as_bytes());
        payload.push(0);
    }
    payload.push(0);

    let message_len = i32::try_from(payload.len())
        .map_err(|_| "yabai message payload too large".to_string())?;

    let mut message = message_len.to_ne_bytes().to_vec();
    message.extend_from_slice(&payload);

    let socket_path = socket_path()?;
    let mut stream = UnixStream::connect(&socket_path)
        .map_err(|e| format!("connect {socket_path}: {e}"))?;

    stream
        .write_all(&message)
        .map_err(|e| format!("send: {e}"))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("shutdown write: {e}"))?;

    let mut response: Vec<u8> = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| format!("read: {e}"))?;

    if response.first() == Some(&FAILURE_BYTE) {
        let msg = String::from_utf8_lossy(&response[1..]).trim().to_string();
        return Err(if msg.is_empty() {
            "yabai returned an error".into()
        } else {
            msg
        });
    }

    Ok(String::from_utf8_lossy(&response).to_string())
}

fn socket_path() -> Result<String, String> {
    let user = env::var("USER").map_err(|_| "env USER not set".to_string())?;
    Ok(format!("/tmp/yabai_{user}.socket"))
}

// ── Data model ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // fields reserved for future use
pub struct Space {
    pub index: i64,
    pub display: i64,
    #[serde(default)]
    pub label: String,
    #[serde(rename = "has-focus", default)]
    pub has_focus: bool,
    #[serde(rename = "is-visible", default)]
    pub is_visible: bool,
    #[serde(default)]
    pub windows: Vec<u64>,
    #[serde(rename = "type", default)]
    pub kind: SpaceKind,
}

impl Space {
    #[allow(dead_code)]
    pub fn display_label(&self) -> String {
        if !self.label.is_empty() {
            self.label.clone()
        } else {
            self.index.to_string()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpaceKind {
    #[default]
    Bsp,
    Float,
    Stack,
}

// ── Queries ───────────────────────────────────────────────────────────────────

/// Return all spaces, or an empty vec on error.
#[allow(dead_code)]
pub fn query_spaces() -> Vec<Space> {
    match call(&["query", "--spaces"]) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(e) => {
            eprintln!("yabai query --spaces: {e}");
            Vec::new()
        }
    }
}

/// Tell yabai to focus `space_index`.
pub fn focus_space(index: i64) {
    if let Err(e) = call(&["space", "--focus", &index.to_string()]) {
        eprintln!("yabai space --focus {index}: {e}");
    }
}
