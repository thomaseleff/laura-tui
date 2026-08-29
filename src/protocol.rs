//! Per-tab local socket: typed messages, NDJSON framing, bind/connect.
//!
//! `LAURA_TAB` holds a namespaced socket name, one per tab; a producer reaches a tab by connecting to it. Scoping is a consequence of addressing, not a security boundary.

use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{self, Receiver};

use anyhow::Result;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, prelude::*};
use serde::{Deserialize, Serialize};

/// A single mutation sent to a tab. Internally tagged so the JSON is a stable wire contract (`{"type":"open","path":"…"}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
    Open {
        path: String,
        /// Move focus into the panel on open. Defaults true so an older `{"type":"open"}` still focuses.
        #[serde(default = "default_true")]
        focus: bool,
    },
    /// Remove the tab's panel.
    Close,
    /// Mark the tab as hosting an agent; enables review submission.
    Ready,
    /// Reserved re-render nudge; not yet emitted.
    Update { path: String },
}

fn default_true() -> bool {
    true
}

/// Client: connect to `tab`, write one NDJSON frame, drop (EOF ends the frame).
pub fn send(tab: &str, msg: &Message) -> Result<()> {
    let name = tab.to_ns_name::<GenericNamespaced>()?;
    let mut conn = Stream::connect(name)?;
    let mut line = serde_json::to_string(msg)?;
    line.push('\n');
    conn.write_all(line.as_bytes())?;
    Ok(())
}

/// Server: bind `tab` synchronously (bind errors surface here), then accept on a background thread, forwarding each decoded `Message` on the returned channel.
pub fn serve(tab: &str) -> Result<Receiver<Message>> {
    let name = tab.to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new().name(name).create_sync()?;
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            for line in BufReader::new(conn).lines().map_while(Result::ok) {
                if let Ok(msg) = serde_json::from_str::<Message>(&line)
                    && tx.send(msg).is_err()
                {
                    return; // receiver dropped
                }
            }
        }
    });

    Ok(rx)
}
