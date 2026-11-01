//! Browser panes (experimental, #107): Laura's own headless Chrome or Edge (Laura's browser)
//! renders a URL, and its screencast streams to the web app. No other module talks CDP.
//!
//! Laura's loop runs the whole workspace on one thread, so nothing here waits on the page except
//! `Page::open`'s setup calls, and `Page::pick` and `Page::find`: `Browser::launch` waits for
//! Chrome on its own thread, and every other call goes out through `Cdp::send`, and its reply
//! comes back through `Page::poll` like an event.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

/// The error when `LAURA_EXPERIMENTAL_BROWSER` isn't set.
pub const FLAG_OFF: &str =
    "browser panes are experimental: set LAURA_EXPERIMENTAL_BROWSER=1 before starting Laura";
/// The error for a URL outside `laura --web`.
pub const NEED_WEB: &str = "browser panes need the web app: start Laura with --web";
/// The error when Laura finds no browser, or `LAURA_BROWSER` names a missing file.
pub const NO_BROWSER: &str =
    "browser panes need Chrome or Edge: install one or set LAURA_BROWSER to its path";
/// The errors for a file pane's flag with a URL.
pub const NO_HIGHLIGHT: &str =
    "a browser pane has no lines: open the page, then run laura highlight '<selector>' --pane <id>";
pub const NO_DIFF: &str =
    "a browser pane has no diff: run laura open <file> --diff on the page's source file";
pub const NO_EDIT: &str =
    "a browser pane has no editor: run laura open <file> --edit on the page's source file";
/// The notice for an address-field URL that isn't http or https.
pub const ONLY_HTTP: &str = "⚠ Use an http or https URL in a browser pane.";
pub const NO_ELEMENT: &str = "⚠ Point at an element to comment on it.";
pub const DIALOG_OPEN: &str = "⚠ Close the page's dialog before you comment.";
pub const LOADING: &str = "⚠ When the page loads, try again.";
/// The notice when Laura's browser exits mid-session. It names the browser pane, because a narrow
/// browser pane sends it to the workspace's corner.
pub const EXITED: &str = "⚠ To start Laura's browser again, press Ctrl+R in a browser pane.";
/// `EXITED` for the agent, in the agent's error format.
pub const EXITED_AGENT: &str =
    "Laura's browser exited: press Ctrl+R in a browser pane to start it again";
/// The notice when Laura's browser doesn't start for the user. The cause is in the journal.
pub const START_FAILED: &str =
    "⚠ Laura's browser didn't start. To try again, press Ctrl+R in a browser pane.";

/// A first start on a CI runner took 7–30 s (cold disk, Defender's first scan). Later starts take
/// 0.1–0.3 s.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);
/// How long `Cdp::call` waits for a reply.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// At most one size per page this often, so a window drag doesn't restart the screencast each step.
const RESIZE_EVERY: Duration = Duration::from_millis(100);
/// How often Laura reads the page's title again: a script can set it with no event.
const TITLE_EVERY: Duration = Duration::from_secs(1);
/// A `fetch` or XHR that starts this soon after a click shows the spinner.
const CLICK_WINDOW: Duration = Duration::from_secs(1);
/// Input this soon after the user's input is the user's.
const USER_INPUT: Duration = Duration::from_millis(500);
/// A click that the page still handles after this long shows the spinner.
const CLICK_SLOW: Duration = Duration::from_millis(100);
/// A request that runs this long stops showing the spinner.
const REQUEST_MAX: Duration = Duration::from_secs(10);

/// A page's size: CSS px wide and high, and the view's device pixel ratio.
pub type Size = (u32, u32, f64);

/// The size of a page before its view reports one.
pub const DEFAULT_SIZE: Size = (800, 600, 1.0);

/// An `http://` or `https://` URL, any case. Any other scheme stays a file path.
pub fn is_url(s: &str) -> bool {
    ["http://", "https://"]
        .iter()
        .any(|p| s.get(..p.len()).is_some_and(|h| h.eq_ignore_ascii_case(p)))
}

/// The address field's text as a URL: an http(s) URL as is, else a bare host. The host gets `http://` when it is an IP address, a name with no dot (`localhost`), or a name that
/// ends in `.localhost`, `.local` or `.internal`, else `https://`. Any other scheme is an error, but `host:port` isn't.
pub fn address(s: &str) -> Result<String, String> {
    let s = s.trim();
    if is_url(s) {
        return Ok(s.to_string());
    }
    // Only the part before the path: a `:` in the path is no scheme.
    let authority = s.split(['/', '?', '#']).next().unwrap_or_default();
    let host = match authority.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => h,
        Some(_) if !authority.starts_with('[') => return Err(ONLY_HTTP.into()),
        _ => authority,
    };
    let ip = host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok();
    let lower = host.to_ascii_lowercase();
    let http = ip
        || !host.contains('.')
        || [".localhost", ".local", ".internal"]
            .iter()
            .any(|t| lower.ends_with(t));
    Ok(format!("{}://{s}", if http { "http" } else { "https" }))
}

/// The startup gate: the browser to run, or why browser panes are off. `LAURA_BROWSER` wins, with
/// no fallback, so a wrong path reads as an error instead of another browser.
pub fn resolve() -> Result<PathBuf, String> {
    let on = std::env::var("LAURA_EXPERIMENTAL_BROWSER").is_ok_and(|v| !v.is_empty() && v != "0");
    if !on {
        return Err(FLAG_OFF.into());
    }
    if let Some(p) = std::env::var_os("LAURA_BROWSER").filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(NO_BROWSER.into())
        };
    }
    candidates()
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| NO_BROWSER.into())
}

/// `LAURA_BROWSER_URL`: where the agent's browser tools reach Laura's browser on `port`.
pub fn endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// Chrome, Edge and Chromium where their installers put them, in that order.
fn candidates() -> Vec<PathBuf> {
    if cfg!(windows) {
        let rel = [
            r"Google\Chrome\Application\chrome.exe",
            r"Microsoft\Edge\Application\msedge.exe",
            r"Chromium\Application\chrome.exe",
        ];
        let dirs: Vec<_> = ["ProgramFiles", "ProgramFiles(x86)", "LocalAppData"]
            .into_iter()
            .filter_map(std::env::var_os)
            .collect();
        rel.iter()
            .flat_map(|r| dirs.iter().map(move |d| Path::new(d).join(r)))
            .collect()
    } else if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").unwrap_or_default();
        let dirs = ["/Applications".to_string(), format!("{home}/Applications")];
        ["Google Chrome", "Microsoft Edge", "Chromium"]
            .iter()
            .flat_map(|n| {
                dirs.iter()
                    .map(move |d| PathBuf::from(format!("{d}/{n}.app/Contents/MacOS/{n}")))
            })
            .collect()
    } else {
        let names = [
            "google-chrome",
            "google-chrome-stable",
            "microsoft-edge",
            "microsoft-edge-stable",
            "chromium",
            "chromium-browser",
        ];
        let dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        names
            .iter()
            .flat_map(|n| dirs.iter().map(move |d| d.join(n)))
            .collect()
    }
}

/// Laura's browser: one headless Chrome or Edge per workspace, with a temporary profile.
pub struct Browser {
    pub cdp: Cdp,
    /// The CDP port the OS picks. The agent reaches Chrome through the proxy at
    /// `LAURA_BROWSER_URL` (`cdp_proxy`).
    pub port: u16,
    /// The browser-level CDP websocket, for a second client in the tests.
    pub ws_url: String,
    /// Chrome's start tab, which `close_start` closes once a pane's page opens, so the agent's
    /// tools list only the panes' pages.
    start: Vec<String>,
    proc: Proc,
}

impl Browser {
    /// Start `path` headless on a fresh profile with CDP on a port the OS picks, and connect to
    /// it on another thread. The receiver gets Laura's browser, or why it didn't start.
    pub fn launch(path: &Path) -> Start {
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        match Browser::spawn(path) {
            Ok(proc) => {
                let (path, cancel) = (path.to_path_buf(), cancel.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(Browser::connect(proc, &path, &cancel));
                });
            }
            Err(reason) => {
                let _ = tx.send(Err(StartError {
                    reason,
                    output: String::new(),
                }));
            }
        }
        Start { rx, cancel }
    }

    /// Start `path`'s process on a fresh profile.
    fn spawn(path: &Path) -> Result<Proc> {
        let profile = std::env::temp_dir().join(format!("laura-browser-{}", crate::web::nonce(8)?));
        std::fs::create_dir_all(&profile)
            .with_context(|| format!("couldn't create {}", profile.display()))?;
        // Chrome's own output, for the journal when the start fails.
        let stderr = match std::fs::File::create(profile.join("chrome-stderr.log")) {
            Ok(f) => f,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&profile);
                return Err(anyhow!("couldn't create {}: {e}", profile.display()));
            }
        };
        let mut cmd = Command::new(path);
        // Density 3, so each page can emulate any lower one at full sharpness; no smooth
        // scrolling, which the viewing browser already does. The temporary profile's background
        // work (updates, sync, Translate) only costs CPU and network.
        cmd.args([
            "--headless=new",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-sync",
            "--disable-extensions",
            "--disable-default-apps",
            "--disable-breakpad",
            "--disable-client-side-phishing-detection",
            "--disable-features=Translate,OptimizationHints,MediaRouter",
            "--force-device-scale-factor=3",
            "--disable-smooth-scrolling",
        ])
        .arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", profile.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr);
        // No keyring prompt or D-Bus wait for a profile that Laura deletes on exit.
        #[cfg(target_os = "linux")]
        cmd.arg("--password-store=basic");
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&profile);
                return Err(anyhow!("couldn't start {}: {e}", path.display()));
            }
        };
        // Only Laura holds the write end of the watcher's stdin, so Laura's death, however it
        // happens, closes the pipe: `read` returns and the watcher ends Chrome and its profile.
        // The trap keeps a terminal hangup from ending the watcher before it ends Chrome.
        #[cfg(unix)]
        let watcher = match Command::new("/bin/sh")
            .args([
                "-c",
                "trap '' HUP INT TERM; read _; kill -9 \"$1\" 2>/dev/null; rm -rf \"$2\"",
                "sh",
            ])
            .arg(child.id().to_string())
            .arg(&profile)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(w) => w,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_dir_all(&profile);
                return Err(anyhow!(
                    "couldn't start the watcher for Laura's browser: {e}"
                ));
            }
        };
        // `cmd` holds a copy of the log's handle, which on Windows would keep `Proc`'s drop from
        // deleting the profile.
        drop(cmd);
        // From here, dropping `proc` ends the process and deletes the profile.
        Ok(Proc {
            #[cfg(windows)]
            job: kill_on_close(&child),
            #[cfg(unix)]
            watcher,
            child,
            profile,
        })
    }

    /// Wait for `proc` to listen, then connect. A failure carries Chrome's output, read before
    /// `proc` drops and deletes the profile.
    fn connect(mut proc: Proc, path: &Path, cancel: &AtomicBool) -> Result<Browser, StartError> {
        match Browser::ready(&mut proc, path, cancel) {
            Ok((cdp, port, ws_url, start)) => Ok(Browser {
                cdp,
                port,
                ws_url,
                start,
                proc,
            }),
            Err(reason) => Err(StartError {
                output: chrome_output(&proc.profile),
                reason,
            }),
        }
    }

    /// `proc`'s CDP client, port, websocket URL and start tab, once it listens.
    fn ready(
        proc: &mut Proc,
        path: &Path,
        cancel: &AtomicBool,
    ) -> Result<(Cdp, u16, String, Vec<String>)> {
        let file = proc.profile.join("DevToolsActivePort");
        let t0 = Instant::now();
        let (port, ws_path) = loop {
            // Two lines: the port, then the browser's websocket path.
            if let Ok(s) = std::fs::read_to_string(&file) {
                let mut lines = s.lines();
                if let (Some(Ok(port)), Some(ws)) =
                    (lines.next().map(|p| p.trim().parse::<u16>()), lines.next())
                {
                    break (port, ws.trim().to_string());
                }
            }
            if cancel.load(Ordering::SeqCst) {
                bail!("Laura quit as Laura's browser started");
            }
            if let Ok(Some(status)) = proc.child.try_wait() {
                bail!("{} exited as it started ({status})", path.display());
            }
            if t0.elapsed() > LAUNCH_TIMEOUT {
                bail!(
                    "{} didn't start within {} s",
                    path.display(),
                    LAUNCH_TIMEOUT.as_secs()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let ws_url = format!("ws://127.0.0.1:{port}{ws_path}");
        let cdp = Cdp::connect(&ws_url, port)?;
        // Title and URL changes for every page, routed by target id.
        cdp.send("Target.setDiscoverTargets", json!({"discover": true}), None);
        // A page can't save files to the user's Downloads folder. Headless Chrome already denies
        // each permission request (`--deny-permission-prompts` changes nothing). The events still
        // name each denied download, for the notice.
        cdp.send(
            "Browser.setDownloadBehavior",
            json!({"behavior": "deny", "eventsEnabled": true}),
            None,
        );
        let start = cdp.call("Target.getTargets", json!({}), None)?["targetInfos"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| t["type"] == "page")
            .filter_map(|t| t["targetId"].as_str().map(str::to_string))
            .collect();
        Ok((cdp, port, ws_url, start))
    }

    /// Close Chrome's start tab. Laura calls it after the first page opens, so Laura's browser
    /// always has a tab.
    pub fn close_start(&mut self) {
        for id in self.start.drain(..) {
            self.cdp
                .send("Target.closeTarget", json!({"targetId": id}), None);
        }
    }

    /// The temporary profile, which Laura deletes when Laura's browser exits.
    pub fn profile(&self) -> &Path {
        &self.proc.profile
    }
}

impl Drop for Browser {
    /// Ask Laura's browser to close, so it shuts down cleanly; `proc` kills it after 2 s, or at
    /// once if the CDP socket is closed and Laura's browser can't hear the ask.
    fn drop(&mut self) {
        if self.cdp.closed() {
            return;
        }
        self.cdp.send("Browser.close", json!({}), None);
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(2)
            && matches!(self.proc.child.try_wait(), Ok(None))
        {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// A GET of Chrome's JSON at `path` on `port`, such as `/json/list`, as the agent's browser tools
/// read it. `None` until Chrome answers.
pub fn get_json(port: u16, path: &str) -> Option<Value> {
    let (_, body) = http(port, "GET", path)?;
    serde_json::from_str(&body).ok()
}

/// A `method` request to Chrome's HTTP endpoint at `path` on `port`: the status, such as `200 OK`,
/// and the body. `None` until Chrome answers.
pub fn http(port: u16, method: &str, path: &str) -> Option<(String, String)> {
    use std::io::{Read, Write};
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
    )
    .ok()?;
    // Chrome keeps the connection open, so the body ends at its `Content-Length`.
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 4096]);
    loop {
        let n = s.read(&mut chunk).ok().filter(|n| *n > 0)?;
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf);
        let Some((head, body)) = text.split_once("\r\n\r\n") else {
            continue;
        };
        let len = head
            .lines()
            .find_map(|l| {
                l.split_once(':')
                    .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
            })
            .and_then(|(_, v)| v.trim().parse::<usize>().ok())?;
        if body.len() >= len {
            let status = head.lines().next()?.split_once(' ')?.1;
            return Some((status.to_string(), body[..len].to_string()));
        }
    }
}

/// Laura's browser as it starts: `Browser::launch`'s thread sends it once Chrome listens, or why
/// it didn't.
pub struct Start {
    rx: Receiver<Result<Browser, StartError>>,
    cancel: Arc<AtomicBool>,
}

impl Start {
    /// The start's result, once it ends.
    pub fn poll(&self) -> Option<Result<Browser, StartError>> {
        match self.rx.try_recv() {
            Ok(res) => Some(res),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(StartError {
                reason: anyhow!("the start ended with no result"),
                output: String::new(),
            })),
        }
    }
}

impl Drop for Start {
    /// Laura quits as Laura's browser starts: stop the thread's wait, and wait while its `Proc`
    /// ends Chrome and deletes the profile, before Laura's process exits.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        let _ = self.rx.recv();
    }
}

/// Why Laura's browser didn't start, and Chrome's output, for the journal.
pub struct StartError {
    pub reason: anyhow::Error,
    pub output: String,
}

/// The last 10 lines of Chrome's stderr in `profile`. Chrome logs the `:ERROR:dbus/` lines at each
/// start on a host with no session bus, so they're left out.
fn chrome_output(profile: &Path) -> String {
    let bytes = std::fs::read(profile.join("chrome-stderr.log")).unwrap_or_default();
    let s = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = s
        .trim_end()
        .lines()
        .filter(|l| !l.contains(":ERROR:dbus/"))
        .collect();
    lines[lines.len().saturating_sub(10)..].join("\n")
}

/// Laura's browser's process and profile.
struct Proc {
    child: Child,
    profile: PathBuf,
    /// Closing it ends Laura's browser and its helper processes, even when Laura is killed.
    #[cfg(windows)]
    job: Option<std::os::windows::io::OwnedHandle>,
    /// Ends Laura's browser and deletes the profile when Laura is killed.
    #[cfg(unix)]
    watcher: Child,
}

impl Drop for Proc {
    fn drop(&mut self) {
        // First, so the watcher can't kill a reused pid once Chrome is reaped.
        #[cfg(unix)]
        {
            let _ = self.watcher.kill();
            let _ = self.watcher.wait();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        #[cfg(windows)]
        drop(self.job.take());
        // Windows keeps the profile's files locked until every helper process exits.
        for _ in 0..50 {
            if std::fs::remove_dir_all(&self.profile).is_ok() || !self.profile.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    }
}

/// A Job Object that kills `child` and its helpers when Laura exits, even on `Ctrl+C`.
#[cfg(windows)]
fn kill_on_close(child: &Child) -> Option<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    // SAFETY: Win32 calls on a handle Laura owns, with `info` alive across the call.
    unsafe {
        let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if raw.is_null() {
            return None;
        }
        let job = OwnedHandle::from_raw_handle(raw);
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            // addr_of!, not &raw: Entire Graph's Rust parser can't parse &raw yet (entire-graph#345).
            std::ptr::addr_of!(info).cast(),
            size_of_val(&info) as u32,
        ) != 0
            && AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) != 0;
        ok.then_some(job)
    }
}

/// Where the IO thread sends what arrives: a waiting call's reply by id, else a page's replies and
/// events by session id or target id.
#[derive(Default)]
struct Routes {
    replies: HashMap<u64, Sender<Value>>,
    pages: HashMap<String, Sender<Value>>,
}

/// A CDP client. One IO thread owns the socket; clones share it.
#[derive(Clone)]
pub struct Cdp {
    out: Sender<String>,
    next: Arc<AtomicU64>,
    routes: Arc<Mutex<Routes>>,
    closed: Arc<AtomicBool>,
}

impl Cdp {
    fn connect(url: &str, port: u16) -> Result<Cdp> {
        let stream = TcpStream::connect(("127.0.0.1", port))?;
        // A frame at density 3 is several MB of base64.
        let config = WebSocketConfig::default()
            .max_message_size(Some(256 << 20))
            .max_frame_size(Some(256 << 20));
        let (ws, _) = tungstenite::client::client_with_config(url, stream, Some(config))
            .map_err(|e| anyhow!("couldn't connect to Laura's browser: {e}"))?;
        // Short: the thread writes only between reads, and an ack waits for the next one.
        ws.get_ref()
            .set_read_timeout(Some(Duration::from_millis(4)))?;
        let (out, out_rx) = channel();
        let cdp = Cdp {
            out,
            next: Arc::new(AtomicU64::new(1)),
            routes: Arc::default(),
            closed: Arc::default(),
        };
        let (routes, closed) = (cdp.routes.clone(), cdp.closed.clone());
        std::thread::spawn(move || {
            let _ = io(ws, &out_rx, &routes);
            // Every waiting call's sender drops here, so `call` returns at once.
            let mut r = lock(&routes);
            closed.store(true, Ordering::SeqCst);
            *r = Routes::default();
        });
        Ok(cdp)
    }

    /// The socket closed: Laura's browser exited.
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Send `method` without waiting; returns its id. The reply reaches `session`'s page.
    pub fn send(&self, method: &str, params: Value, session: Option<&str>) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.write(id, method, params, session);
        id
    }

    /// Send `method` and wait up to 5 s for its result.
    /// ponytail: blocks Laura's loop; only `Page::open`, `Page::pick` and `Page::find` use it there
    /// (`Browser::launch`'s start tab waits on its own thread). Move to `send` plus a pending reply if a slow browser makes the
    /// workspace stutter.
    pub fn call(&self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        {
            let mut r = lock(&self.routes);
            if self.closed() {
                bail!("Laura's browser exited");
            }
            r.replies.insert(id, tx);
        }
        self.write(id, method, params, session);
        match rx.recv_timeout(CALL_TIMEOUT) {
            Ok(mut v) => match v.get("error") {
                Some(e) => bail!("{method}: {}", e["message"].as_str().unwrap_or("failed")),
                None => Ok(v["result"].take()),
            },
            Err(RecvTimeoutError::Timeout) => {
                lock(&self.routes).replies.remove(&id);
                bail!("{method}: Laura's browser didn't answer within 5 s")
            }
            Err(RecvTimeoutError::Disconnected) => bail!("Laura's browser exited"),
        }
    }

    fn write(&self, id: u64, method: &str, params: Value, session: Option<&str>) {
        let mut m = json!({"id": id, "method": method, "params": params});
        if let Some(s) = session {
            m["sessionId"] = s.into();
        }
        let _ = self.out.send(m.to_string());
    }

    fn route(&self, key: &str, tx: Sender<Value>) {
        lock(&self.routes).pages.insert(key.to_string(), tx);
    }

    fn unroute(&self, key: &str) {
        lock(&self.routes).pages.remove(key);
    }
}

fn lock(routes: &Mutex<Routes>) -> MutexGuard<'_, Routes> {
    routes.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The IO thread, as `web::view_conn`: a short polling read, then drain the outbound queue.
fn io(
    mut ws: WebSocket<TcpStream>,
    out: &Receiver<String>,
    routes: &Mutex<Routes>,
) -> tungstenite::Result<()> {
    loop {
        match ws.read() {
            Ok(Message::Text(t)) => {
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    route(routes, v);
                }
            }
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(e) if crate::web::nothing_yet(&e) => {}
            Err(e) => return Err(e),
        }
        loop {
            match out.try_recv() {
                Ok(s) => ws.write(Message::text(s))?,
                Err(TryRecvError::Empty) => break,
                // Every `Cdp` dropped.
                Err(TryRecvError::Disconnected) => {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    return Ok(());
                }
            }
        }
        ws.flush()?;
    }
}

fn route(routes: &Mutex<Routes>, v: Value) {
    let mut r = lock(routes);
    if let Some(tx) = v["id"].as_u64().and_then(|id| r.replies.remove(&id)) {
        let _ = tx.send(v);
        return;
    }
    let p = &v["params"];
    // A popup's target goes to the page that opened it. A download names its frame: a page's top
    // frame id is its target id.
    // shortcut: a download from a subframe reaches no page and shows no notice; map frame ids to
    // pages if users download from frames.
    let keys = [
        &v["sessionId"],
        &p["targetInfo"]["targetId"],
        &p["targetId"],
        &p["targetInfo"]["openerId"],
        &p["frameId"],
    ];
    if let Some(tx) = keys
        .iter()
        .filter_map(|k| k.as_str())
        .find_map(|k| r.pages.get(k))
    {
        let _ = tx.send(v);
    }
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MouseKind {
    Down,
    Up,
    Move,
    Wheel,
}

/// A pointer event on a browser pane's canvas, in CSS px from its top left. `button` is the DOM's
/// index, `buttons` its bitmask, `mods` CDP's (Alt 1, Ctrl 2, Meta 4, Shift 8).
#[derive(Deserialize)]
pub struct Mouse {
    pub kind: MouseKind,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub button: u8,
    #[serde(default)]
    pub buttons: u8,
    #[serde(default)]
    pub clicks: u8,
    #[serde(default)]
    pub dx: f64,
    #[serde(default)]
    pub dy: f64,
    #[serde(default)]
    pub mods: u8,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum KeyKind {
    Down,
    Up,
}

/// A keystroke for the page: the DOM's `key` and `code`, the text it types (empty for a key that
/// types none), and `key_code`, the Windows virtual key code that Chrome needs for Backspace and
/// the arrows.
#[derive(Deserialize)]
pub struct Key {
    pub kind: KeyKind,
    pub key: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub key_code: u32,
    #[serde(default)]
    pub mods: u8,
}

/// page.js's rule for an element that takes the keystrokes.
const TEXT: &str = "textarea, select, input:not([type=checkbox], [type=radio], [type=button], \
                    [type=submit], [type=reset], [type=image], [type=file], [type=color])";

/// Runs in each document before the page's own scripts. Laura's browser is headless, so a new tab
/// and a dropdown list's popup never reach the screencast: a link or `window.open` that opens a
/// new tab opens in the same pane, and a click on a dropdown list only focuses it, so `↑` and `↓`
/// change the choice. A middle-click's tab has no opener, so `Page::on_popup` can't catch it.
/// ponytail: the script runs in the page's world, `window.open` returns `null`, and a Ctrl+click
/// still opens a hidden tab; open new tabs as panes if users miss them.
const PAGE_JS: &str = "\
    addEventListener('click', e => { \
      const a = e.target.closest?.('a[target=_blank]'); if (a) a.target = '_self'; }, true); \
    addEventListener('auxclick', e => { \
      const a = e.button == 1 && e.target.closest?.('a[href]'); \
      if (a) { e.preventDefault(); location.assign(a.href); } }, true); \
    window.open = u => (u && location.assign(u), null); \
    addEventListener('mousedown', e => { \
      const s = e.target.closest?.('select'); \
      if (s && !s.multiple && s.size <= 1) { e.preventDefault(); s.focus(); } }, true);";

/// Finds the threads' elements in each document, in the isolated world `laura`.
const BROWSER_JS: &str = concat!("{\n", include_str!("../assets/web/browser.js"), "\n}");

/// What `Page::poll` reports to the tab.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// The top frame moved to this http(s) URL.
    Url(String),
    /// A `navigate` to `url` failed, such as `net::ERR_CONNECTION_REFUSED`.
    Failed { url: String, error: String },
    /// On `url`, the threads on these lines (1-based) have no element; the others have one.
    Lost { url: String, lines: Vec<usize> },
    /// The page closed (the agent's browser tools closed it): the tab opens it again.
    Closed,
    /// The page's renderer crashed: `Ctrl+R` opens it again.
    Crashed,
    /// Laura's browser denied a download.
    Download,
}

/// One browser pane's page, in its own window: a background tab sends no frames.
pub struct Page {
    cdp: Cdp,
    target: String,
    session: String,
    window: Option<i64>,
    rx: Receiver<Value>,
    /// The pending `Page.navigate`'s id and URL, for its `errorText`.
    nav: Option<(u64, String)>,
    url: String,
    title: String,
    /// The pending `document.title` read's id, and when the last one went.
    title_ask: Option<u64>,
    title_at: Instant,
    /// The last frame: when Laura's browser painted it (ms since the epoch), and its PNG.
    frame: Option<(f64, Vec<u8>)>,
    /// The last frame is newer than any the view received.
    fresh: bool,
    /// The screencast's CDP session, for acks.
    cast: Option<i64>,
    /// Frames Laura hasn't acked yet. The view's ack acks them all, so the screencast pauses
    /// while no view takes frames.
    owed: u32,
    size: Size,
    pending: Option<Size>,
    resized: Instant,
    /// `apply`'s `Page.startScreencast` until its reply: frames before it may be the old size.
    settling: Option<u64>,
    /// `keep_size`'s read of `screen`'s size, for a frame at another size.
    size_ask: Option<(u64, (u32, u32))>,
    /// The size the agent's tools hold the page at which Laura shows.
    emulated: Option<(u32, u32)>,
    /// The agent's tool connections that record the page.
    recorders: HashSet<u64>,
    /// The last press's point, to tell a drag from a click.
    down: Option<(f64, f64)>,
    /// The last release, for the requests it starts.
    released: Option<Instant>,
    /// The user's last keystroke or paste.
    keyed: Option<Instant>,
    /// The top frame loads.
    loading: bool,
    /// The `fetch` and XHR requests a click started, by request id.
    requests: HashMap<String, Instant>,
    /// The CSS cursor at the pointer, a keyword.
    cursor: String,
    /// The pending cursor read, and the newest point that waits for it.
    cursor_ask: Option<u64>,
    cursor_next: Option<(f64, f64)>,
    /// The pending release: its `Input` reply's id, its point, and whether it dragged. Then the
    /// read that follows it.
    release_ask: Option<(u64, f64, f64, bool)>,
    /// The pending focus read: its id, whether a drag or a click asked for it.
    select_ask: Option<(u64, bool, bool)>,
    /// The page's history: the current index, and each entry's id and URL.
    history: (usize, Vec<(i64, String)>),
    history_ask: Option<u64>,
    /// The page's open dialog: `{type, message, default}`.
    dialog: Option<Value>,
    /// Events for the view, which `take_out` drains.
    out: Vec<Value>,
    /// The top frame's `laura` world, where browser.js runs.
    world: Option<i64>,
    /// The scene that browser.js last drew, so an unchanged scene isn't sent again.
    drawn: Option<String>,
    /// The cursor's element's box that browser.js last sent, for a new view.
    rect: Value,
    /// The boxes that the web app outlines, as browser.js last sent them, for a new view.
    marks: Value,
    /// Comment mode: a hover outlines the element under the pointer.
    pub comment: bool,
    /// The popups this page opened that Laura closed, so a later event doesn't load one again.
    popups: HashSet<String>,
}

/// `v` is an array of `n` numbers.
fn numbers(v: &Value, n: usize) -> bool {
    v.as_array()
        .is_some_and(|a| a.len() == n && a.iter().all(Value::is_number))
}

/// browser.js's marks: `th` holds `[line, top, left, bottom, right]` per thread, `hl` a box or
/// null, and `cur` and `hov` a box with its tag (a string, or null for none), or null.
fn is_marks(m: &Value) -> bool {
    let tagged = |v: &Value| {
        v.is_null()
            || v.as_array().is_some_and(|a| {
                a.len() == 2 && numbers(&a[0], 4) && (a[1].is_null() || a[1].is_string())
            })
    };
    let thread = |t: &Value| {
        t.as_array()
            .is_some_and(|a| a.len() == 5 && a[0].is_u64() && a[1..].iter().all(Value::is_number))
    };
    m["th"].as_array().is_some_and(|th| th.iter().all(thread))
        && (m["hl"].is_null() || numbers(&m["hl"], 4))
        && tagged(&m["cur"])
        && tagged(&m["hov"])
}

/// browser.js's element: its CSS selector, its quote and its page's URL.
fn element(v: &Value) -> Option<(String, String, String)> {
    let field = |k: &str| v[k].as_str().map(str::to_string);
    field("selector")
        .zip(field("quote"))
        .zip(field("url"))
        .map(|((s, q), u)| (s, q, u))
}

impl Page {
    /// Open `url` in a new window of Laura's browser at `size`. The screencast starts before the
    /// navigation: started right after one, it fails while the page's process swaps.
    pub fn open(cdp: &Cdp, url: &str, size: Size) -> Result<Page> {
        let target = cdp.call(
            "Target.createTarget",
            json!({"url": "about:blank", "newWindow": true}),
            None,
        )?["targetId"]
            .as_str()
            .context("Laura's browser opened no page")?
            .to_string();
        let (tx, rx) = channel();
        cdp.route(&target, tx.clone());
        // From here, `Drop` closes the target if a step fails.
        let mut page = Page {
            cdp: cdp.clone(),
            target,
            session: String::new(),
            window: None,
            rx,
            nav: None,
            url: String::new(),
            title: String::new(),
            title_ask: None,
            title_at: Instant::now(),
            frame: None,
            fresh: false,
            cast: None,
            owed: 0,
            size,
            pending: None,
            resized: Instant::now(),
            settling: None,
            size_ask: None,
            emulated: None,
            recorders: HashSet::new(),
            down: None,
            released: None,
            keyed: None,
            loading: false,
            requests: HashMap::new(),
            cursor: "default".into(),
            cursor_ask: None,
            cursor_next: None,
            release_ask: None,
            select_ask: None,
            history: (0, vec![]),
            history_ask: None,
            dialog: None,
            out: vec![],
            world: None,
            drawn: None,
            rect: Value::Null,
            marks: Value::Null,
            comment: false,
            popups: HashSet::new(),
        };
        let attached = cdp.call(
            "Target.attachToTarget",
            json!({"targetId": page.target, "flatten": true}),
            None,
        )?;
        page.session = attached["sessionId"]
            .as_str()
            .context("Laura's browser attached no session")?
            .to_string();
        cdp.route(&page.session, tx);
        page.window = cdp.call(
            "Browser.getWindowForTarget",
            json!({"targetId": page.target}),
            None,
        )?["windowId"]
            .as_i64();
        page.send("Page.enable", json!({}));
        // For the spinner: the requests a click starts.
        page.send("Network.enable", json!({}));
        page.send(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source": PAGE_JS}),
        );
        // browser.js runs where the page's scripts can't see or change it.
        page.send("Runtime.enable", json!({}));
        page.send(
            "Runtime.addBinding",
            json!({"name": "laura", "executionContextName": "laura"}),
        );
        // Wait for the page's process to take the scripts and domains above: a navigation sent
        // before then can swap processes and drop them, leaving the page without its `laura` world.
        cdp.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source": BROWSER_JS, "worldName": "laura"}),
            Some(&page.session),
        )?;
        page.apply(size);
        page.navigate(url);
        Ok(page)
    }

    fn send(&self, method: &str, params: Value) -> u64 {
        self.cdp.send(method, params, Some(&self.session))
    }

    /// Load `url` in this page, keeping its window and screencast.
    pub fn navigate(&mut self, url: &str) {
        let id = self.send("Page.navigate", json!({"url": url}));
        self.nav = Some((id, url.to_string()));
    }

    /// A new tab or window this page opened (a middle-click, `target=other`) loads in this page
    /// instead: Laura's browser is headless, so the user can't see the popup.
    /// ponytail: a popup that never gets an http(s) URL (`about:blank` with `document.write`)
    /// stays open and hidden; close it too if that causes a problem.
    fn on_popup(&mut self, info: &Value) {
        let (Some(id), Some(url)) = (info["targetId"].as_str(), info["url"].as_str()) else {
            return;
        };
        if is_url(url) && self.popups.insert(id.to_string()) {
            self.cdp
                .send("Target.closeTarget", json!({"targetId": id}), None);
            self.navigate(url);
        }
    }

    pub fn reload(&self) {
        self.send("Page.reload", json!({}));
    }

    /// The page's CDP target id, which the agent's browser tools name the page by.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// The agent's browser tool took a screenshot of the page.
    pub fn screenshot(&mut self) {
        self.out.push(json!({"t": "screenshot"}));
    }

    /// Tool connection `conn` started (`on`) or stopped recording the page.
    pub fn record(&mut self, conn: u64, on: bool) {
        if on {
            self.recorders.insert(conn);
        } else {
            self.recorders.remove(&conn);
        }
    }

    /// The page's title, empty until the page names one.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The boxes that the web app outlines on the page, as browser.js last sent them.
    pub fn marks(&self) -> &Value {
        &self.marks
    }

    /// The cursor's element's box, as browser.js last sent it: a new view gets it.
    pub fn rect(&self) -> &Value {
        &self.rect
    }

    /// Set the window and the page to `size` and restart the screencast there, at
    /// most every 100 ms; `poll` applies the newest.
    pub fn resize(&mut self, size: Size) {
        self.pending = (size != self.size).then_some(size);
    }

    fn apply(&mut self, size: Size) {
        let (w, h, dpr) = size;
        (self.size, self.resized, self.owed) = (size, Instant::now(), 0);
        // A resize keeps the tools' emulation: at a new window size, Chrome shows the emulated page
        // at the window's size. Only the screencast takes the pane's size until the emulation ends.
        if self.emulated.is_none() {
            if let Some(id) = self.window {
                self.cdp.send(
                    "Browser.setWindowBounds",
                    json!({"windowId": id, "bounds": {"width": w, "height": h}}),
                    None,
                );
            }
            self.send(
                "Emulation.setDeviceMetricsOverride",
                json!({"width": w, "height": h, "deviceScaleFactor": dpr, "mobile": false}),
            );
        }
        let px = |n: u32| (f64::from(n) * dpr).round() as u32;
        self.send("Page.stopScreencast", json!({}));
        // Chrome scales an emulated page's frame to fit these, so the frame keeps its shape.
        self.settling = Some(self.send(
            "Page.startScreencast",
            json!({"format": "png", "maxWidth": px(w), "maxHeight": px(h)}),
        ));
    }

    /// Take what arrived since the last poll: frames, title and URL changes, a failed navigation,
    /// and the replies to Laura's reads.
    pub fn poll(&mut self, now: Instant) -> Vec<Event> {
        let mut out = vec![];
        while let Ok(v) = self.rx.try_recv() {
            if let Some(id) = v["id"].as_u64() {
                self.on_reply(id, &v, &mut out);
                continue;
            }
            let p = &v["params"];
            let top = p["frameId"].as_str() == Some(self.target.as_str());
            match v["method"].as_str() {
                Some("Page.screencastFrame") => self.on_frame(p),
                Some("Target.targetDestroyed" | "Target.detachedFromTarget")
                    if p["targetId"] == self.target.as_str() =>
                {
                    out.push(Event::Closed);
                }
                Some("Target.targetCrashed") if p["targetId"] == self.target.as_str() => {
                    out.push(Event::Crashed);
                }
                // Chrome's `targetInfoChanged` misses a title change, so Laura reads the title.
                Some("Page.domContentEventFired") => self.ask_title(now),
                Some("Page.loadEventFired") => {
                    self.ask_title(now);
                    self.ask_focus(-1.0, -1.0, false, false);
                }
                // A single-page app's route: `targetInfoChanged` can miss its URL.
                Some("Page.navigatedWithinDocument") if top => {
                    self.ask_title(now);
                    self.ask_focus(-1.0, -1.0, false, false);
                    self.ask_history();
                    self.set_url(p["url"].as_str().unwrap_or_default(), &mut out);
                }
                Some("Page.frameNavigated") if p["frame"]["parentId"].is_null() => {
                    self.ask_history();
                    // The new document's script sends its own marks; a document with no `body`
                    // sends none.
                    self.marks = Value::Null;
                    self.out.push(json!({"t": "marks", "marks": null}));
                }
                Some("Page.frameStartedLoading") if top => self.loading = true,
                Some("Page.frameStoppedLoading") if top => self.loading = false,
                Some("Network.requestWillBeSent")
                    if matches!(p["type"].as_str(), Some("Fetch" | "XHR"))
                        && self.released.is_some_and(|t| now < t + CLICK_WINDOW) =>
                {
                    if let Some(r) = p["requestId"].as_str() {
                        self.requests.insert(r.to_string(), now);
                    }
                }
                // The server answered: a streaming response stops the spinner.
                Some(
                    "Network.responseReceived"
                    | "Network.loadingFinished"
                    | "Network.loadingFailed",
                ) => {
                    if let Some(r) = p["requestId"].as_str() {
                        self.requests.remove(r);
                    }
                }
                Some("Page.javascriptDialogOpening") => {
                    self.dialog = Some(json!({"type": p["type"], "message": p["message"],
                                              "default": p["defaultPrompt"]}));
                }
                Some("Page.javascriptDialogClosed") => self.dialog = None,
                Some("Browser.downloadWillBegin") if top => {
                    out.push(Event::Download);
                }
                // Before the pane's own `targetInfoChanged`, so a popup's URL isn't the pane's.
                Some("Target.targetCreated" | "Target.targetInfoChanged")
                    if p["targetInfo"]["openerId"] == self.target.as_str()
                        && p["targetInfo"]["targetId"] != self.target.as_str() =>
                {
                    self.on_popup(&p["targetInfo"]);
                }
                Some("Target.targetInfoChanged") => {
                    let url = p["targetInfo"]["url"].as_str().unwrap_or_default();
                    self.set_url(url, &mut out);
                }
                Some("Runtime.executionContextCreated")
                    if p["context"]["name"] == "laura"
                        && p["context"]["auxData"]["frameId"] == self.target.as_str() =>
                {
                    self.world = p["context"]["id"].as_i64();
                    self.drawn = None;
                }
                Some("Runtime.executionContextsCleared") => self.world = None,
                Some("Runtime.executionContextDestroyed")
                    if p["executionContextId"].as_i64() == self.world =>
                {
                    self.world = None;
                }
                // The binding exists only in the `laura` worlds, one per frame.
                Some("Runtime.bindingCalled") if p["name"] == "laura" => {
                    let top =
                        self.world.is_some() && p["executionContextId"].as_i64() == self.world;
                    let payload = p["payload"].as_str().unwrap_or_default();
                    self.on_binding(payload, top, now, &mut out);
                }
                _ => {}
            }
        }
        self.requests.retain(|_, t| now < *t + REQUEST_MAX);
        // A read waits while a dialog is open, until the user answers it.
        if self.dialog.is_none() && now >= self.title_at + TITLE_EVERY {
            self.ask_title(now);
        }
        if let Some(size) = self.pending
            && now >= self.resized + RESIZE_EVERY
        {
            self.pending = None;
            self.apply(size);
        }
        out
    }

    fn on_reply(&mut self, id: u64, v: &Value, out: &mut Vec<Event>) {
        self.settling.take_if(|n| *n == id);
        let value = &v["result"]["result"]["value"];
        if let Some((_, dev)) = self.size_ask.take_if(|(n, _)| *n == id) {
            let screen = (value[0].as_u64(), value[1].as_u64());
            if screen == (Some(dev.0.into()), Some(dev.1.into())) {
                self.emulated = Some(dev);
            } else {
                self.emulated = None;
                self.apply(self.size);
            }
        }
        if self.title_ask.take_if(|n| *n == id).is_some()
            && let Some(t) = value.as_str()
        {
            self.title = t.to_string();
        }
        if let Some((_, url)) = self.nav.take_if(|(n, _)| *n == id) {
            let error = v["result"]["errorText"]
                .as_str()
                .or(v["error"]["message"].as_str())
                .unwrap_or_default();
            // A later navigation or Stay stopped it, and the page shows what the user chose.
            if !error.is_empty() && error != "net::ERR_ABORTED" {
                let error = error.to_string();
                out.push(Event::Failed { url, error });
            }
        }
        // The page handled the release.
        if let Some((_, x, y, dragged)) = self.release_ask.take_if(|r| r.0 == id) {
            self.ask_focus(x, y, dragged, true);
        }
        if let Some((_, dragged, click)) = self.select_ask.take_if(|s| s.0 == id) {
            let editable = value["editable"] == true;
            if click || editable {
                self.out
                    .push(json!({"t": "editable", "editable": editable}));
            }
            if let Some(text) = value["text"].as_str().filter(|t| dragged && !t.is_empty()) {
                self.out.push(json!({"t": "copy", "text": text}));
            }
        }
        if self.cursor_ask.take_if(|n| *n == id).is_some() {
            // A keyword only, so the page can't make the web app load a `url(…)`.
            let keyword =
                |c: &&str| !c.is_empty() && c.bytes().all(|b| b.is_ascii_lowercase() || b == b'-');
            self.cursor = value
                .as_str()
                .filter(keyword)
                .unwrap_or("default")
                .to_string();
            if let Some((x, y)) = self.cursor_next.take() {
                self.ask_cursor(x, y);
            }
        }
        if self.history_ask.take_if(|n| *n == id).is_some() {
            let r = &v["result"];
            let entries = r["entries"].as_array().map_or_else(Vec::new, |es| {
                es.iter()
                    .map(|e| {
                        let url = e["url"].as_str().unwrap_or_default().to_string();
                        (e["id"].as_i64().unwrap_or_default(), url)
                    })
                    .collect()
            });
            let at = r["currentIndex"].as_u64().unwrap_or_default() as usize;
            self.history = (at, entries);
        }
    }

    /// The top frame moved to `url`. Chrome's error page has its own URL, which isn't the pane's,
    /// and nor is the page's first `about:blank`.
    fn set_url(&mut self, url: &str, out: &mut Vec<Event>) {
        if is_url(url) && url != self.url {
            self.url = url.to_string();
            out.push(Event::Url(self.url.clone()));
        }
    }

    /// browser.js's reply: the cursor's element's box, the boxes that the web app outlines, the
    /// lost threads, or a click or an input on the page, for the agent's marks. The page's content
    /// shapes the reply, so each field's type is checked. A frame in the page (`top` false) reports
    /// only clicks and input.
    fn on_binding(&mut self, payload: &str, top: bool, now: Instant, out: &mut Vec<Event>) {
        let Ok(m) = serde_json::from_str::<Value>(payload) else {
            return;
        };
        // CDP doesn't send the agent's `Input` to Laura's session, so the page reports each click
        // and input, and Laura drops the user's own.
        // ponytail: the agent's input within `USER_INPUT` of the user's counts as the user's; tag
        // the user's input in the page if users miss a mark.
        let user = self
            .released
            .max(self.keyed)
            .is_some_and(|t| now < t + USER_INPUT);
        for key in ["click", "type"] {
            if !user && numbers(&m[key], if key == "click" { 2 } else { 4 }) {
                self.out.push(json!({"t": "agent", key: m[key]}));
            }
        }
        if !top {
            return;
        }
        if let Some(rect) = m.get("rect")
            && (numbers(rect, 4) || rect.is_null())
        {
            self.rect = rect.clone();
            self.out.push(json!({"t": "rect", "rect": rect}));
        }
        if let Some(marks) = m.get("marks")
            && is_marks(marks)
        {
            self.marks = marks.clone();
            self.out.push(json!({"t": "marks", "marks": marks}));
        }
        if let (Some(lines), Some(url)) = (m["lost"].as_array(), m["url"].as_str()) {
            let lines = lines.iter().filter_map(Value::as_u64).map(|l| l as usize);
            out.push(Event::Lost {
                url: url.to_string(),
                lines: lines.collect(),
            });
        }
    }

    /// Evaluate `js` in the `laura` world once it exists, else in the page's world.
    fn evaluate(&self, js: &str) -> u64 {
        let mut params = json!({"expression": js, "returnByValue": true, "awaitPromise": true});
        if let Some(id) = self.world {
            params["contextId"] = id.into();
        }
        self.send("Runtime.evaluate", params)
    }

    /// Draw the pane's threads and cursor (`Panel::scene`) on the page, with comment mode.
    pub fn draw(&mut self, mut scene: Value) {
        scene["comment"] = self.comment.into();
        let scene = scene.to_string();
        if self.world.is_none() || self.drawn.as_ref() == Some(&scene) {
            return;
        }
        self.evaluate(&format!("__laura.set({scene})"));
        self.drawn = Some(scene);
    }

    /// The element at the point: its CSS selector, its quote and its page's URL; `None` for no
    /// element. Waits up to 5 s for the page.
    pub fn pick(&self, x: f64, y: f64) -> Result<Option<(String, String, String)>, String> {
        if self.dialog.is_some() {
            return Err(DIALOG_OPEN.into());
        }
        let Some(id) = self.world else {
            return Err(LOADING.into());
        };
        let r = self
            .cdp
            .call(
                "Runtime.evaluate",
                json!({"expression": format!("__laura.pick({x}, {y})"),
                       "returnByValue": true, "contextId": id}),
                Some(&self.session),
            )
            .map_err(|e| format!("⚠ {e}"))?;
        Ok(element(&r["result"]["value"]))
    }

    /// The element that the agent's `selector` finds: its quote and its page's URL, or the thread's
    /// anchor on it; `None` for no element. Waits up to 5 s for the page.
    pub fn find(&self, selector: &str) -> Result<Option<(String, String, String)>, String> {
        let Some(id) = self.world else {
            return Ok(None);
        };
        let r = self
            .cdp
            .call(
                "Runtime.evaluate",
                json!({"expression": format!("__laura.find({})", json!(selector)),
                       "returnByValue": true, "contextId": id}),
                Some(&self.session),
            )
            .map_err(|e| e.to_string())?;
        let v = &r["result"]["value"];
        if v.get("error").is_some() {
            return Err(format!("invalid selector: {selector}"));
        }
        Ok(element(v))
    }

    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// The page has no `laura` world yet, so `find` can't look.
    pub fn loading(&self) -> bool {
        self.world.is_none()
    }

    /// Scroll the highlight's element into view.
    pub fn reveal(&self) {
        self.evaluate("__laura.reveal()");
    }

    /// Whether a text field has the focus, after a click at the point or a load (at `(-1, -1)`).
    /// A field in a same-origin iframe (a Storybook story) has the focus in the iframe's document,
    /// or a field in an open shadow root (a web component) in its shadow root.
    /// A page can focus a field a moment after the click (a form that opens), so with no field
    /// focused the read waits 100 ms and reads again.
    // ponytail: a focus later than 100 ms, or in a cross-origin iframe, reads as no field; a
    // `focusin` binding if users hit it. The read also runs after each load, for `autofocus`.
    fn ask_focus(&mut self, x: f64, y: f64, dragged: bool, click: bool) {
        if self.dialog.is_some() {
            return;
        }
        let js = format!(
            "(async () => {{ const read = () => {{ \
             let d = document, a = d.activeElement; \
             while (a?.shadowRoot?.activeElement || a?.contentDocument) {{              if (a.shadowRoot?.activeElement) a = a.shadowRoot.activeElement;              else {{ d = a.contentDocument; a = d.activeElement; }} }} \
             const t = document.elementFromPoint({x}, {y}); \
             return {{ editable: !!a && (a.isContentEditable || a.matches({TEXT:?}) \
             || a === t && a.hasAttribute('tabindex')), \
             text: {dragged} ? d.getSelection().toString() : '' }}; }}; \
             let v = read(); \
             if (!v.editable) {{ await new Promise(r => setTimeout(r, 100)); v = read(); }} \
             return v; }})()"
        );
        self.select_ask = Some((self.evaluate(&js), dragged, click));
    }

    fn ask_title(&mut self, now: Instant) {
        self.title_at = now;
        self.title_ask = Some(self.evaluate("document.title"));
    }

    fn ask_history(&mut self) {
        self.history_ask = Some(self.send("Page.getNavigationHistory", json!({})));
    }

    /// The computed cursor at the point. `auto` reads as `text` over a field or text, else
    /// `default`; a `url(…)` cursor reads as its keyword fallback.
    fn ask_cursor(&mut self, x: f64, y: f64) {
        let js = format!(
            "(() => {{ const e = document.elementFromPoint({x}, {y}); if (!e) return 'default'; \
             const c = getComputedStyle(e).cursor.split(',').pop().trim(); \
             if (c !== 'auto') return c; \
             if (e.isContentEditable || e.matches({TEXT:?})) return 'text'; \
             const n = document.caretRangeFromPoint({x}, {y})?.startContainer; \
             if (n?.nodeType !== 3) return 'default'; \
             const r = document.createRange(); r.selectNodeContents(n); \
             return [...r.getClientRects()].some(b => {x} >= b.left && {x} <= b.right \
             && {y} >= b.top && {y} <= b.bottom) ? 'text' : 'default'; }})()"
        );
        self.cursor_ask = Some(self.evaluate(&js));
    }

    /// A pointer event from the view. A release then reads whether the focused element takes the
    /// keystrokes and, after a drag, the selection; a move or the wheel reads the cursor.
    pub fn mouse(&mut self, m: &Mouse, now: Instant) {
        let held = match m.buttons {
            b if b & 1 != 0 => "left",
            b if b & 4 != 0 => "middle",
            b if b & 2 != 0 => "right",
            _ => "none",
        };
        let pressed = ["left", "middle", "right"]
            .get(usize::from(m.button))
            .copied()
            .unwrap_or("none");
        let (typ, button) = match m.kind {
            MouseKind::Down => ("mousePressed", pressed),
            MouseKind::Up => ("mouseReleased", pressed),
            MouseKind::Move => ("mouseMoved", held),
            MouseKind::Wheel => ("mouseWheel", "none"),
        };
        let mut params = json!({"type": typ, "x": m.x, "y": m.y, "button": button,
                                "buttons": m.buttons, "modifiers": m.mods});
        match m.kind {
            MouseKind::Down | MouseKind::Up => params["clickCount"] = m.clicks.max(1).into(),
            MouseKind::Wheel => (params["deltaX"], params["deltaY"]) = (m.dx.into(), m.dy.into()),
            MouseKind::Move => {}
        }
        let id = self.send("Input.dispatchMouseEvent", params);
        match m.kind {
            MouseKind::Down => self.down = Some((m.x, m.y)),
            MouseKind::Up => {
                let moved = |(x, y): (f64, f64)| (m.x - x).hypot(m.y - y) > 3.0;
                let dragged = m.clicks > 1 || self.down.take().is_some_and(moved);
                self.released = Some(now);
                self.release_ask = Some((id, m.x, m.y, dragged));
            }
            // A drag keeps the cursor it started with.
            _ if self.dialog.is_some() || m.buttons != 0 => {}
            _ if self.cursor_ask.is_some() => self.cursor_next = Some((m.x, m.y)),
            _ => self.ask_cursor(m.x, m.y),
        }
    }

    /// A keystroke from the view. Enter types `\r`, as a browser's own Enter does.
    /// ponytail: no IME, and Chrome runs no editing shortcut (`Ctrl+A`, `Ctrl+C` in a field) from a
    /// key event alone; send the key's `commands` too if the user misses them.
    pub fn key(&mut self, k: &Key, now: Instant) {
        self.keyed = Some(now);
        let text = if k.key == "Enter" { "\r" } else { &k.text };
        let typ = match k.kind {
            KeyKind::Down if !text.is_empty() => "keyDown",
            KeyKind::Down => "rawKeyDown",
            KeyKind::Up => "keyUp",
        };
        let mut params = json!({"type": typ, "key": k.key, "code": k.code, "modifiers": k.mods,
                                "windowsVirtualKeyCode": k.key_code,
                                "nativeVirtualKeyCode": k.key_code});
        if typ == "keyDown" {
            (params["text"], params["unmodifiedText"]) = (text.into(), text.into());
        }
        self.send("Input.dispatchKeyEvent", params);
    }

    /// A paste from the view.
    pub fn insert_text(&mut self, text: &str, now: Instant) {
        self.keyed = Some(now);
        self.send("Input.insertText", json!({"text": text}));
    }

    /// `Esc` gave the keystrokes back to Laura: the page's field lets go of the focus, so the scroll
    /// keys scroll instead of typing into it.
    pub fn blur(&self) {
        self.evaluate(
            "{ let a = document.activeElement; \
             while (a?.shadowRoot?.activeElement || a?.contentDocument?.activeElement)              a = a.shadowRoot?.activeElement ?? a.contentDocument.activeElement; \
             a?.blur(); }",
        );
    }

    /// Go back (`delta` < 0) or forward one page in the page's history, past its first
    /// `about:blank`.
    pub fn go(&self, delta: isize) -> Result<(), String> {
        let (at, entries) = &self.history;
        let real = |i: &usize| entries.get(*i).is_some_and(|(_, u)| u != "about:blank");
        let to = if delta < 0 {
            (0..*at).rev().find(real)
        } else {
            (at + 1..entries.len()).find(real)
        };
        let Some(to) = to else {
            let way = if delta < 0 { "back" } else { "forward" };
            return Err(format!("the page has no history to go {way} to"));
        };
        self.send(
            "Page.navigateToHistoryEntry",
            json!({"entryId": entries[to].0}),
        );
        Ok(())
    }

    /// Answer the page's dialog: OK (Leave) or Cancel (Stay), with a `prompt`'s text.
    pub fn answer(&self, accept: bool, text: &str) {
        self.send(
            "Page.handleJavaScriptDialog",
            json!({"accept": accept, "promptText": text}),
        );
    }

    /// What the view shows of the page besides its frames, title and URL.
    pub fn state(&self, now: Instant) -> Map<String, Value> {
        let (at, entries) = &self.history;
        let real = |es: Option<&[(i64, String)]>| {
            es.unwrap_or_default()
                .iter()
                .any(|(_, u)| u != "about:blank")
        };
        // Chrome replies to a release after the page handles it, and `release_ask` waits for that.
        let handling = self.dialog.is_none()
            && self.release_ask.is_some()
            && self
                .released
                .is_some_and(|t| now >= t + CLICK_SLOW && now < t + REQUEST_MAX);
        let mut m = Map::new();
        m.insert(
            "busy".into(),
            (self.loading || !self.requests.is_empty() || handling).into(),
        );
        m.insert("cursor".into(), self.cursor.clone().into());
        m.insert("back".into(), real(entries.get(..*at)).into());
        m.insert("forward".into(), real(entries.get(at + 1..)).into());
        m.insert("dialog".into(), self.dialog.clone().unwrap_or_default());
        let emulated = self.emulated.map(|(w, h)| json!({"w": w, "h": h}));
        m.insert("emulated".into(), emulated.unwrap_or_default());
        m.insert("recording".into(), (!self.recorders.is_empty()).into());
        m
    }

    /// The view's events since the last call: `editable` after each release, `copy` after a drag,
    /// `agent` for the agent's click or input, `screenshot` for the agent's screenshot, `rect` for
    /// the cursor's element's box, and `marks` for the boxes that the web app outlines.
    pub fn take_out(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.out)
    }

    /// A new view: drop the events that no view saw, and send again the page's state that the old
    /// view took: the cursor's element's box and the focused field.
    pub fn new_view(&mut self) {
        self.out = vec![
            json!({"t": "rect", "rect": self.rect}),
            json!({"t": "marks", "marks": self.marks}),
        ];
        self.ask_focus(-1.0, -1.0, false, false);
    }

    fn on_frame(&mut self, p: &Value) {
        let Some(png) = p["data"]
            .as_str()
            .and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok())
        else {
            return;
        };
        self.cast = p["sessionId"].as_i64();
        let meta = &p["metadata"];
        if self.settling.is_none()
            && let (Some(dw), Some(dh)) =
                (meta["deviceWidth"].as_f64(), meta["deviceHeight"].as_f64())
        {
            self.keep_size((dw.round() as u32, dh.round() as u32));
        }
        // A repeated frame (a caret's blink back, a repaint) changes nothing on screen.
        if self.frame.as_ref().is_some_and(|(_, f)| *f == png) {
            self.ack_one();
            return;
        }
        // CDP's timestamp is in seconds.
        let painted = p["metadata"]["timestamp"]
            .as_f64()
            .map_or_else(now_ms, |s| s * 1000.0);
        self.frame = Some((painted, png));
        self.fresh = true;
        self.owed += 1;
    }

    /// Keep the pane's size. A frame at another size is the agent's tools emulating
    /// a device, which Laura shows, or the tools clearing their emulation, which clears Laura's too.
    /// Laura can't take the size back from a tool's emulation: setting it again only moves the
    /// frames, and the page stays laid out at the tool's size. An emulation sets `screen` to its
    /// size; a cleared page has the browser's own `screen`, and Laura sets the pane's size again.
    /// ponytail: a tool that sets `screenWidth` to another size than its page reads as a clear.
    fn keep_size(&mut self, dev: (u32, u32)) {
        if dev == (self.size.0, self.size.1) {
            self.emulated = None;
        } else if self.emulated != Some(dev) && self.size_ask.is_none_or(|(_, d)| d != dev) {
            let id = self.evaluate("[screen.width, screen.height]");
            self.size_ask = Some((id, dev));
        }
    }

    fn ack_one(&self) {
        if let Some(s) = self.cast {
            self.send("Page.screencastFrameAck", json!({"sessionId": s}));
        }
    }

    /// The view received a frame: Laura's browser may send the next.
    pub fn ack(&mut self) {
        for _ in 0..std::mem::take(&mut self.owed) {
            self.ack_one();
        }
    }

    /// Whether a frame came since the last call.
    pub fn take_fresh(&mut self) -> bool {
        std::mem::take(&mut self.fresh)
    }

    /// The last frame for the view: its paint time as 8 big-endian bytes, then the PNG.
    pub fn frame(&self) -> Option<Vec<u8>> {
        let (painted, png) = self.frame.as_ref()?;
        let mut out = painted.to_be_bytes().to_vec();
        out.extend_from_slice(png);
        Some(out)
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        self.cdp
            .send("Target.closeTarget", json!({"targetId": self.target}), None);
        self.cdp.unroute(&self.target);
        self.cdp.unroute(&self.session);
    }
}

/// `url` without its `#fragment`: a link within the page keeps its threads.
pub fn page_of(url: &str) -> &str {
    match url.split_once('#') {
        // A hash-routed app's route, such as `#/settings`, is a page of its own.
        Some((page, f)) if !f.starts_with('/') && !f.starts_with("!/") => page,
        _ => url,
    }
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}
