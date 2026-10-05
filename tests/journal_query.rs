//! `laura journal` reads every session's journal off disk and filters it, outside a workspace.
//! Done when: one command lists feedback across sessions, with no `jq`.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use assert_cmd::Command;
use serde_json::{Value, json};
use tempfile::TempDir;

const MIN: u64 = 60_000;

/// A home with two sessions plus noise: a bad line in `a`, and a non-`.ndjson` file.
fn fixtures() -> Result<TempDir> {
    let home = tempfile::tempdir()?;
    let dir = home.path().join(".laura").join("sessions");
    std::fs::create_dir_all(&dir)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    let line = |ago: u64, v: Value| {
        let mut v = v;
        v["ts"] = json!(now - ago);
        format!("{v}\n")
    };
    std::fs::write(
        dir.join("a.ndjson"),
        line(2 * 24 * 60 * MIN, json!({"type": "ready"}))
            + &line(60 * MIN, json!({"type": "feedback", "sentiment": "+"}))
            + "not json\n",
    )?;
    std::fs::write(
        dir.join("b.ndjson"),
        line(30 * MIN, json!({"type": "review", "comments": 2}))
            + &line(10 * MIN, json!({"type": "feedback", "sentiment": "-"})),
    )?;
    std::fs::write(
        dir.join("notes.txt"),
        line(MIN, json!({"type": "feedback", "sentiment": "+"})),
    )?;
    Ok(home)
}

fn journal(home: &TempDir, args: &[&str]) -> Result<std::process::Output> {
    Ok(Command::cargo_bin("laura")?
        .arg("journal")
        .args(args)
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env_remove("LAURA_TAB")
        .output()?)
}

/// Each stdout line's `type` (and `sentiment`, when present), in order.
fn kinds(out: &std::process::Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| {
            let v: Value = serde_json::from_str(l).expect("stdout is NDJSON");
            match v["sentiment"].as_str() {
                Some(s) => format!("{}{s}", v["type"].as_str().unwrap_or("")),
                None => v["type"].as_str().unwrap_or("").to_string(),
            }
        })
        .collect()
}

#[test]
fn journal_merges_every_session() -> Result<()> {
    let home = fixtures()?;
    let out = journal(&home, &[])?;
    assert!(out.status.success());
    assert_eq!(
        kinds(&out),
        ["ready", "feedback+", "review", "feedback-"],
        "every session, ts order, no bad line or notes.txt"
    );
    Ok(())
}

#[test]
fn journal_filters_on_fields() -> Result<()> {
    let home = fixtures()?;
    let out = journal(&home, &["type=feedback", "sentiment=+"])?;
    assert_eq!(kinds(&out), ["feedback+"], "different fields AND");

    let out = journal(&home, &["type=feedback", "type=review"])?;
    assert_eq!(
        kinds(&out),
        ["feedback+", "review", "feedback-"],
        "a repeated field ORs"
    );

    let out = journal(&home, &["comments=2"])?;
    assert_eq!(kinds(&out), ["review"], "non-strings match by JSON text");
    Ok(())
}

#[test]
fn journal_since_drops_older_events() -> Result<()> {
    let home = fixtures()?;
    let out = journal(&home, &["--since", "24h"])?;
    assert_eq!(kinds(&out), ["feedback+", "review", "feedback-"]);
    Ok(())
}

#[test]
fn journal_limit_keeps_the_newest() -> Result<()> {
    let home = fixtures()?;
    let out = journal(&home, &["-n", "1", "type=feedback"])?;
    assert_eq!(kinds(&out), ["feedback-"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("showing 1 of 2"));

    let out = journal(&home, &["-n", "0", "type=feedback"])?;
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("showing 0 of 2"));
    Ok(())
}

#[test]
fn journal_rejects_a_bad_filter() -> Result<()> {
    let home = fixtures()?;
    let out = journal(&home, &["type"])?;
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("FIELD=VALUE"));
    Ok(())
}

#[test]
fn journal_with_no_sessions_is_empty() -> Result<()> {
    let home = tempfile::tempdir()?;
    let out = journal(&home, &[])?;
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    Ok(())
}
