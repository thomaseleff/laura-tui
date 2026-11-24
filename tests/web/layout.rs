//! The layout frame: the panes' rects, the drawer and dock, the start canvas, the drawer's saved
//! and start heights, the fit of a split, the icons, the hint line and the help.

use super::*;

#[test]
fn the_layout_describes_each_pane() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = changed_file(
        "a.rs",
        &five_lines(),
        &five_lines().replace("line 2", "line 2 CHANGED"),
    )?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.drive(&["highlight", "--pane", "1", "2", "3"]);
    let acted = rig.act(&mut ws, json!({"act": "toggle_diff", "pane": 1}));
    assert_eq!(acted["ok"], true, "{acted}");
    let layout = rig
        .until(
            &mut ws,
            "diff view",
            |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"][1]["diff"] == true),
        )
        .json();
    let p = &layout["panes"][1];
    assert_eq!(p["name"], "a.rs", "{p}");
    assert!(
        p["path"].as_str().is_some_and(|s| s.ends_with("a.rs")),
        "{p}"
    );
    assert_eq!(p["added"], 1, "{p}");
    assert_eq!(p["removed"], 1, "{p}");
    assert_eq!(p["highlight"], json!([2, 3]), "{p}");
    assert_eq!(layout["panes"][0]["name"], "shell");
    Ok(())
}

/// The shell takes the `--agent` name of the latest `laura ready`, and is `shell` before it.
#[test]
fn the_shell_takes_the_agents_name() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let layout = rig.until(&mut ws, "layout", |f| f.is("layout")).json();
    assert_eq!(layout["panes"][0]["name"], "shell", "{layout}");
    rig.drive(&["ready", "--agent", "claude"]);
    rig.until(
        &mut ws,
        "the agent's name",
        |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"][0]["name"] == "claude"),
    );
    Ok(())
}

/// Pane `id`'s entry in a `layout` frame.
fn pane_of(layout: &Value, id: u64) -> &Value {
    layout["panes"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["id"] == id))
        .unwrap_or(&Value::Null)
}

/// Pane `id`'s cells in a `layout` frame: x, y, w, h.
fn cells(layout: &Value, id: u64) -> [u64; 4] {
    let p = pane_of(layout, id);
    ["x", "y", "w", "h"].map(|k| p[k].as_u64().unwrap_or(u64::MAX))
}

/// #187: the panes fill the workspace and the shell floats in the drawer, from the start (#213).
/// The drawer follows the focus in the web app.
#[test]
fn a_pane_fills_the_workspace_and_the_shell_floats_in_the_drawer() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let first = layout_when(&mut rig, &mut ws, "layout", |_| true);
    assert_eq!(pane_of(&first, 0)["drawer"], true, "{first}");
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    let layout = layout_when(&mut rig, &mut ws, "pane 1", |v| v["focus"] == 1);
    let (cols, rows) = (layout["cols"].as_u64(), layout["rows"].as_u64());
    let whole = [0, 0, cols.unwrap_or(0), rows.unwrap_or(0)];
    assert_eq!(cells(&layout, 1), whole, "{layout}");
    assert_eq!(cells(&layout, 0), whole, "{layout}");
    assert_eq!(pane_of(&layout, 0)["drawer"], true, "{layout}");
    let report: Value = serde_json::from_str(&rig.drive(&["layout"]))?;
    for id in [0, 1] {
        let r = &report["panes"][id]["rect"];
        assert_eq!((&r["x"], &r["y"]), (&json!(0), &json!(0)), "{report}");
        assert_eq!(
            (&r["width"], &r["height"]),
            (&json!(cols), &json!(rows)),
            "{report}"
        );
    }
    assert_eq!(report["panes"][0]["drawer"], true, "{report}");
    assert!(report["panes"][1].get("drawer").is_none(), "{report}");
    rig.act(&mut ws, json!({"act": "focus", "pane": 0}));
    let open = layout_when(&mut rig, &mut ws, "the shell", |v| v["focus"] == 0);
    assert_eq!(
        open["hints"]["pane"][0],
        json!(["Ctrl+`", "hide the shell"]),
        "{open}"
    );
    assert_eq!(
        open["hints"]["pane"][1],
        json!(["Ctrl+Shift+↑ ↓", "drawer height"]),
        "{open}"
    );
    rig.act(&mut ws, json!({"act": "focus", "pane": 1}));
    layout_when(&mut rig, &mut ws, "the file pane", |v| v["focus"] == 1);
    Ok(())
}

/// **Show the shell as a pane** shows the shell as the left-most pane, at the full height, with
/// 40% of the width; **Show the shell in a drawer** puts the shell back. Laura keeps the choice
/// for the workspace.
#[test]
fn dock_shows_the_shell_as_a_pane_and_drawer_puts_it_back() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    let layout = layout_when(&mut rig, &mut ws, "pane 1", |v| v["focus"] == 1);
    let (cols, rows) = (
        layout["cols"].as_u64().unwrap_or(0),
        layout["rows"].as_u64().unwrap_or(0),
    );
    let acted = rig.act(&mut ws, json!({"act": "dock"}));
    assert_eq!(acted["ok"], true, "{acted}");
    let docked = layout_when(&mut rig, &mut ws, "docked", |v| {
        v["panes"][0]["drawer"] == false
    });
    let w = cols * 40 / 100;
    assert_eq!(cells(&docked, 0), [0, 0, w, rows], "{docked}");
    assert_eq!(cells(&docked, 1), [w, 0, cols - w, rows], "{docked}");
    let report: Value = serde_json::from_str(&rig.drive(&["layout"]))?;
    assert!(report["panes"][0].get("drawer").is_none(), "{report}");
    // The choice outlasts the last pane, and the next pane takes the canvas's place.
    rig.drive(&["close", "1"]);
    rig.drive(&["open", &path]);
    let again = layout_when(&mut rig, &mut ws, "pane 2", |v| v["focus"] == 2);
    assert_eq!(pane_of(&again, 0)["drawer"], false, "{again}");
    assert_eq!(cells(&again, 0)[2], w, "{again}");
    rig.act(&mut ws, json!({"act": "drawer"}));
    let back = layout_when(&mut rig, &mut ws, "drawer", |v| {
        v["panes"][0]["drawer"] == true
    });
    assert_eq!(cells(&back, 2), [0, 0, cols, rows], "{back}");
    Ok(())
}

/// A drag of the drawer's grip saves the height in `~/.laura/settings.json`, and a workspace that
/// starts later shows the drawer at that height; Laura keeps it within 20–85%, and a reset removes
/// it. One test: every web test shares `settings.json`.
#[test]
fn the_drawer_height_is_saved_for_every_workspace() -> Result<()> {
    let mut a = Rig::new()?;
    let mut ws = a.connect();
    let saved = || -> Value {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(".laura/settings.json");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .map_or(Value::Null, |v| v["drawer"].clone())
    };
    // `percent` null: a double-click on the grip, which resets the height.
    let mut set = |percent: Value, shown: u64| {
        let acted = a.act(&mut ws, json!({"act": "drawer_height", "percent": percent}));
        assert_eq!(acted["ok"], true, "{acted}");
        layout_when(&mut a, &mut ws, "the height", |v| {
            v["drawer_height"] == shown
        });
    };

    set(json!(40), 40);
    assert_eq!(saved(), 40);
    let mut b = Rig::new()?;
    let mut wb = b.connect();
    let first = b.until(&mut wb, "layout", |f| f.is("layout")).json();
    assert_eq!(first["drawer_height"], 40, "{first}");

    set(json!(95), 85);
    assert_eq!(saved(), 85);
    set(json!(5), 20);
    assert_eq!(saved(), 20);

    // The reset also leaves the shared file clean for later runs. Over the canvas, the drawer
    // goes back to the start height.
    set(Value::Null, 36);
    assert_eq!(saved(), Value::Null);

    // A step and a reset in one loop end at the height that Laura showed: Laura sends the layout
    // again, so the web app drops the step that it showed.
    send(&mut ws, json!({"act": "drawer_height", "percent": 59}));
    send(&mut ws, json!({"act": "drawer_height", "percent": null}));
    thread::sleep(Duration::from_millis(100));
    layout_when(&mut a, &mut ws, "the start height", |v| {
        v["drawer_height"] == 36
    });
    assert_eq!(saved(), Value::Null);
    Ok(())
}

/// An earlier Laura's `~/.laura/editor-theme` moves into `settings.json` at start, so the user
/// doesn't get the wizard again.
#[test]
fn an_earlier_editor_theme_moves_into_the_settings() -> Result<()> {
    let home = tempfile::tempdir()?;
    let dir = home.path().join(".laura");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("editor-theme"), "laura\n")?;
    let (_child, _url) = laura_start(home.path(), &["--", PROGRAM], Some(home.path()))?;
    let start = Instant::now();
    loop {
        let saved = std::fs::read_to_string(dir.join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok());
        if saved.is_some_and(|v| v["editor_theme"] == "laura") && !dir.join("editor-theme").exists()
        {
            return Ok(());
        }
        assert!(start.elapsed() < common::DEADLINE, "the theme never moved");
        thread::sleep(Duration::from_millis(50));
    }
}

/// A `settings.json` that Laura can't parse, for example after a typo in a hand edit: Laura shows
/// the start height, a drag applies to this workspace only, and the file keeps the user's keys.
#[test]
fn an_invalid_settings_file_stays_as_the_user_left_it() -> Result<()> {
    let home = tempfile::tempdir()?;
    let dir = home.path().join(".laura");
    std::fs::create_dir_all(&dir)?;
    let typo = r#"{"drawer":60,"editor_theme":"laura",}"#;
    std::fs::write(dir.join("settings.json"), typo)?;
    let (_child, mut ws) = laura_web_in(home.path())?;
    let first = raw_layout_when(&mut ws, "a height", |v| !v["drawer_height"].is_null());
    assert_eq!(first["drawer_height"], 36, "{first}");
    send(&mut ws, json!({"act": "drawer_height", "percent": 40}));
    raw_layout_when(&mut ws, "the height 40", |v| v["drawer_height"] == 40);
    assert_eq!(std::fs::read_to_string(dir.join("settings.json"))?, typo);
    Ok(())
}

/// Two Lauras that save a setting at the same moment keep each other's keys: a save waits for
/// another Laura's save, here the test's lock on `settings.lock`.
#[test]
fn a_save_waits_for_another_lauras_save() -> Result<()> {
    let home = tempfile::tempdir()?;
    let dir = home.path().join(".laura");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("settings.json");
    std::fs::write(&file, r#"{"editor_theme":"neovim"}"#)?;
    let lock = std::fs::File::create(dir.join("settings.lock"))?;
    lock.lock()?;
    let (_child, mut ws) = laura_web_in(home.path())?;
    raw_layout_when(&mut ws, "a height", |v| !v["drawer_height"].is_null());
    send(&mut ws, json!({"act": "drawer_height", "percent": 40}));
    let saved = || -> Value {
        std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null)
    };
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(300) {
        let _ = read(&mut ws);
        assert_eq!(saved(), json!({"editor_theme": "neovim"}));
    }
    drop(lock);
    let start = Instant::now();
    while saved() != json!({"editor_theme": "neovim", "drawer": 40}) {
        let _ = read(&mut ws);
        assert!(
            start.elapsed() < common::DEADLINE,
            "never saved: {}",
            saved()
        );
    }
    Ok(())
}

/// The start height: 36% or 18 rows of the shell, at most 54%, which the web app measures.
fn start_fit() -> Value {
    json!({"rows": 18, "max": 54})
}

/// #213: a new web workspace shows the shell in the drawer over *Get started*, at the start height,
/// until the user's first submitted inline review; a failed submit doesn't count. The first pane
/// takes the canvas's place at the usual height, and the last close brings the canvas back.
#[test]
fn a_new_workspace_starts_in_the_drawer_over_the_start_canvas() -> Result<()> {
    let home = tempfile::tempdir()?;
    journal_review(home.path(), true)?;
    let (_child, mut ws) = laura_web_in(home.path())?;
    let first = raw_layout_when(&mut ws, "a layout", |_| true);
    assert_eq!(pane_of(&first, 0)["drawer"], true, "{first}");
    assert_eq!(first["canvas"], "start", "{first}");
    assert_eq!(first["drawer_height"], 36, "{first}");
    assert_eq!(first["drawer_fit"], start_fit(), "{first}");
    // Over the canvas, the drawer stays open: no `Ctrl+`` to hide it.
    assert_eq!(
        first["hints"]["pane"][0],
        json!(["Ctrl+Shift+↑ ↓", "drawer height"]),
        "{first}"
    );
    // The user clicks a path in the shell.
    std::fs::write(home.path().join("a.rs"), five_lines())?;
    send(&mut ws, json!({"act": "open_path", "path": "a.rs"}));
    let open = raw_layout_when(&mut ws, "pane 1", |v| !pane_of(v, 1).is_null());
    assert_eq!(open["canvas"], Value::Null, "{open}");
    assert_eq!(open["drawer_fit"], Value::Null, "{open}");
    assert_eq!(open["drawer_height"], 54, "{open}");
    send(&mut ws, json!({"act": "close", "pane": 1}));
    let back = raw_layout_when(&mut ws, "the canvas", |v| pane_of(v, 1).is_null());
    assert_eq!(back["canvas"], "start", "{back}");
    assert_eq!(back["drawer_fit"], start_fit(), "{back}");
    Ok(())
}

/// After the user's first submitted inline review, in any folder, a new workspace shows the calm
/// canvas.
#[test]
fn a_workspace_after_a_review_starts_over_the_calm_canvas() -> Result<()> {
    let home = tempfile::tempdir()?;
    journal_review(home.path(), false)?;
    let (_child, mut ws) = laura_web_in(home.path())?;
    // The first layout can show the glance while Laura reads the folder.
    let calm = raw_layout_when(&mut ws, "the calm canvas", |l| l["canvas"] == "calm");
    assert_eq!(calm["drawer_fit"], start_fit(), "{calm}");
    Ok(())
}

/// A height the user set applies over the canvas too: no start height.
#[test]
fn a_saved_drawer_height_replaces_the_start_height() -> Result<()> {
    let home = tempfile::tempdir()?;
    let dir = home.path().join(".laura");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("settings.json"), r#"{"drawer":60}"#)?;
    let (_child, mut ws) = laura_web_in(home.path())?;
    let first = raw_layout_when(&mut ws, "a layout", |_| true);
    assert_eq!(first["canvas"], "start", "{first}");
    assert_eq!(first["drawer_height"], 60, "{first}");
    assert_eq!(first["drawer_fit"], Value::Null, "{first}");
    Ok(())
}

/// **Show the shell as a pane** works over the canvas: the shell takes the left 40%. The agent's
/// next `open` without split flags puts the pane in the canvas's place, and `--dry-run` says so.
#[test]
fn dock_over_the_canvas_gives_the_next_pane_its_place() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let first = layout_when(&mut rig, &mut ws, "layout", |_| true);
    let (cols, rows) = (
        first["cols"].as_u64().unwrap_or(0),
        first["rows"].as_u64().unwrap_or(0),
    );
    let acted = rig.act(&mut ws, json!({"act": "dock"}));
    assert_eq!(acted["ok"], true, "{acted}");
    let docked = layout_when(&mut rig, &mut ws, "docked", |v| {
        v["panes"][0]["drawer"] == false
    });
    let w = cols * 40 / 100;
    assert_eq!(cells(&docked, 0), [0, 0, w, rows], "{docked}");
    assert!(!docked["canvas"].is_null(), "{docked}");
    let (_dir, path) = file("a.rs", &five_lines())?;
    let dry: Value = serde_json::from_str(&rig.drive(&["open", &path, "--dry-run"]))?;
    rig.drive(&["open", &path]);
    let report: Value = serde_json::from_str(&rig.drive(&["layout"]))?;
    let rect = |r: &Value, id: usize| r["panes"][id]["rect"].clone();
    assert_eq!(
        rect(&report, 1),
        json!({"x": w, "y": 0, "width": cols - w, "height": rows}),
        "{report}"
    );
    assert_eq!(rect(&dry, 1), rect(&report, 1), "{dry}");
    assert_eq!(rect(&dry, 0), rect(&report, 0), "{dry}");
    Ok(())
}

/// The shell docks only when every pane still fits beside the shell.
#[test]
fn dock_needs_room_for_every_pane() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    send(&mut ws, json!({"view": {"cols": 40, "rows": 20}}));
    rig.until(
        &mut ws,
        "resized layout",
        |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["cols"] == 40),
    );
    let (_a, a) = file("a.rs", &five_lines())?;
    let (_b, b) = file("b.rs", &five_lines())?;
    rig.drive(&["open", &a]);
    // Pane 2 gets 4 of the 40 columns, and would get 2 beside the shell.
    rig.drive(&["open", &b, "--split", "1", "--dir", "h", "--ratio", "8"]);
    layout_when(&mut rig, &mut ws, "pane 2", |v| v["focus"] == 2);
    let acted = rig.act(&mut ws, json!({"act": "dock"}));
    assert_eq!(acted["ok"], false, "{acted}");
    assert_eq!(acted["text"], NO_ROOM_TO_DOCK, "{acted}");
    let report: Value = serde_json::from_str(&rig.drive(&["layout"]))?;
    assert_eq!(report["panes"][0]["drawer"], true, "{report}");
    Ok(())
}

/// A close doesn't open the drawer: Laura focuses the first other pane, and with none left the
/// shell takes the focus, in the drawer over the canvas.
#[test]
fn a_close_in_drawer_mode_focuses_another_pane() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_a, a) = file("a.rs", &five_lines())?;
    let (_b, b) = file("b.rs", &five_lines())?;
    rig.drive(&["open", &a]);
    rig.drive(&["open", &b]);
    layout_when(&mut rig, &mut ws, "pane 2", |v| v["focus"] == 2);
    rig.act(&mut ws, json!({"act": "close", "pane": 2}));
    let one = layout_when(&mut rig, &mut ws, "one pane", |v| {
        v["panes"].as_array().is_some_and(|p| p.len() == 2)
    });
    assert_eq!(one["focus"], 1, "{one}");
    rig.act(&mut ws, json!({"act": "close", "pane": 1}));
    let alone = layout_when(&mut rig, &mut ws, "the shell alone", |v| {
        v["panes"].as_array().is_some_and(|p| p.len() == 1)
    });
    assert_eq!(alone["focus"], 0, "{alone}");
    assert_eq!(pane_of(&alone, 0)["drawer"], true, "{alone}");
    let whole = [
        0,
        0,
        alone["cols"].as_u64().unwrap_or(0),
        alone["rows"].as_u64().unwrap_or(0),
    ];
    assert_eq!(cells(&alone, 0), whole, "{alone}");
    Ok(())
}

/// In drawer mode, Laura tiles as if the shell weren't there: a second open dwindles across the
/// whole area, and a split of the shell splits the first other pane.
#[test]
fn drawer_mode_tiles_without_the_shell() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_a, a) = file("a.rs", &five_lines())?;
    let (_b, b) = file("b.rs", &five_lines())?;
    rig.drive(&["open", &a]);
    rig.drive(&["open", &b]);
    let two = layout_when(&mut rig, &mut ws, "pane 2", |v| v["focus"] == 2);
    let (cols, rows) = (
        two["cols"].as_u64().unwrap_or(0),
        two["rows"].as_u64().unwrap_or(0),
    );
    assert_eq!(cells(&two, 1), [0, 0, cols / 2, rows], "{two}");
    assert_eq!(
        cells(&two, 2),
        [cols / 2, 0, cols - cols / 2, rows],
        "{two}"
    );
    // The focus on the shell is the open drawer; `--split 0` names the shell too.
    rig.drive(&["focus", "0"]);
    rig.drive(&["open", &a, "--dir", "h"]);
    let three = layout_when(&mut rig, &mut ws, "pane 3", |v| v["focus"] == 3);
    assert_eq!(
        cells(&three, 1)[2] + cells(&three, 3)[2],
        cols / 2,
        "{three}"
    );
    assert_eq!(cells(&three, 2), cells(&two, 2), "{three}");
    rig.drive(&["open", &b, "--split", "0", "--dir", "v"]);
    let four = layout_when(&mut rig, &mut ws, "pane 4", |v| v["focus"] == 4);
    assert_eq!(cells(&four, 1)[3] + cells(&four, 4)[3], rows, "{four}");
    Ok(())
}

/// `Shift+S` focuses the shell, which opens the drawer, so the user sees the agent answer.
#[test]
fn a_submit_opens_the_drawer() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("a.rs", &five_lines())?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 2, "text": "why"}),
    );
    let acted = rig.act(&mut ws, json!({"act": "submit", "pane": 1, "body": ""}));
    assert_eq!(acted["ok"], true, "{acted}");
    // `act` keeps the layouts from before the submit, when the shell had the focus too.
    layout_when(&mut rig, &mut ws, "the shell in the drawer", |v| {
        v["focus"] == 0 && pane_of(v, 0)["drawer"] == true
    });
    Ok(())
}

/// #112: a bare open fits a stacked pane to its file's rows plus the web app's three-row border.
/// The browser draws markdown blocks taller than their rows, so a markdown file gets half. The
/// shell is in the drawer, so the second pane splits the first side by side, and the third stacks.
#[test]
fn a_stacked_split_fits_a_plain_file_but_not_markdown_in_the_web_app() -> Result<()> {
    for (name, body, want) in [
        ("c.txt", five_lines(), 8),
        ("c.txt", String::new(), 4),
        ("c.md", "# Title\n\npara\n".to_string(), 20),
    ] {
        let mut rig = Rig::new()?;
        let (_a, a) = file("a.txt", "x\n")?;
        let (_b, b) = file("b.txt", "x\n")?;
        let (_c, path) = file(name, &body)?;
        rig.drive(&["open", &a]);
        rig.drive(&["open", &b]);
        rig.drive(&["open", &path]);
        let report: Value = serde_json::from_str(&rig.drive(&["layout"]))?;
        assert_eq!(report["panes"][3]["rect"]["height"], want, "{report}");
    }
    Ok(())
}

/// The web app wraps a file pane's line inside the border, less the gutter's blank cell, and
/// `laura layout` counts its rows at that width.
#[test]
fn web_layout_counts_the_gutter_column() -> Result<()> {
    let mut rig = Rig::new()?;
    // Plain text wraps; code clips.
    let (_dir, path) = file("a.txt", "x\n")?;
    rig.drive(&["open", &path]);
    let report: Value = serde_json::from_str(&rig.drive(&["layout"]))?;
    let w = report["panes"][1]["rect"]["width"].as_u64().expect("width") as usize;
    // Fills the TUI's text width exactly: the border's two columns, then a one-digit line number
    // and the gutter's five cells (`Panel::layout`).
    std::fs::write(&path, format!("{}\n", "y".repeat(w - 2 - 6)))?;
    let start = Instant::now();
    while !rig.tab.panels[&1].content.contains('y') {
        assert!(
            start.elapsed() < common::DEADLINE,
            "the pane never reloaded"
        );
        rig.pump();
        thread::sleep(Duration::from_millis(10));
    }
    let report: Value = serde_json::from_str(&rig.drive(&["layout"]))?;
    assert_eq!(report["panes"][1]["content_rows"], 2, "{report}");
    Ok(())
}

#[test]
fn pane_html_escapes_the_file() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    // Not `.html`, which shows as a page.
    let (_dir, path) = file("a.xml", "<script>alert(1)</script>\n")?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(rows(&html)[0].1.contains("&lt;script&gt;"), "{html}");
    assert!(!html.contains("<script"), "{html}");
    Ok(())
}

#[test]
fn the_pane_selector_hint_names_its_keystrokes() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let layout = rig.until(&mut ws, "layout", |f| f.is("layout")).json();
    let hint = pairs(&layout["hints"]["panes"]);
    assert_eq!(
        hint, "↑ ↓ select · Enter focus · type an id to focus at once · Esc close",
        "{layout}"
    );
    Ok(())
}

#[test]
fn the_help_names_the_mockups_groups() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let strings = rig.until(&mut ws, "strings", |f| f.is("strings")).json();
    let groups: Vec<&str> = strings["help"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| g[0].as_str())
        .collect();
    for g in [
        "Workspace",
        "In a focused file pane",
        "In an editor pane",
        "Typing a comment or review body",
    ] {
        assert!(groups.contains(&g), "{groups:?}");
    }
    assert!(
        strings["help_note"]
            .as_str()
            .is_some_and(|n| n.contains("Hover a button to see its keystroke")),
        "{strings}"
    );
    Ok(())
}

#[test]
fn comment_mode_names_its_keystrokes() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let strings = rig.until(&mut ws, "strings", |f| f.is("strings")).json();
    let group = strings["help"]
        .as_array()
        .and_then(|gs| gs.iter().find(|g| g[0] == "Comment mode on an HTML page"))
        .expect("a comment mode group");
    assert!(
        group[1]
            .as_array()
            .is_some_and(|keys| keys.iter().any(|k| k[0] == "Esc"))
    );
    let layout = rig.until(&mut ws, "layout", |f| f.is("layout")).json();
    assert!(pairs(&layout["hints"]["comment_mode"]).contains("Esc leave comment mode"));
    Ok(())
}

#[test]
fn markdown_html_is_sanitized() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file(
        "doc.md",
        "# T {#overlay .row data-line=1}\n\n<script>alert(1)</script>\n\n\
         <b>x</b> [x](javascript:alert(1)) [y](https://example.com) [y](#top) [z](b.md) [w](//h/x.md) <a@b.co> note[^1]\n\n[^1]: foot\n\n```x\trow\ncode data-line=\"1\">\n```\n",
    )?;
    rig.drive(&["open", &path]);
    let html = pane_frame(&mut rig, &mut ws, 1)["html"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(html.contains("&lt;script&gt;"), "{html}");
    assert!(html.contains("&lt;b&gt;"), "{html}");
    assert!(!html.contains("<script") && !html.contains("<b>"), "{html}");
    // Only the `https` link keeps an `href`, a relative link gets a `data-href`, and the
    // footnote reference is text.
    assert_eq!(html.matches(" href=").count(), 1, "{html}");
    assert!(html.contains(r#"href="https://example.com""#), "{html}");
    assert!(html.contains(r##"data-href="#top""##), "{html}");
    assert!(html.contains(r#"data-href="b.md""#), "{html}");
    assert!(!html.contains(r#"data-href="//"#), "{html}");
    assert!(!html.contains(r#"data-href="a@b.co""#), "{html}");
    assert!(html.contains("note[^1]"), "{html}");
    assert!(html.contains("[1]: foot"), "{html}");
    let heading = &blocks(&html)[0].1;
    assert!(!heading.contains("id="), "{heading}");
    assert!(!heading.contains(r#"class="row""#), "{heading}");
    assert_eq!(heading.matches("data-line").count(), 1, "{heading}");
    // An unknown fence language can't add a class, and code can't fake a line's attribute.
    assert!(!html.contains("language-"), "{html}");
    assert!(html.contains("data-line=&quot;1&quot;&gt;"), "{html}");
    Ok(())
}

#[test]
fn the_layout_says_when_the_agent_is_ready() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let layout = rig.until(&mut ws, "layout", |f| f.is("layout")).json();
    assert_eq!(layout["agent"], false, "{layout}");
    rig.drive(&["ready"]);
    rig.until(
        &mut ws,
        "ready layout",
        |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["agent"] == true),
    );
    Ok(())
}
