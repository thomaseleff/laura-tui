//! The project at a glance (#214): after the user's first submitted inline review, the start
//! canvas shows the folder's README, its git changes or latest commit, and the recent sessions in
//! the folder. With nothing to show, the canvas is the calm canvas.

use super::*;

/// The first `glance` frame and the first `layout` with `canvas`, in either order: a thread reads
/// the glance, so its frame can come after the first layout. `None` when the layout comes and no
/// glance came before it, for `canvas` other than `glance`.
fn first_frames(ws: &mut Ws, canvas: &str) -> (Option<Value>, Value) {
    let start = Instant::now();
    let (mut glance, mut layout) = (None, None);
    loop {
        match read(ws) {
            Some(Frame::Json(v)) if v["t"] == "glance" && glance.is_none() => glance = Some(v),
            Some(Frame::Json(v)) if v["t"] == "layout" && v["canvas"] == canvas => {
                layout.get_or_insert(v);
            }
            _ => {}
        }
        if let Some(l) = &layout
            && (glance.is_some() || canvas != "glance")
        {
            return (glance, l.clone());
        }
        assert!(
            start.elapsed() < common::DEADLINE,
            "no layout with {canvas}"
        );
    }
}

/// `laura --web` in `dir` after a submitted inline review: its glance frame.
fn glance_in(dir: &Path, home: &Path) -> Result<(Kill, Value)> {
    journal_review(home, false)?;
    let (child, mut ws) = laura_web_at(dir, home)?;
    let (glance, _) = first_frames(&mut ws, "glance");
    Ok((child, glance.expect("a glance frame")))
}

/// `path` as Laura names its folder: canonical, without Windows' `\\?\`.
fn canonical(path: &Path) -> String {
    let p = path.canonicalize().expect("the folder exists");
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

/// The file row for `path`.
fn row<'a>(g: &'a Value, path: &str) -> &'a Value {
    g["project"]["files"]
        .as_array()
        .and_then(|f| f.iter().find(|f| f["path"] == path))
        .unwrap_or_else(|| panic!("no row for {path}: {g}"))
}

#[test]
fn a_changed_file_shows_with_its_counts() -> Result<()> {
    let home = tempfile::tempdir()?;
    let (dir, path) = changed_file("a.rs", "one\ntwo\n", "one\nTWO\n")?;
    std::fs::write(dir.path().join("b.txt"), "new\n")?;
    let (_child, g) = glance_in(dir.path(), home.path())?;
    assert_eq!(g["project"]["kind"], "changes", "{g}");
    assert!(
        g["project"]["branch"]
            .as_str()
            .is_some_and(|b| !b.is_empty()),
        "{g}"
    );
    assert_eq!(
        *row(&g, "a.rs"),
        json!({"status": "M", "path": "a.rs", "add": 1, "del": 1}),
        "{path}"
    );
    assert_eq!(row(&g, "b.txt")["status"], "?", "{g}");
    Ok(())
}

#[test]
fn a_clean_tree_shows_the_latest_commit() -> Result<()> {
    let home = tempfile::tempdir()?;
    let (dir, _) = file("a.rs", "one\ntwo\n")?;
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["add", "a.rs"]);
    git(dir.path(), &["commit", "-q", "-m", "Add the parser"]);
    let (_child, g) = glance_in(dir.path(), home.path())?;
    let p = &g["project"];
    assert_eq!(p["kind"], "latest", "{g}");
    assert_eq!(p["subject"], "Add the parser", "{g}");
    assert_eq!(p["author"], "t", "{g}");
    assert_eq!(p["hash"].as_str().map(str::len), Some(7), "{g}");
    assert_eq!(
        *row(&g, "a.rs"),
        json!({"status": "A", "path": "a.rs", "add": 2, "del": 0}),
    );
    Ok(())
}

#[test]
fn a_merge_commit_lists_its_files() -> Result<()> {
    let home = tempfile::tempdir()?;
    let (dir, _) = file("a.rs", "one\n")?;
    let d = dir.path();
    git(d, &["init", "-q", "-b", "main"]);
    git(d, &["add", "a.rs"]);
    git(d, &["commit", "-q", "-m", "Start"]);
    git(d, &["checkout", "-q", "-b", "side"]);
    std::fs::write(d.join("b"), "b\n")?;
    git(d, &["add", "b"]);
    git(d, &["commit", "-q", "-m", "Add b"]);
    git(d, &["checkout", "-q", "main"]);
    std::fs::write(d.join("c"), "c\n")?;
    git(d, &["add", "c"]);
    git(d, &["commit", "-q", "-m", "Add c"]);
    git(d, &["merge", "-q", "--no-edit", "side"]);
    let (_child, g) = glance_in(d, home.path())?;
    assert_eq!(g["project"]["kind"], "latest", "{g}");
    assert_eq!(
        *row(&g, "b"),
        json!({"status": "A", "path": "b", "add": 1, "del": 0}),
    );
    Ok(())
}

#[test]
fn a_folder_without_git_shows_the_readme_only() -> Result<()> {
    let home = tempfile::tempdir()?;
    let dir = tempfile::tempdir()?;
    // Any case of the name, a setext title, and HTML that a page would show as its source.
    let readme = "<p align=\"center\">x</p>\n\nRidgeline\n=========\n\nPlan a day hike. [d](#data)\n\n\
                  ## Data\n\nThe tracks.\n\n## Deploy\n\nThe last line.\n";
    std::fs::write(dir.path().join("Readme.md"), readme)?;
    let (_child, g) = glance_in(dir.path(), home.path())?;
    assert_eq!(g["readme"]["name"], "Readme.md", "{g}");
    let html = g["readme"]["html"].as_str().unwrap_or("");
    assert!(
        html.contains("<h1") && html.contains("Plan a day hike."),
        "{g}"
    );
    assert!(!html.contains("&lt;p") && !html.contains("<p align"), "{g}");
    // The canvas has no README pane, so a heading link names the README.
    assert!(html.contains(r#"data-href="Readme.md#data""#), "{g}");
    assert!(html.contains("The last line."), "the whole README: {g}");
    assert_eq!(g["project"], Value::Null, "{g}");
    assert_eq!(g["sessions"], json!([]), "{g}");
    Ok(())
}

#[test]
fn a_session_in_the_folder_shows_its_label() -> Result<()> {
    let home = tempfile::tempdir()?;
    let dir = tempfile::tempdir()?;
    let other = tempfile::tempdir()?;
    for f in ["a.md", "b.md"] {
        std::fs::write(dir.path().join(f), "x\n")?;
    }
    std::fs::write(other.path().join("c.md"), "x\n")?;
    let (root, elsewhere) = (canonical(dir.path()), canonical(other.path()));
    let at = |f: &str| dir.path().join(f).to_string_lossy().into_owned();
    let sessions = home.path().join(".laura/sessions");
    std::fs::create_dir_all(&sessions)?;
    let write = |name: &str, events: &[Value]| {
        let body: String = events.iter().map(|e| format!("{e}\n")).collect();
        std::fs::write(sessions.join(name), body)
    };
    write(
        "now.ndjson",
        &[
            json!({"type": "ready", "ts": 3000, "root": root, "title": "flat profile"}),
            json!({"type": "open", "ts": 3001, "path": at("a.md")}),
            json!({"type": "review", "ts": 3002, "path": at("a.md")}),
        ],
    )?;
    // From a Laura before `root`: the session opened a file in the folder.
    write(
        "old.ndjson",
        &[
            json!({"type": "ready", "ts": 1000, "title": "gpx import"}),
            json!({"type": "open", "ts": 1001, "path": at("b.md")}),
        ],
    )?;
    write(
        "other.ndjson",
        &[
            json!({"type": "ready", "ts": 5000, "root": elsewhere, "title": "elsewhere"}),
            json!({"type": "open", "ts": 5001, "path": other.path().join("c.md")}),
        ],
    )?;
    let (_child, g) = glance_in(dir.path(), home.path())?;
    assert_eq!(
        g["sessions"],
        json!([
            {"label": "flat profile", "ts": 3002, "files": ["a.md"], "reviews": 1},
            {"label": "gpx import", "ts": 1001, "files": ["b.md"], "reviews": 0},
        ]),
        "{g}"
    );
    Ok(())
}

#[test]
fn an_empty_folder_gives_the_calm_canvas() -> Result<()> {
    let home = tempfile::tempdir()?;
    let dir = tempfile::tempdir()?;
    journal_review(home.path(), false)?;
    let (_child, mut ws) = laura_web_at(dir.path(), home.path())?;
    let (glance, _) = first_frames(&mut ws, "calm");
    assert_eq!(glance, None);
    Ok(())
}

/// A README link on the canvas opens its file, from Laura's folder, at its heading.
#[test]
fn a_readme_link_opens_its_file_at_the_heading() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 0, "href": "docs/navigation.md#the-drawer"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    let nav = std::fs::read_to_string("docs/navigation.md")?;
    let line = nav
        .lines()
        .position(|l| l == "## The drawer")
        .expect("the heading")
        + 1;
    assert_eq!(rig.tab.panels[&1].cursor_line() as usize, line);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 0, "href": "#x"}),
    );
    assert_eq!(r["text"], "⚠ The link target #x does not exist.", "{r}");
    Ok(())
}
