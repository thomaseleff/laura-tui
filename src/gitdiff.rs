//! Diff an open file against git HEAD. #26 folds this to per-line markers;
//! #18 renders removed/added text inline. One git call, header + body parse.
//!
//! We shell out to the `git` CLI (not a git crate) to keep the release binary
//! small and C-dependency-free — see `planning/git-diff-gutter.md`.

use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// Fold hunks → per-source-line marker, 1:1 with a `line_count`-line file.
/// `None` = unchanged. `Removed(n)` lands on the surviving line below the gap.
pub fn line_changes(hunks: &[Hunk], line_count: usize) -> Vec<Option<ChangeKind>> {
    let mut out = vec![None; line_count];
    if line_count == 0 {
        return out;
    }
    for h in hunks {
        if h.added.is_empty() {
            // Pure deletion: `new_start` is the last surviving line above the gap,
            // so the line *below* the gap is 0-based index `new_start`.
            // ponytail: an EOF deletion clamps onto the last line (gap can't render
            // below it); good enough until someone misses a trailing-line delete.
            let idx = h.new_start.min(line_count - 1);
            out[idx] = Some(ChangeKind::Removed(h.removed.len()));
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

/// Windows `canonicalize` yields `\\?\C:\…` verbatim paths git can't parse; drop
/// the drive-letter prefix. (UNC shares are left alone — an accepted edge.)
fn strip_verbatim(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    if let Some(rest) = p.to_str().and_then(|s| s.strip_prefix(r"\\?\"))
        && !rest.starts_with("UNC\\")
    {
        return PathBuf::from(rest);
    }
    p
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

        // New file has 6 lines (5 - 1 deleted + 1 added = ... conceptually).
        let changes = line_changes(&hunks, 6);
        assert_eq!(changes[1], Some(ChangeKind::Modified)); // line 2
        assert_eq!(changes[3], Some(ChangeKind::Added)); // line 4
        assert_eq!(changes[5], Some(ChangeKind::Removed(1))); // gap above line 6
        assert_eq!(changes[0], None);
        assert_eq!(changes[2], None);
        assert_eq!(changes[4], None);
    }
}
