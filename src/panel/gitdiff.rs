//! Diff an open file against git HEAD. #26 folds this to per-line markers;
//! #18 renders removed/added text inline. One git call, header + body parse.
//! `project` reads the folder's branch, changes and latest commit for the start canvas.
//!
//! We shell out to the `git` CLI (not a git crate) to keep the release binary
//! small and C-dependency-free — see `planning/git-diff-gutter.md`.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use crate::platform::strip_verbatim;

/// Latched once when any diff attempt finds no `git` binary. Git-presence is a
/// machine-global fact (one PATH), so a set-once latch, not per-panel state; the
/// TUI reads it to arm the "install git" toast.
static GIT_MISSING: AtomicBool = AtomicBool::new(false);

/// Whether a `hunks` call ever failed to spawn `git` (binary absent from PATH).
pub fn git_missing() -> bool {
    GIT_MISSING.load(Ordering::Relaxed)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ChangeKind {
    Added,
    Modified,
    /// Deleted lines; carries the count for the synthetic gap row.
    Removed(usize),
}

/// One zero-context hunk, keyed to current (new-file) line numbers.
#[derive(Debug, PartialEq)]
pub struct Hunk {
    /// 1-based new-file line the hunk's added lines start at (the line *above*
    /// the gap for a pure deletion).
    pub new_start: usize,
    /// Old lines (text, prefix stripped) — #18 renders these.
    pub removed: Vec<String>,
    /// New lines (text, prefix stripped).
    pub added: Vec<String>,
}

pub enum DiffOutcome {
    /// Parsed hunks (empty = clean / identical to HEAD).
    Ok(Vec<Hunk>),
    /// `git` binary not found → drives the warning + toast.
    NoGit,
    /// Untracked / non-repo / any other error → silent, no markers.
    Unavailable,
}

/// Run `git diff --unified=0 HEAD -- <path>` and parse it. The single source of
/// truth both workstreams call.
pub fn hunks(path: &str) -> DiffOutcome {
    // Canonicalize so git's own path resolution matches our pathspec (symlinked
    // temp dirs, etc.). A path that can't resolve isn't diffable.
    let Ok(abs) = std::fs::canonicalize(path) else {
        return DiffOutcome::Unavailable;
    };
    let abs = strip_verbatim(abs);
    let Some(dir) = abs.parent() else {
        return DiffOutcome::Unavailable;
    };
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["diff", "--unified=0", "--no-color", "HEAD", "--"])
        .arg(&abs)
        .output();
    match output {
        Ok(o) if o.status.success() => DiffOutcome::Ok(parse(&String::from_utf8_lossy(&o.stdout))),
        // Non-zero: untracked file, not a repo, no HEAD yet — no markers, no noise.
        Ok(_) => DiffOutcome::Unavailable,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            GIT_MISSING.store(true, Ordering::Relaxed);
            DiffOutcome::NoGit
        }
        Err(_) => DiffOutcome::Unavailable,
    }
}

/// The most files the start canvas lists; the rest is a count.
const FILES: usize = 8;

/// A changed file on the start canvas: `M`, `A`, `D` (a rename is a `D` and an `A`), or `?` for an
/// untracked file. Its path is relative to Laura's folder; the counts are `None` for a binary or
/// untracked file.
#[derive(Serialize)]
pub(crate) struct FileChange {
    status: char,
    path: String,
    add: Option<u32>,
    del: Option<u32>,
}

/// The project's git state for the start canvas: the changes since HEAD, else the latest commit.
/// `more` counts the files past the first `FILES`.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum Project {
    Changes {
        branch: String,
        files: Vec<FileChange>,
        more: usize,
    },
    Latest {
        branch: String,
        hash: String,
        author: String,
        /// The commit's time, in Unix ms.
        ts: u64,
        subject: String,
        files: Vec<FileChange>,
        more: usize,
    },
}

/// `root`'s branch and its changes, else its latest commit. `None` outside a repo, in a repo with
/// no commit, or without `git`.
pub(crate) fn project(root: &Path) -> Option<Project> {
    let root = strip_verbatim(root.to_path_buf());
    let branch = text(git(&root, &["branch", "--show-current"])?);
    let branch = if branch.is_empty() {
        "HEAD".into()
    } else {
        branch
    };
    let mut changed = files(&root, &["diff", "HEAD"])?;
    let untracked = git(&root, &["ls-files", "-o", "--exclude-standard", "-z"])?;
    changed.extend(nul_split(&untracked).map(|path| FileChange {
        status: '?',
        path: path.into(),
        add: None,
        del: None,
    }));
    if !changed.is_empty() {
        let more = changed.len().saturating_sub(FILES);
        changed.truncate(FILES);
        return Some(Project::Changes {
            branch,
            files: changed,
            more,
        });
    }
    let log = text(git(
        &root,
        &["log", "-1", "--format=%h%x00%an%x00%ct%x00%s"],
    )?);
    let mut f = log.split('\0');
    let (hash, author, ts, subject) = (f.next()?, f.next()?, f.next()?, f.next()?);
    let (hash, author, subject) = (hash.into(), author.into(), subject.into());
    let ts = ts.parse::<u64>().ok()? * 1000;
    let mut files = files(&root, &["show", "HEAD", "--format="])?;
    let more = files.len().saturating_sub(FILES);
    files.truncate(FILES);
    Some(Project::Latest {
        branch,
        hash,
        author,
        ts,
        subject,
        files,
        more,
    })
}

/// The files that `git <rev>` changes, below `root` and relative to it, with their counts.
fn files(root: &Path, rev: &[&str]) -> Option<Vec<FileChange>> {
    let run = |kind: &str| {
        let mut args = rev.to_vec();
        args.extend(["--no-renames", "--relative", "-z", kind]);
        git(root, &args)
    };
    let numstat = run("--numstat")?;
    // `add\tdel\tpath`, with `-` counts for a binary file.
    let counts: HashMap<&str, (Option<u32>, Option<u32>)> = nul_split(&numstat)
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            let (add, del, path) = (f.next()?, f.next()?, f.next()?);
            Some((path, (add.parse().ok(), del.parse().ok())))
        })
        .collect();
    let status = run("--name-status")?;
    // `X\0path\0` pairs.
    let mut f = nul_split(&status);
    let mut files = vec![];
    while let (Some(s), Some(path)) = (f.next(), f.next()) {
        let (add, del) = counts.get(path).copied().unwrap_or_default();
        files.push(FileChange {
            status: s.chars().next().unwrap_or('M'),
            path: path.into(),
            add,
            del,
        });
    }
    Some(files)
}

/// `git -C root <args>`'s output. `None` when git exits non-zero, or is missing, which latches
/// `GIT_MISSING`.
fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    match Command::new("git").arg("-C").arg(root).args(args).output() {
        Ok(o) if o.status.success() => Some(o.stdout),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            GIT_MISSING.store(true, Ordering::Relaxed);
            None
        }
        _ => None,
    }
}

fn text(out: Vec<u8>) -> String {
    String::from_utf8_lossy(&out).trim().to_string()
}

/// The fields of `-z` output. A path that isn't UTF-8 reads as `�`, so the pairs stay aligned.
fn nul_split(out: &[u8]) -> impl Iterator<Item = &str> {
    out.split(|&b| b == 0)
        .filter(|f| !f.is_empty())
        .map(|f| std::str::from_utf8(f).unwrap_or("\u{FFFD}"))
}

/// Fold hunks → per-source-line marker, 1:1 with a `line_count`-line file.
/// `None` = unchanged. `Removed(n)` lands on the surviving line below the gap. An EOF deletion has
/// no line below it, so it marks none; `removed_lines` keys it at `line_count` (#142).
pub fn line_changes(hunks: &[Hunk], line_count: usize) -> Vec<Option<ChangeKind>> {
    let mut out = vec![None; line_count];
    for h in hunks {
        if h.added.is_empty() {
            // Pure deletion: `new_start` is the last surviving line above the gap,
            // so the line *below* the gap is 0-based index `new_start`.
            if let Some(slot) = out.get_mut(h.new_start) {
                *slot = Some(ChangeKind::Removed(h.removed.len()));
            }
        } else {
            let kind = if h.removed.is_empty() {
                ChangeKind::Added
            } else {
                ChangeKind::Modified
            };
            for i in 0..h.added.len() {
                let idx = h.new_start.saturating_sub(1) + i; // 1-based header → 0-based
                if idx < line_count {
                    out[idx] = Some(kind);
                }
            }
        }
    }
    out
}

/// Fold hunks → deleted-line text keyed by the 0-based source line the gap sits
/// *before* (mirrors `line_changes`' deletion-vs-modified split). #18 renders these
/// as red `-` rows above that line; an EOF deletion keys to `line_count` (trailing).
pub fn removed_lines(hunks: &[Hunk], line_count: usize) -> Vec<(usize, Vec<String>)> {
    hunks
        .iter()
        .filter(|h| !h.removed.is_empty())
        .map(|h| {
            let idx = if h.added.is_empty() {
                h.new_start // surviving line below the gap is 0-based `new_start`; gap sits before it
            } else {
                h.new_start.saturating_sub(1) // removed pair above the first added (0-based) line
            };
            (idx.min(line_count), h.removed.clone())
        })
        .collect()
}

/// Parse `git diff --unified=0` output into hunks. Header math is trusted over
/// re-counting bodies — git's contract.
fn parse(text: &str) -> Vec<Hunk> {
    let mut hunks: Vec<Hunk> = vec![];
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("@@") {
            if let Some(new_start) = parse_new_start(rest) {
                hunks.push(Hunk {
                    new_start,
                    removed: vec![],
                    added: vec![],
                });
            }
        } else if let Some(h) = hunks.last_mut() {
            // Inside a hunk body every `+`/`-` line is content; file headers
            // (`+++`/`---`) only appear before the first `@@`, so no special-case.
            if let Some(t) = line.strip_prefix('+') {
                h.added.push(t.to_string());
            } else if let Some(t) = line.strip_prefix('-') {
                h.removed.push(t.to_string());
            }
        }
    }
    hunks
}

/// From `@@`'s tail (` -a,b +c,d @@ …`), pull the new-file start `c`. Omitted
/// `,count` means 1; `new_start` itself can be 0 (deletion before line 1).
fn parse_new_start(rest: &str) -> Option<usize> {
    let plus = rest.split_whitespace().find(|t| t.starts_with('+'))?;
    plus.trim_start_matches('+').split(',').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Parser + fold are pure logic reachable only through a live git repo; check
    // them here on a canned `git diff --unified=0` string so CI without git still
    // exercises the header math. The repo-driven test in tests/diff_gutter.rs is
    // the real coverage.
    #[test]
    fn parses_and_folds_a_mixed_diff() {
        // A 5-line file: line 2 modified, a line added after 3, line 5 deleted.
        let diff = "\
diff --git a/f.rs b/f.rs
index 111..222 100644
--- a/f.rs
+++ b/f.rs
@@ -2,1 +2,1 @@
-old two
+new two
@@ -3,0 +4,1 @@
+added line
@@ -6,1 +6,0 @@
-gone
";
        let hunks = parse(diff);
        assert_eq!(
            hunks,
            vec![
                Hunk {
                    new_start: 2,
                    removed: vec!["old two".into()],
                    added: vec!["new two".into()],
                },
                Hunk {
                    new_start: 4,
                    removed: vec![],
                    added: vec!["added line".into()],
                },
                Hunk {
                    new_start: 6,
                    removed: vec!["gone".into()],
                    added: vec![],
                },
            ]
        );

        // A 7th line puts the deletion mid-file.
        let changes = line_changes(&hunks, 7);
        assert_eq!(changes[1], Some(ChangeKind::Modified)); // line 2
        assert_eq!(changes[3], Some(ChangeKind::Added)); // line 4
        assert_eq!(changes[6], Some(ChangeKind::Removed(1))); // gap above line 7
        assert_eq!(changes[0], None);
        assert_eq!(changes[2], None);
        assert_eq!(changes[4], None);
        assert_eq!(changes[5], None);
        // With 6 lines the deletion ends the file, so no line carries it (#142).
        assert_eq!(line_changes(&hunks, 6)[5], None);
    }
}
