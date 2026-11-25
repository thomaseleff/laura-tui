//! Feat #26, #215: a changed line's row tint. A real git repo in a tempdir, driven through the
//! real seam (binary → socket → `Tab::drain`); assert on `panel.changes` state, not
//! pixels, per CLAUDE.md.

mod common;

use std::path::Path;
use std::process::Command as StdCommand;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use assert_cmd::Command;
use laura::{ChangeKind, Rect, Tab};
use ratatui::style::Color;

fn spawn_tab() -> Result<Tab> {
    let cmd = portable_pty::CommandBuilder::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
    Tab::spawn(cmd, 24, 80)
}

fn area() -> Rect {
    Rect::new(0, 0, 120, 40)
}

/// Run `laura <args>` against a real tab, draining until it exits; return trimmed stdout.
fn drive(tab: &mut Tab, args: &[&str]) -> String {
    let name = tab.socket.clone();
    let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        let out = Command::cargo_bin("laura")
            .unwrap()
            .args(&a)
            .env("LAURA_TAB", &name)
            .output()
            .unwrap();
        tx.send(out).unwrap();
    });
    let start = Instant::now();
    let out = loop {
        tab.drain(area());
        if let Ok(out) = rx.try_recv() {
            break out;
        }
        assert!(start.elapsed() < common::DEADLINE, "client timed out");
        thread::sleep(Duration::from_millis(5));
    };
    h.join().unwrap();
    assert!(
        out.status.success(),
        "laura {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Run `git <args>` in `dir`, asserting success.
fn git(dir: &Path, args: &[&str]) {
    let out = StdCommand::new("git")
        .arg("-C")
        .arg(dir)
        // Identity + no signing so `commit` works on a bare CI account.
        .args([
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .expect("git spawns (test host needs git on PATH)");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Init a repo in a fresh tempdir and commit `name` with `body`. Returns the dir + file path.
fn repo_with(name: &str, body: &str) -> Result<(tempfile::TempDir, String)> {
    let dir = tempfile::tempdir()?;
    git(dir.path(), &["init", "-q"]);
    let file = dir.path().join(name);
    std::fs::write(&file, body)?;
    git(dir.path(), &["add", name]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    let path = file.to_str().unwrap().to_string();
    Ok((dir, path))
}

#[test]
fn markers_track_modify_add_delete() -> Result<()> {
    // 10-line committed file.
    let committed: String = (1..=10).map(|i| format!("line {i}\n")).collect();
    let (_dir, path) = repo_with("f.txt", &committed)?;

    // Modify line 2, delete line 5, insert a line after line 8.
    let edited = "\
line 1
line 2 CHANGED
line 3
line 4
line 6
line 7
line 8
inserted
line 9
line 10
";
    std::fs::write(&path, edited)?;

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    let changes = &tab.panels[&id].changes;

    assert_eq!(changes[1], Some(ChangeKind::Modified), "line 2 modified");
    // The deleted line 5 marks the surviving line below the gap (new line 5, "line 6").
    assert_eq!(
        changes[4],
        Some(ChangeKind::Removed(1)),
        "deletion below gap"
    );
    assert_eq!(changes[7], Some(ChangeKind::Added), "inserted line");
    for (i, c) in changes.iter().enumerate() {
        if ![1, 4, 7].contains(&i) {
            assert_eq!(*c, None, "line {i} unchanged");
        }
    }
    // The border's `+N -M`: one line modified, one deleted, one inserted.
    assert_eq!(tab.panels[&id].diff_stat, (2, 2));

    // Reload picks up a further edit: also modify line 1.
    std::fs::write(&path, edited.replacen("line 1\n", "line 1 CHANGED\n", 1))?;
    tab.drain(area());
    assert_eq!(
        tab.panels[&id].changes[0],
        Some(ChangeKind::Modified),
        "reload refreshed markers"
    );
    assert_eq!(tab.panels[&id].diff_stat, (3, 3), "reload refreshed counts");
    Ok(())
}

#[test]
fn a_clean_file_counts_no_changes() -> Result<()> {
    let (_dir, path) = repo_with(
        "f.txt", "a
b
",
    )?;
    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    assert_eq!(tab.panels[&id].diff_stat, (0, 0));
    Ok(())
}

#[test]
fn markdown_markers_key_off_source_lines() -> Result<()> {
    // #32: the block-map makes markdown diff work — `changes` is source-line-keyed.
    let (_dir, path) = repo_with("doc.md", "# title\n\nbody\n")?;
    std::fs::write(&path, "# title\n\nbody changed\n")?; // source line 3 modified

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    let changes = &tab.panels[&id].changes;
    assert_eq!(
        changes.get(2).copied().flatten(),
        Some(ChangeKind::Modified),
        "edited source line 3 is Modified, keyed by source line: {changes:?}"
    );

    // The paragraph's rendered row carries a row tint and its gutter shows source line 3.
    let layout = tab.panels[&id].layout(78);
    let body = layout
        .rows
        .iter()
        .find(|r| r.text().contains("body"))
        .expect("paragraph row");
    assert_eq!(body.gutter, Some(3), "gutter shows the source line");
    assert_eq!(
        body.change,
        Some(ChangeKind::Modified),
        "the rendered row's tint folds the edited source line"
    );
    Ok(())
}

#[test]
fn a_deletion_at_a_closing_fence_shows_on_the_next_row() -> Result<()> {
    // #99: the deletion marks the hidden closing fence, so the row after the code block carries it.
    let (_dir, path) = repo_with("doc.md", "```\na\nb\nc\n```\nafter\n")?;
    std::fs::write(&path, "```\na\nb\n```\nafter\n")?;

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    let layout = tab.panels[&id].layout(78);
    let rows = &layout.rows;
    let gap = rows
        .iter()
        .position(|r| r.text().contains("1 line removed"));
    let after = rows
        .iter()
        .position(|r| r.text().contains("after"))
        .expect("after row");
    assert!(
        gap.is_some_and(|g| g < after),
        "removed-lines row above `after`"
    );
    // #215: the removed-lines row carries the deletion; the line after it shows no tint.
    assert_eq!(rows[gap.unwrap()].change, Some(ChangeKind::Removed(1)));
    assert_eq!(rows[after].change, None);
    Ok(())
}

#[test]
fn a_deletion_above_an_opening_fence_shows_on_the_first_code_row() -> Result<()> {
    // #99: gitdiff marks the opening fence below the gap; the first code row carries it.
    let (_dir, path) = repo_with("doc.md", "x\n\ny\n```\na\n```\n")?;
    std::fs::write(&path, "x\n\n```\na\n```\n")?;

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    let layout = tab.panels[&id].layout(78);
    let rows = &layout.rows;
    let gap = rows
        .iter()
        .position(|r| r.text().contains("1 line removed"));
    let code = rows
        .iter()
        .position(|r| r.text().trim() == "a")
        .expect("code row");
    assert!(
        gap.is_some_and(|g| g < code),
        "removed-lines row above the code row"
    );
    Ok(())
}

#[test]
fn a_deletion_at_a_closing_fence_that_ends_the_file_shows_on_the_last_row() -> Result<()> {
    // #99: no row comes after a closing fence on the file's last line, so the last row carries it.
    let (_dir, path) = repo_with("doc.md", "```\na\nb\n```\n")?;
    std::fs::write(&path, "```\na\n```\n")?;

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    let layout = tab.panels[&id].layout(78);
    let rows = &layout.rows;
    let gap = rows
        .iter()
        .position(|r| r.text().contains("1 line removed"));
    let a = rows
        .iter()
        .position(|r| r.text().trim() == "a")
        .expect("code row");
    assert!(
        gap.is_some_and(|g| g < a),
        "removed-lines row above the last row"
    );
    assert_eq!(rows[gap.unwrap()].change, Some(ChangeKind::Removed(1)));
    assert_eq!(rows[a].change, None);
    Ok(())
}

#[test]
fn an_end_of_file_deletion_shows_below_the_last_row() -> Result<()> {
    // #142: no line comes after the deleted ones, so the gap goes below the last row, above its card.
    let (_dir, path) = repo_with("f.txt", "one\ntwo\nthree\n")?;
    std::fs::write(&path, "one\ntwo\n")?;

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    let text = |tab: &Tab| -> Vec<String> {
        tab.panels[&id]
            .layout(78)
            .rows
            .iter()
            .map(|r| r.text())
            .collect()
    };
    let at = |rows: &[String], s: &str| {
        rows.iter()
            .position(|r| r.contains(s))
            .unwrap_or_else(|| panic!("no {s}: {rows:#?}"))
    };
    let rows = text(&tab);
    assert!(at(&rows, "two") < at(&rows, "1 line removed"), "{rows:#?}");
    let layout = tab.panels[&id].layout(78);
    assert_eq!(layout.rows[at(&rows, "two")].change, None);
    assert!(
        !tab.panels[&id]
            .changes
            .iter()
            .any(|c| matches!(c, Some(ChangeKind::Removed(_)))),
        "the last line carries no removed mark"
    );

    drive(
        &mut tab,
        &["comment", "--pane", &id.to_string(), "2", "note"],
    );
    let rows = text(&tab);
    assert!(at(&rows, "1 line removed") < at(&rows, "╭"), "{rows:#?}");
    Ok(())
}

#[test]
fn row_tint_runs_every_wrapped_row() -> Result<()> {
    // #35: the row tint paints every visual row of a changed wrapped paragraph, while
    // the gutter number stays on the first row only.
    let long = "word ".repeat(40); // wraps into many rows at a narrow width
    let (_dir, path) = repo_with("doc.md", &format!("# t\n\n{long}\n"))?;
    std::fs::write(&path, format!("# t\n\n{long}edited\n"))?; // paragraph modified vs HEAD

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    let layout = tab.panels[&id].layout(30);
    let para: Vec<_> = layout
        .rows
        .iter()
        .filter(|r| r.text().contains("word"))
        .collect();
    assert!(
        para.len() >= 2,
        "paragraph wraps to multiple rows: {}",
        para.len()
    );
    assert!(
        para.iter().all(|r| r.change == Some(ChangeKind::Modified)),
        "the row tint paints every wrapped row: {:?}",
        para.iter().map(|r| r.change).collect::<Vec<_>>()
    );
    assert_eq!(
        para.iter().filter(|r| r.gutter.is_some()).count(),
        1,
        "the gutter number stays on the first wrapped row only"
    );
    Ok(())
}

#[test]
fn scroll_offset_ignores_neighbor_deletion_gap() -> Result<()> {
    // 30-line file; delete a 16-line block (11..=26) in one hunk.
    let committed: String = (1..=30).map(|i| format!("line {i}\n")).collect();
    let (_dir, path) = repo_with("f.txt", &committed)?;
    let edited: String = (1..=10)
        .chain(27..=30)
        .map(|i| format!("line {i}\n"))
        .collect();
    std::fs::write(&path, &edited)?;

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    let panel = tab.panels.get_mut(&id).unwrap();
    panel.set_diff_view(true).expect("edits vs HEAD exist");

    // Park the cursor on line 10 (0-based 9), the surviving line right above the
    // 16-row deletion block. It has no wraps/comments, so its correct viewport
    // anchor is its own single row — not dragged 16 rows further by the deletion
    // rows queued for the line below it.
    panel.move_cursor(9);
    let layout = panel.layout(80);
    let view_h = 5;
    let expected = layout.starts[9].saturating_sub(view_h - 1);
    assert_eq!(
        panel.scroll_offset(&layout, view_h),
        expected,
        "the neighboring 16-line deletion gap must not shift the cursor's own viewport anchor"
    );
    Ok(())
}

#[test]
fn untracked_file_has_no_tints() -> Result<()> {
    let dir = tempfile::tempdir()?;
    git(dir.path(), &["init", "-q"]);
    let file = dir.path().join("new.txt");
    std::fs::write(&file, "brand new\n")?; // never added/committed
    let path = file.to_str().unwrap().to_string();

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    assert!(
        tab.panels[&id].changes.is_empty(),
        "untracked file → git diff empty, no tints (v1 scope)"
    );
    Ok(())
}

#[test]
fn a_changed_line_tints_its_rows_and_no_gutter_has_a_change_cell() -> Result<()> {
    // #215: a modified line's every wrapped row takes the Modified tint, the removed-lines row the
    // Removed tint, and the gutter is the number and one space.
    let long = "word ".repeat(20);
    let (_dir, path) = repo_with("f.txt", &format!("keep\n{long}\nmid\ngone\nlast\n"))?;
    std::fs::write(&path, format!("keep\n{long}edited\nmid\nlast\n"))?;

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    let panel = &tab.panels[&id];
    let width = 40;
    let layout = panel.layout(width);
    let lines = laura::view::panel_lines(panel, &layout, false, width, false);
    // A 5-line file still gets two digits, so Neovim's 2-cell sign fits in the editor's gutter.
    assert_eq!(layout.gutter_width, 2);
    let rows = &layout.rows;
    let changed: Vec<usize> = (0..rows.len())
        .filter(|&j| rows[j].text().contains("word"))
        .collect();
    assert!(changed.len() >= 2, "the modified line wraps: {changed:?}");
    let gap = rows
        .iter()
        .position(|r| r.text().contains("1 line removed"))
        .expect("removed-lines row");
    let bg = |j: usize| lines[j].style.bg;
    for &j in &changed {
        assert_eq!(rows[j].change, Some(ChangeKind::Modified), "row {j}");
        assert_eq!(bg(j), Some(Color::Rgb(36, 52, 74)), "row {j} is tinted");
    }
    assert_eq!(rows[gap].change, Some(ChangeKind::Removed(1)));
    assert_eq!(
        bg(gap),
        Some(Color::Rgb(61, 38, 43)),
        "the removed-lines row"
    );
    let keep = rows.iter().position(|r| r.text() == "keep").expect("keep");
    assert_eq!(rows[keep].change, None);
    assert_eq!(bg(keep), None, "an unchanged row has no tint");
    for (j, line) in lines.iter().enumerate().filter(|(j, _)| !rows[*j].comment) {
        assert_eq!(
            line.spans[1].content, " ",
            "row {j}: the space after the number"
        );
        let body: String = line.spans[2..].iter().map(|s| s.content.as_ref()).collect();
        assert!(
            body.starts_with(&rows[j].text()),
            "row {j}: the text follows the space: {body:?}"
        );
    }
    Ok(())
}

#[test]
fn a_changed_code_line_is_tinted_over_its_box() -> Result<()> {
    // #215: in the TUI, an edited line in a fenced code block takes the Modified tint on its text
    // too, not only beside the code box.
    let (_dir, path) = repo_with("f.md", "# t\n\n```\nlet a = 1;\nlet b = 2;\n```\n")?;
    std::fs::write(&path, "# t\n\n```\nlet a = 1;\nlet b = 3;\n```\n")?;

    let mut tab = spawn_tab()?;
    let id: u64 = drive(&mut tab, &["open", &path, "--no-focus"]).parse()?;
    let panel = &tab.panels[&id];
    let width = 40;
    let layout = panel.layout(width);
    let lines = laura::view::panel_lines(panel, &layout, false, width, false);
    let rows = &layout.rows;
    let j = rows
        .iter()
        .position(|r| r.text().contains("let b = 3;"))
        .expect("edited code row");
    assert_eq!(rows[j].change, Some(ChangeKind::Modified));
    assert_eq!(lines[j].style.bg, Some(Color::Rgb(36, 52, 74)));
    let code_bg = Some(Color::Rgb(45, 45, 45));
    assert!(
        lines[j].spans.iter().all(|s| s.style.bg != code_bg),
        "no code box under the tint: {:?}",
        lines[j].spans
    );
    let k = rows
        .iter()
        .position(|r| r.text().contains("let a = 1;"))
        .expect("unchanged code row");
    assert!(
        lines[k].spans.iter().any(|s| s.style.bg == code_bg),
        "an unchanged code row keeps its box"
    );
    Ok(())
}
