//! Links in a file pane and their notices.

use super::*;

#[test]
fn a_heading_link_moves_the_cursor() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) =
        image_doc("# A\n\n[s](#setup)\n\n## Setup\n\nrun it\n\n## Two {#Install}\n")?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "#setup"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.panels[&1].cursor_line(), 5);
    rig.tab
        .panels
        .get_mut(&1)
        .expect("pane 1 is open")
        .set_cursor_line(1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "#Setup"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.panels[&1].cursor_line(), 5);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "#install"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.panels[&1].cursor_line(), 9);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "#nope"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    rig.until(&mut ws, "heading notice", |f| {
        matches!(f, Frame::Json(v) if v["t"] == "notice" && v["pane"] == 1
            && v["text"] == "⚠ The file a.md has no heading #nope.")
    });
    Ok(())
}

#[test]
fn a_link_to_a_missing_file_shows_a_notice() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = image_doc("[n](nope.md)\n")?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "nope.md"}),
    );
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(
        r["text"], "⚠ The link target nope.md does not exist.",
        "{r}"
    );
    assert!(!rig.tab.panels.contains_key(&2));
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "../img/"}),
    );
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(r["text"], "⚠ The link target ../img/ is a folder.", "{r}");
    assert!(!rig.tab.panels.contains_key(&2));
    // A rooted path to a real file: the full path on Unix, drive-less on Windows.
    let rooted = &path[path.find(['/', '\\']).unwrap_or(0)..];
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": rooted}),
    );
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(
        r["text"],
        format!("⚠ The link target {rooted} does not exist."),
        "{r}"
    );
    assert!(!rig.tab.panels.contains_key(&2));
    Ok(())
}

#[test]
fn a_link_in_a_full_workspace_shows_a_notice() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, path) = image_doc("[x](x.md)\n")?;
    std::fs::write(dir.path().join("docs/x.md"), "x\n")?;
    let y = dir.path().join("y.txt");
    std::fs::write(&y, "y\n")?;
    rig.drive(&["open", &path]);
    pane_frame(&mut rig, &mut ws, 1);
    let mut stderr = String::new();
    for n in 0..40 {
        let f = dir.path().join(format!("f{n}.txt"));
        std::fs::write(&f, "f\n")?;
        let out = rig.run(&["open", &f.to_string_lossy()]);
        if !out.status.success() {
            stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            break;
        }
    }
    // The agent keeps its own text.
    assert!(stderr.contains("lower --ratio"), "{stderr}");
    let panes = rig.tab.panels.len();
    let no_room = "⚠ No room for another pane. Close a pane, then click the link again.";
    let r = rig.act(
        &mut ws,
        json!({"act": "open_path", "pane": 0, "path": y.to_string_lossy()}),
    );
    assert_eq!(
        (r["ok"].clone(), r["text"].clone()),
        (json!(false), json!(no_room))
    );
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "x.md"}),
    );
    assert_eq!(
        (r["ok"].clone(), r["text"].clone()),
        (json!(false), json!(no_room))
    );
    assert_eq!(rig.tab.panels.len(), panes);
    Ok(())
}

#[test]
fn a_link_to_a_file_in_an_editor_pane_focuses_it() -> Result<()> {
    let mut rig = Rig::new()?;
    rig.tab.editor = Ok(fake_nvim());
    let mut ws = rig.connect();
    let (dir, path) = image_doc("[b](b.md#two)\n")?;
    let b = dir.path().join("docs/b.md");
    std::fs::write(&b, "# B\n\n## Two\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["open", "--edit", &b.to_string_lossy()]);
    pane_frame(&mut rig, &mut ws, 1);
    rig.tab.focus = 1;
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "b.md#two"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.focus, 2);
    assert_eq!(rig.tab.panels[&2].cursor_line(), 3);
    Ok(())
}

#[test]
fn a_missing_heading_in_another_pane_shows_its_notice_there() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, path) = image_doc("[b](b.md#nope)\n")?;
    let b = dir.path().join("docs/b.md");
    std::fs::write(&b, "# B\n")?;
    rig.drive(&["open", &path]);
    rig.drive(&["open", &b.to_string_lossy()]);
    pane_frame(&mut rig, &mut ws, 1);
    let r = rig.act(
        &mut ws,
        json!({"act": "open_link", "pane": 1, "href": "b.md#nope"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(rig.tab.focus, 2);
    rig.until(&mut ws, "heading notice in pane 2", |f| {
        matches!(f, Frame::Json(v) if v["t"] == "notice" && v["pane"] == 2
            && v["text"] == "⚠ The file b.md has no heading #nope.")
    });
    Ok(())
}
