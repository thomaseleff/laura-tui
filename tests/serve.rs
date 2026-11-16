//! The Laura server (#94): `laura --serve` through its HTTP routes, as the Laura server's page
//! drives it, and the records that each workspace writes. Each test keeps Laura's data in its own
//! home folder, so the tests' workspaces and Laura servers stay apart.

mod common;
#[path = "common/web.rs"]
#[allow(dead_code)] // each test crate uses part of the rig
mod rig;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use serde_json::{Value, json};
use tungstenite::Message;

use rig::*;

/// One request; the raw response. `headers`: each line ends in `\r\n`.
fn http(port: u16, method: &str, path: &str, headers: &str, body: &str) -> Result<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{headers}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )?;
    let mut resp = vec![];
    s.read_to_end(&mut resp)?;
    Ok(String::from_utf8_lossy(&resp).into_owned())
}

fn status(resp: &str) -> &str {
    resp.lines().next().unwrap_or("")
}

fn body(resp: &str) -> &str {
    resp.split_once("\r\n\r\n").map_or("", |(_, b)| b)
}

/// A fresh folder below the tests' own temp folder.
fn temp() -> Result<tempfile::TempDir> {
    Ok(tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))?)
}

/// A `laura --serve` with its own home folder.
struct Server {
    child: Kill,
    port: u16,
    token: String,
    home: tempfile::TempDir,
}

impl Server {
    fn start() -> Result<Server> {
        let home = temp()?;
        Server::start_in(home)
    }

    fn start_in(home: tempfile::TempDir) -> Result<Server> {
        let args = ["--serve", "--port", "0"];
        let (child, url) = laura_start(home.path(), &args, Some(home.path()))?;
        let (port, token) = port_token(&url);
        Ok(Server {
            child,
            port,
            token,
            home,
        })
    }

    /// The headers of the Laura server's own page: the key and the page's origin.
    fn headers(&self) -> String {
        format!(
            "X-Laura-Key: {}\r\nOrigin: http://127.0.0.1:{}\r\n",
            self.token, self.port
        )
    }

    fn get(&self, path: &str) -> Result<String> {
        http(self.port, "GET", path, &self.headers(), "")
    }

    fn post(&self, path: &str, body: Value) -> Result<String> {
        http(self.port, "POST", path, &self.headers(), &body.to_string())
    }

    fn workspaces(&self) -> Result<Vec<Value>> {
        let resp = self.get("/workspaces")?;
        assert_eq!(status(&resp), "HTTP/1.1 200 OK", "{resp}");
        Ok(serde_json::from_str(body(&resp))?)
    }

    /// A workspace with the shell in `dir`, in the Laura server's home folder.
    fn workspace(&self, dir: &Path) -> Result<(Kill, String)> {
        laura_start(dir, &["--web", "--", PROGRAM], Some(self.home.path()))
    }

    /// Start a workspace with the shell in `dir` through `POST /new`: its URL.
    fn new_workspace(&self, dir: &Path) -> Result<(Stop, String)> {
        let resp = self.post("/new", json!({"folder": dir, "program": PROGRAM}))?;
        assert_eq!(status(&resp), "HTTP/1.1 200 OK", "{resp}");
        let url = serde_json::from_str::<Value>(body(&resp))?["url"]
            .as_str()
            .expect("a URL")
            .to_string();
        let record = self.record(&url)?.expect("the workspace's record");
        Ok((Stop(record["pid"].as_u64().expect("a pid")), url))
    }

    /// The row of the workspace at `url`.
    fn record(&self, url: &str) -> Result<Option<Value>> {
        Ok(self.workspaces()?.into_iter().find(|w| w["url"] == url))
    }
}

/// Ends the workspace that the Laura server started, also when an assert fails.
struct Stop(u64);

impl Drop for Stop {
    fn drop(&mut self) {
        let pid = self.0.to_string();
        let mut kill = if cfg!(windows) {
            let mut c = std::process::Command::new("taskkill");
            c.args(["/F", "/T", "/PID", &pid]);
            c
        } else {
            let mut c = std::process::Command::new("kill");
            c.args(["-9", &pid]);
            c
        };
        let _ = kill.output();
    }
}

/// Wait until `done` holds; fail after [`common::DEADLINE`].
fn until(what: &str, mut done: impl FnMut() -> Result<bool>) -> Result<()> {
    let start = Instant::now();
    while !done()? {
        assert!(start.elapsed() < common::DEADLINE, "timed out: {what}");
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

/// A view of the workspace at `url`, as a browser tab opens it.
fn view(url: &str) -> Result<Ws> {
    let (port, token) = port_token(url);
    let origin = format!("http://127.0.0.1:{port}");
    Ok(connect_to(port, Some(&token), Some(&origin))?)
}

#[test]
fn the_laura_server_shows_workspaces_only_with_the_key() -> Result<()> {
    let s = Server::start()?;
    let dir = temp()?;
    let (_ws, url) = s.workspace(dir.path())?;
    until("the workspace's row", || Ok(s.record(&url)?.is_some()))?;
    let workspace_token = port_token(&url).1;

    // No key: no workspace.
    let none = http(s.port, "GET", "/workspaces", "", "")?;
    assert_eq!(status(&none), "HTTP/1.1 401 Unauthorized", "{none}");
    let page = http(s.port, "GET", "/", "", "")?;
    assert_eq!(status(&page), "HTTP/1.1 200 OK", "{page}");
    assert!(page.contains("Open the Laura server"));
    assert!(page.contains("Select the Laura icon, or run <code>laura --serve</code>"));
    assert!(page.contains("or run <code>laura --serve</code>, to start the Laura server again"));
    assert!(!page.contains(&workspace_token), "the page holds a token");

    // The printed address serves the page, which keeps the key and sends it in a header.
    let first = http(s.port, "GET", &format!("/?t={}", s.token), "", "")?;
    assert_eq!(body(&first), body(&page), "{first}");
    let wrong = http(s.port, "GET", "/workspaces", "X-Laura-Key: nope\r\n", "")?;
    assert_eq!(status(&wrong), "HTTP/1.1 401 Unauthorized", "{wrong}");
    // A browser sends a cookie to every 127.0.0.1 port, so a cookie is no key.
    let cookie = format!("Cookie: laura_serve_{}={}\r\n", s.port, s.token);
    let cookie = http(s.port, "GET", "/workspaces", &cookie, "")?;
    assert_eq!(status(&cookie), "HTTP/1.1 401 Unauthorized", "{cookie}");
    let list = s.get("/workspaces")?;
    assert!(list.contains(&workspace_token), "{list}");

    // Another site's page: no list, and no workspace starts.
    let key = format!("X-Laura-Key: {}\r\n", s.token);
    let other = format!("{key}Origin: http://127.0.0.1:1\r\n");
    let body = json!({"folder": dir.path(), "program": PROGRAM}).to_string();
    let new = http(s.port, "POST", "/new", &other, &body)?;
    assert_eq!(status(&new), "HTTP/1.1 403 Forbidden", "{new}");
    let quit = http(s.port, "POST", "/quit", &key, "")?;
    assert_eq!(status(&quit), "HTTP/1.1 403 Forbidden", "{quit}");
    let no_key = format!("Origin: http://127.0.0.1:{}\r\n", s.port);
    let new = http(s.port, "POST", "/new", &no_key, &body)?;
    assert_eq!(status(&new), "HTTP/1.1 401 Unauthorized", "{new}");
    let fetch = format!("{key}Sec-Fetch-Site: same-site\r\nSec-Fetch-Mode: no-cors\r\n");
    let script = http(s.port, "GET", "/workspaces", &fetch, "")?;
    assert_eq!(status(&script), "HTTP/1.1 404 Not Found", "{script}");
    assert_eq!(s.workspaces()?.len(), 1, "a workspace started");

    let version = format!("X-Laura-Version: {}\r\n", env!("CARGO_PKG_VERSION"));
    for resp in [
        &none, &page, &first, &wrong, &cookie, &list, &new, &quit, &script,
    ] {
        assert!(resp.contains(&version), "{resp}");
        assert!(!resp.contains("Set-Cookie"), "{resp}");
        let lower = resp.to_lowercase();
        assert!(!lower.contains("access-control-allow-origin"), "{resp}");
    }
    Ok(())
}

#[test]
fn the_laura_server_shows_the_laura_logo() -> Result<()> {
    let s = Server::start()?;
    let logo = http(s.port, "GET", "/logo.svg", "", "")?;
    assert_eq!(status(&logo), "HTTP/1.1 200 OK", "{logo}");
    assert!(logo.contains("Content-Type: image/svg+xml\r\n"), "{logo}");
    assert!(body(&logo).starts_with("<svg"), "{logo}");
    let icon = http(s.port, "GET", "/laura.ico", "", "")?;
    assert_eq!(status(&icon), "HTTP/1.1 200 OK", "{icon}");
    assert!(icon.contains("Content-Type: image/x-icon\r\n"), "{icon}");
    // An ICO header, then 8 frames: 16, 24, 32, 48, 64, 96, 128 and 256 px.
    assert!(
        body(&icon).as_bytes().starts_with(&[0, 0, 1, 0, 8, 0]),
        "{:?}",
        &body(&icon).as_bytes()[..6.min(body(&icon).len())]
    );
    let page = http(s.port, "GET", "/", "", "")?;
    assert!(page.contains("<link rel=\"icon\" href=\"laura.ico\">"));
    assert!(page.contains("src=\"logo.svg\" alt=\"Laura\""));
    // The top bar shows the logo, then "Workspaces", with no "/" between them.
    let css = http(s.port, "GET", "/style.css", "", "")?;
    assert!(body(&css).contains(".crumb"), "{css}");
    assert!(!body(&css).contains("content: \"/\""), "{css}");
    Ok(())
}

#[test]
fn a_workspace_shows_in_the_list_until_it_ends() -> Result<()> {
    let s = Server::start()?;
    let dir = temp()?;
    let (ws, url) = s.workspace(dir.path())?;
    until("the workspace's row", || Ok(s.record(&url)?.is_some()))?;
    let row = s.record(&url)?.expect("the row");
    let root = dir.path().canonicalize()?;
    assert_eq!(
        Path::new(row["path"].as_str().expect("a path")).canonicalize()?,
        root
    );
    let name = root.file_name().expect("a name").to_string_lossy();
    assert!(
        row["root"].as_str().is_some_and(|r| r.ends_with(&*name)),
        "{row}"
    );
    assert_eq!(row["program"], PROGRAM, "{row}");
    assert!(row["title"].is_null(), "{row}");
    assert!(row["started"].as_u64().is_some_and(|ms| ms > 0), "{row}");

    // A workspace that ends with no time to delete its record: the row goes too.
    drop(ws);
    until("the row goes", || Ok(s.record(&url)?.is_none()))
}

#[test]
fn a_new_workspace_on_the_same_port_replaces_the_old_row() -> Result<()> {
    let s = Server::start()?;
    let port = TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port()
        .to_string();
    let args = ["--web", "--port", &port, "--", PROGRAM];
    let (dir_a, dir_b) = (temp()?, temp()?);
    let (old, url_a) = laura_start(dir_a.path(), &args, Some(s.home.path()))?;
    until("the first workspace's row", || {
        Ok(s.record(&url_a)?.is_some())
    })?;
    // The first workspace ends with no time to delete its record.
    drop(old);
    let (_new, url_b) = laura_start(dir_b.path(), &args, Some(s.home.path()))?;
    until("the second workspace's row", || {
        Ok(s.record(&url_b)?.is_some())
    })?;
    let list = s.workspaces()?;
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["url"], url_b.as_str(), "{list:?}");
    Ok(())
}

#[test]
fn the_session_label_shows_in_the_list() -> Result<()> {
    let s = Server::start()?;
    let dir = temp()?;
    let (_ws, url) = s.workspace(dir.path())?;
    let mut view = view(&url)?;
    // The agent runs `laura ready --title` in the shell.
    let laura = env!("CARGO_BIN_EXE_laura");
    for title in ["one", "two"] {
        let line = format!("\"{laura}\" ready --session s --title {title}\r");
        let mut frame = 0u64.to_be_bytes().to_vec();
        frame.extend(line.as_bytes());
        view.send(Message::binary(frame))?;
        until("the session label", || {
            while read(&mut view).is_some() {}
            Ok(s.record(&url)?.is_some_and(|w| w["title"] == title))
        })?;
    }
    Ok(())
}

#[test]
fn the_laura_server_starts_a_workspace_that_outlives_it() -> Result<()> {
    let s = Server::start()?;
    let dir = temp()?;
    let (_stop, url) = s.new_workspace(dir.path())?;
    let row = s.record(&url)?.expect("the row");
    assert_eq!(
        Path::new(row["path"].as_str().expect("a path")).canonicalize()?,
        dir.path().canonicalize()?
    );
    assert_eq!(row["program"], PROGRAM, "{row}");
    drop(view(&url)?);
    // The workspace has no terminal and no pipe to the Laura server.
    drop(s.child);
    thread::sleep(Duration::from_millis(300));
    let mut ws = view(&url)?;
    until(
        "the workspace's first frame",
        || Ok(read(&mut ws).is_some()),
    )
}

#[test]
fn the_page_stops_the_laura_server_and_the_workspace_keeps_running() -> Result<()> {
    let mut s = Server::start()?;
    let dir = temp()?;
    let (_stop, url) = s.new_workspace(dir.path())?;
    let quit = s.post("/quit", json!({}))?;
    assert_eq!(status(&quit), "HTTP/1.1 200 OK", "{quit}");
    until("the Laura server ends", || {
        Ok(s.child.0.try_wait()?.is_some())
    })?;
    let mut ws = view(&url)?;
    until(
        "the workspace's first frame",
        || Ok(read(&mut ws).is_some()),
    )
}

#[test]
fn start_workspace_tells_why_a_workspace_did_not_start() -> Result<()> {
    let s = Server::start()?;
    let dir = temp()?;
    let missing = dir.path().join("nope");
    let resp = s.post("/new", json!({"folder": missing, "program": ""}))?;
    assert_eq!(status(&resp), "HTTP/1.1 400 Bad Request", "{resp}");
    assert_eq!(
        body(&resp),
        format!("{} is not a folder.", missing.display())
    );

    let resp = s.post(
        "/new",
        json!({"folder": dir.path(), "program": "no-such-program"}),
    )?;
    assert_eq!(status(&resp), "HTTP/1.1 400 Bad Request", "{resp}");
    let text = body(&resp);
    assert!(
        text.starts_with("The workspace failed to start.\n"),
        "{text}"
    );
    assert!(text.contains("no-such-program"), "{text}");

    // A program that ends at once.
    let program = if cfg!(windows) {
        "cmd.exe /c exit"
    } else {
        "true"
    };
    let resp = s.post("/new", json!({"folder": dir.path(), "program": program}))?;
    assert_eq!(status(&resp), "HTTP/1.1 400 Bad Request", "{resp}");
    assert_eq!(
        body(&resp),
        "The workspace failed to start.\nThe program exited at once."
    );
    assert!(s.workspaces()?.is_empty(), "a workspace started");
    Ok(())
}

#[test]
fn the_folder_browser_lists_folders_without_dot_folders() -> Result<()> {
    let s = Server::start()?;
    let dir = temp()?;
    for d in ["a/.git", "b", ".hidden"] {
        std::fs::create_dir_all(dir.path().join(d))?;
    }
    std::fs::write(dir.path().join("file.txt"), "")?;
    let path = dir
        .path()
        .to_string_lossy()
        .replace('\\', "%5C")
        .replace(' ', "%20");
    let resp = s.get(&format!("/dirs?path={path}"))?;
    assert_eq!(status(&resp), "HTTP/1.1 200 OK", "{resp}");
    let v: Value = serde_json::from_str(body(&resp))?;
    let dirs: Vec<(&str, bool)> = v["dirs"]
        .as_array()
        .expect("folders")
        .iter()
        .map(|d| (d["name"].as_str().unwrap_or(""), d["git"] == true))
        .collect();
    assert_eq!(dirs, [("a", true), ("b", false)], "{v}");
    let shown = Path::new(v["path"].as_str().expect("a path")).canonicalize()?;
    assert_eq!(shown, dir.path().canonicalize()?);
    // The breadcrumbs end at the folder, and an empty path is the home folder.
    let last = v["crumbs"]
        .as_array()
        .and_then(|c| c.last())
        .expect("crumbs");
    assert_eq!(last["path"], v["path"], "{v}");
    let home: Value = serde_json::from_str(body(&s.get("/dirs?path=")?))?;
    assert_eq!(home["shown"], "~", "{home}");

    let resp = s.get("/dirs?path=%2Fno%2Fsuch%2Ffolder")?;
    assert_eq!(status(&resp), "HTTP/1.1 404 Not Found", "{resp}");
    Ok(())
}

#[test]
fn serve_takes_no_program() {
    Command::cargo_bin("laura")
        .unwrap()
        .args(["--serve", "--port", "0", "--", "claude"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--serve takes no program"));
}

#[test]
fn a_second_laura_serve_prints_the_same_address() -> Result<()> {
    let s = Server::start()?;
    let port = s.port.to_string();
    let home = s.home.path();
    // One Laura server for each user: a different port prints the same address too.
    for other in [port.as_str(), "0"] {
        Command::cargo_bin("laura")?
            .args(["--serve", "--port", other])
            .env("HOME", home)
            .env("USERPROFILE", home)
            .assert()
            .success()
            .stdout(format!("http://127.0.0.1:{port}/?t={}\n", s.token));
    }
    Ok(())
}

#[test]
fn the_laura_server_makes_a_new_key_on_each_start() -> Result<()> {
    let Server {
        child, token, home, ..
    } = Server::start()?;
    drop(child);
    let s = Server::start_in(home)?;
    assert_ne!(s.token, token, "the key stayed");
    let old = format!("X-Laura-Key: {token}\r\n");
    let resp = http(s.port, "GET", "/workspaces", &old, "")?;
    assert_eq!(status(&resp), "HTTP/1.1 401 Unauthorized", "{resp}");
    Ok(())
}

#[test]
fn a_port_in_use_returns_an_error_that_names_the_port() -> Result<()> {
    let taken = TcpListener::bind("127.0.0.1:0")?;
    let port = taken.local_addr()?.port().to_string();
    // An own home folder: a Laura server of the developer does not change the result.
    let home = temp()?;
    for mode in ["--serve", "--web"] {
        Command::cargo_bin("laura")
            .unwrap()
            .args([mode, "--port", &port])
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .assert()
            .failure()
            .stderr(predicates::str::contains(format!(
                "port {port} is in use: pass --port"
            )));
    }
    Ok(())
}

/// Read `view` until a `workspaces` frame holds `want`; the frame.
fn workspaces_frame(view: &mut Ws, what: &str, want: impl Fn(&Value) -> bool) -> Result<Value> {
    let mut found = None;
    until(what, || {
        while let Some(f) = read(view) {
            assert!(!matches!(f, Frame::Closed), "the view closed: {what}");
            if f.is("workspaces") {
                found = Some(f.json()).filter(&want);
            }
        }
        Ok(found.is_some())
    })?;
    Ok(found.expect("until found the frame"))
}

#[test]
fn a_view_lists_the_running_workspaces_oldest_first() -> Result<()> {
    let home = temp()?;
    let (dir_a, dir_b) = (temp()?, temp()?);
    let args = ["--web", "--", PROGRAM];
    let (_a, url_a) = laura_start(dir_a.path(), &args, Some(home.path()))?;
    let mut view = view(&url_a)?;
    workspaces_frame(&mut view, "workspace A alone", |f| {
        f["list"].as_array().is_some_and(|l| l.len() == 1)
    })?;
    // The records order by the start time in ms: B starts later.
    thread::sleep(Duration::from_millis(20));
    let (b, url_b) = laura_start(dir_b.path(), &args, Some(home.path()))?;
    let frame = workspaces_frame(&mut view, "workspaces A and B", |f| {
        f["list"].as_array().is_some_and(|l| l.len() == 2)
    })?;
    let list = &frame["list"];
    assert_eq!(list[0]["url"], url_a.as_str(), "{frame}");
    assert_eq!(list[0]["current"], true, "{frame}");
    assert_eq!(list[1]["url"], url_b.as_str(), "{frame}");
    assert_eq!(list[1]["current"], false, "{frame}");
    assert_eq!(list[1]["program"], PROGRAM, "{frame}");
    let name = dir_b.path().canonicalize()?;
    let name = name.file_name().expect("a name").to_string_lossy();
    assert!(
        list[1]["root"]
            .as_str()
            .is_some_and(|r| r.ends_with(&*name)),
        "{frame}"
    );
    assert!(list[1]["title"].is_null(), "{frame}");

    drop(b);
    let frame = workspaces_frame(&mut view, "workspace B goes", |f| {
        f["list"].as_array().is_some_and(|l| l.len() == 1)
    })?;
    assert_eq!(frame["list"][0]["url"], url_a.as_str(), "{frame}");
    Ok(())
}

#[test]
fn a_view_learns_the_laura_servers_address_when_it_starts() -> Result<()> {
    let home = temp()?;
    let dir = temp()?;
    // A different program on the port of an old Laura server is no Laura server.
    let other = TcpListener::bind("127.0.0.1:0")?;
    let state = json!({"token": "0".repeat(64), "port": other.local_addr()?.port()});
    std::fs::create_dir_all(home.path().join(".laura"))?;
    std::fs::write(home.path().join(".laura/serve.json"), state.to_string())?;
    let args = ["--web", "--", PROGRAM];
    let (_ws, url) = laura_start(dir.path(), &args, Some(home.path()))?;
    let mut view = view(&url)?;
    let frame = workspaces_frame(&mut view, "the first frame", |_| true)?;
    assert!(frame["server"].is_null(), "{frame}");

    let s = Server::start_in(home)?;
    let server = format!("http://127.0.0.1:{}/?t={}", s.port, s.token);
    // A new view gets the kept frame too.
    workspaces_frame(&mut view, "the Laura server's address", |f| {
        f["server"] == server.as_str()
    })?;
    let mut second = self::view(&url)?;
    workspaces_frame(&mut second, "the kept frame", |f| {
        f["server"] == server.as_str()
    })?;
    Ok(())
}
