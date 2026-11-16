//! The Laura server (#94): `laura --serve` lists the running workspaces at one fixed address and
//! starts new ones. The Laura server owns no workspace and forwards no frames: each workspace
//! keeps its own port and token, and the Laura server's page links to it.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde_json::{Value, json};

use crate::gitdiff::strip_verbatim;
use crate::journal::{data_dir, runtime_dir};
use crate::web::{self, header, shown_root};

/// 1989: the year Rachel and Stephen Kaplan named soft fascination.
pub const DEFAULT_PORT: u16 = 1989;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_BODY: usize = 64 * 1024;
const START_TIMEOUT: Duration = Duration::from_secs(10);
/// shortcut: a program that ends after this time still opens a workspace that then ends, tell the page if this is reported.
const SETTLE: Duration = Duration::from_millis(300);

const PAGE: &[u8] = include_bytes!("../assets/web/serve.html");
const SCRIPT: &[u8] = include_bytes!("../assets/web/serve.js");

/// What every connection needs.
struct Server {
    token: String,
    port: u16,
    /// Read once: Linux reports `… (deleted)` after an upgrade.
    exe: PathBuf,
}

/// Where the Laura server keeps its token and its port: `{token, port}`.
fn state_path() -> PathBuf {
    data_dir().join("serve.json")
}

/// The Laura server's URL with its token, when a Laura server runs.
pub(crate) fn running() -> Option<String> {
    let v: Value = serde_json::from_slice(&std::fs::read(state_path()).ok()?).ok()?;
    let port = u16::try_from(v["port"].as_u64()?).ok()?;
    let token = v["token"].as_str()?;
    is_laura_server(port).then(|| format!("http://127.0.0.1:{port}/?t={token}"))
}

/// Whether the program on `port` is a Laura server: each response of one has the version.
/// shortcut: a program that copies the header passes, sign the answer if this matters.
fn is_laura_server(port: u16) -> bool {
    let wait = Duration::from_millis(200);
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), wait) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(wait));
    let _ = s.set_write_timeout(Some(wait));
    if s.write_all(b"GET /serve.js HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .is_err()
    {
        return false;
    }
    // A timeout keeps the bytes read so far.
    let mut head = vec![];
    let _ = s.take(512).read_to_end(&mut head);
    String::from_utf8_lossy(&head).contains("\r\nX-Laura-Version: ")
}

/// `laura --serve`: listen on `port` until the user stops the Laura server.
pub fn run(port: u16) -> Result<()> {
    let here = format!("http://127.0.0.1:{port}/");
    if let Some(url) = running().filter(|u| u.starts_with(&here)) {
        eprintln!("The Laura server already runs. Open this URL in a web browser:");
        println!("{url}");
        return Ok(());
    }
    let listener = web::listen(port)?;
    let port = listener.local_addr()?.port();
    // The token stays across restarts, so the key that the page keeps still works.
    let kept = std::fs::read(state_path())
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["token"].as_str().map(str::to_string))
        .filter(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()));
    let token = match kept {
        Some(t) => t,
        None => web::nonce(32)?,
    };
    std::fs::create_dir_all(data_dir())?;
    web::write_private(
        &state_path(),
        json!({"token": token, "port": port}).to_string().as_bytes(),
    )?;
    let server = std::sync::Arc::new(Server {
        token,
        port,
        exe: std::env::current_exe()?,
    });
    // The label goes to stderr, so stdout stays the URL alone for scripts.
    eprintln!("Open this URL in a web browser:");
    println!("http://127.0.0.1:{port}/?t={}", server.token);
    for stream in listener.incoming().flatten() {
        let server = server.clone();
        // A thread per connection: a socket that sends nothing can't block the rest.
        std::thread::spawn(move || {
            let _ = route(stream, &server);
        });
    }
    Ok(())
}

/// One response. Every response carries the version, and none carries
/// `Access-Control-Allow-Origin`: no other site's page reads a response.
fn reply(stream: &mut TcpStream, status: &str, mime: &str, body: &[u8], extra: &str) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n\
         X-Laura-Version: {VERSION}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nReferrer-Policy: {}\r\n\
         Content-Security-Policy: {}\r\n{extra}Connection: close\r\n\r\n",
        body.len(),
        web::REFERRER_POLICY,
        web::CSP,
    )?;
    stream.write_all(body)?;
    Ok(())
}

fn text(stream: &mut TcpStream, status: &str, body: &str) -> Result<()> {
    reply(
        stream,
        status,
        "text/plain; charset=utf-8",
        body.as_bytes(),
        "",
    )
}

fn json_reply(stream: &mut TcpStream, v: &Value) -> Result<()> {
    reply(
        stream,
        "200 OK",
        "application/json",
        v.to_string().as_bytes(),
        "",
    )
}

/// One connection: the page and its files, or a request that needs the key.
fn route(mut stream: TcpStream, s: &Server) -> Result<()> {
    let Some(mut head) = web::peek_head(&mut stream)? else {
        return Ok(());
    };
    stream.read_exact(&mut head)?;
    let head = String::from_utf8_lossy(&head).into_owned();
    let mut words = head.split(' ');
    let method = words.next().unwrap_or("");
    let target = words.next().unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let param = |name: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
    };
    // Read the body before any answer: a close with unread bytes resets the connection.
    let len = header(&head, "Content-Length").and_then(|n| n.parse::<usize>().ok());
    let mut body = vec![0; len.unwrap_or(0).min(MAX_BODY)];
    stream.read_exact(&mut body)?;
    if len.is_some_and(|n| n > MAX_BODY) {
        return text(&mut stream, "413 Content Too Large", "");
    }
    if web::foreign_subresource(&head) {
        return text(&mut stream, "404 Not Found", "");
    }
    // The page sends the key in a header: a browser sends a cookie to every 127.0.0.1 port.
    let authed =
        header(&head, "X-Laura-Key").is_some_and(|k| web::same(k.as_bytes(), s.token.as_bytes()));
    let page = "text/html; charset=utf-8";
    match (method, path) {
        // The page holds no workspace: it asks `/workspaces`, and shows how to start Laura on 401.
        ("GET", "/") => reply(&mut stream, "200 OK", page, PAGE, ""),
        ("GET", "/serve.js") => reply(&mut stream, "200 OK", "text/javascript", SCRIPT, ""),
        ("GET", "/style.css" | "/fonts/Inter.woff2" | "/fonts/JetBrainsMono.woff2") => {
            match web::ASSETS.iter().find(|(p, ..)| *p == path) {
                Some((_, mime, body)) => reply(&mut stream, "200 OK", mime, body, ""),
                None => text(&mut stream, "404 Not Found", ""),
            }
        }
        ("GET" | "POST", "/workspaces" | "/dirs" | "/new" | "/quit") if !authed => {
            text(&mut stream, "401 Unauthorized", "")
        }
        ("GET", "/workspaces") => {
            let mut list = web::live();
            list.reverse(); // newest first
            for w in &mut list {
                let shown = shown_root(Path::new(w["root"].as_str().unwrap_or("")));
                w["path"] = std::mem::replace(&mut w["root"], shown.into());
            }
            json_reply(&mut stream, &Value::Array(list))
        }
        ("GET", "/dirs") => {
            let path = param("path").and_then(web::pct_decode).unwrap_or_default();
            match dirs(&path) {
                Some(v) => json_reply(&mut stream, &v),
                None => text(&mut stream, "404 Not Found", &not_a_folder(&path)),
            }
        }
        // A second check: only the Laura server's own page starts or stops a process.
        ("POST", "/new" | "/quit") if !web::our_origin(header(&head, "Origin"), s.port) => {
            text(&mut stream, "403 Forbidden", "")
        }
        ("POST", "/new") => {
            let v: Value = serde_json::from_slice(&body).unwrap_or_default();
            let (folder, program) = (v["folder"].as_str(), v["program"].as_str());
            match start(&s.exe, folder.unwrap_or(""), program.unwrap_or("")) {
                Ok(url) => json_reply(&mut stream, &json!({"url": url})),
                Err(e) => text(&mut stream, "400 Bad Request", &e),
            }
        }
        // The workspaces are separate processes: they keep running.
        ("POST", "/quit") => {
            text(&mut stream, "200 OK", "")?;
            stream.flush()?;
            let _ = stream.shutdown(std::net::Shutdown::Both);
            std::process::exit(0);
        }
        _ => text(&mut stream, "404 Not Found", ""),
    }
}

fn not_a_folder(typed: &str) -> String {
    format!("{typed} is not a folder.")
}

/// The home folder, in the form `shown_root` compares with.
fn home() -> Option<PathBuf> {
    std::env::home_dir()?
        .canonicalize()
        .ok()
        .map(strip_verbatim)
}

/// The folder that the user typed: an empty text is the home folder, and a leading `~` is the
/// home folder. `None` when the text names no folder.
fn folder(typed: &str) -> Option<PathBuf> {
    let typed = typed.trim();
    let path = match typed.strip_prefix('~') {
        _ if typed.is_empty() => home()?,
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            home()?.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(typed),
    };
    let path = path
        .is_absolute()
        .then(|| path.canonicalize().ok())
        .flatten()?;
    path.is_dir().then(|| strip_verbatim(path))
}

/// The folder browser's list for the folder that the user typed: the folder, the folders above it
/// and the folders in it, without dot-folders.
fn dirs(typed: &str) -> Option<Value> {
    // On Windows, the folder above a drive lists the drives.
    if cfg!(windows) && typed == "/" {
        let drives: Vec<Value> = (b'A'..=b'Z')
            .map(|d| format!("{}:\\", d as char))
            .filter(|d| Path::new(d).is_dir())
            .map(|d| json!({"name": d[..2], "path": d, "git": false}))
            .collect();
        return Some(json!({"path": "/", "shown": "Drives", "dirs": drives,
                           "crumbs": [{"name": "Drives", "path": "/"}]}));
    }
    let dir = folder(typed)?;
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    let list: Vec<Value> = names
        .iter()
        .map(|n| {
            let p = dir.join(n);
            json!({"name": n, "path": p.to_string_lossy(), "git": p.join(".git").exists()})
        })
        .collect();
    // The breadcrumbs: from the home folder (`~`) when the folder is below it, else from the root.
    let home = home();
    let mut crumbs = vec![];
    for a in dir.ancestors() {
        let at_home = home.as_deref() == Some(a);
        let name = match a.file_name() {
            _ if at_home => "~".to_string(),
            Some(n) => n.to_string_lossy().into_owned(),
            None if cfg!(windows) => a.to_string_lossy().trim_end_matches('\\').to_string(),
            None => "/".to_string(),
        };
        crumbs.push(json!({"name": name, "path": a.to_string_lossy()}));
        if at_home {
            break;
        }
    }
    if cfg!(windows) && crumbs.last().is_some_and(|c| c["name"] != "~") {
        crumbs.push(json!({"name": "Drives", "path": "/"}));
    }
    crumbs.reverse();
    Some(
        json!({"path": dir.to_string_lossy(), "shown": shown_root(&dir),
                "dirs": list, "crumbs": crumbs}),
    )
}

/// Start a workspace in the folder that the user typed, with no terminal: the workspace's URL, or
/// the error that the page shows.
fn start(exe: &Path, typed: &str, program: &str) -> Result<String, String> {
    let dir = folder(typed).ok_or_else(|| not_a_folder(typed))?;
    let failed = |e: &dyn std::fmt::Display| format!("The workspace stopped at once: {e}");
    let runtime = runtime_dir();
    std::fs::create_dir_all(&runtime).map_err(|e| failed(&e))?;
    // The workspace's stderr, which tells why a workspace stopped at once.
    let log = runtime.join(format!(
        "start-{}.log",
        web::nonce(8).map_err(|e| failed(&e))?
    ));
    let stderr = std::fs::File::create(&log).map_err(|e| failed(&e))?;
    let mut cmd = Command::new(exe);
    cmd.args(["--web", "--port", "0"]);
    // shortcut: no quoting, upgrade when a user needs a quoted argument.
    let words: Vec<&str> = program.split_whitespace().collect();
    if !words.is_empty() {
        cmd.arg("--").args(words);
    }
    // No pipe to the Laura server: the workspace keeps running after the Laura server stops.
    cmd.current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr);
    // The terminal's Ctrl+C and hangup go to the Laura server's group only.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    // An older record with the same process id is a different workspace.
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let mut child = cmd.spawn().map_err(|e| failed(&e))?;
    let record = web::record_path(child.id());
    let spawned = Instant::now();
    let result = loop {
        if let Ok(Some(_)) = child.try_wait() {
            // Laura's error can take more than one line: the page shows them as one. A program
            // that ends at once leaves only the label of the URL.
            let out = std::fs::read_to_string(&log).unwrap_or_default();
            let lines: Vec<&str> = out
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && *l != "Open this URL in a web browser:")
                .collect();
            break Err(failed(
                &match lines.join(" ").trim_start_matches("Error: ") {
                    "" => "the program exited.",
                    why => why,
                },
            ));
        }
        let url = std::fs::read(&record)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .filter(|v| v["started"].as_u64().is_some_and(|ms| ms >= before))
            .and_then(|v| v["url"].as_str().map(str::to_string));
        // The record comes before the program ends: wait that long for a program that ends at once.
        if let Some(url) = url.filter(|_| spawned.elapsed() >= SETTLE) {
            break Ok(url);
        }
        if spawned.elapsed() >= START_TIMEOUT {
            let _ = child.kill();
            break Err("The workspace did not start in 10 s.".to_string());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = std::fs::remove_file(&log);
    // Collect the workspace's exit status when it ends, so no ended process stays in the table.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    result
}
