//! A thread's card rows in a file pane: the rounded gray box of the thread's comments under its
//! line, which the TUI draws and the web app skips for its own card.

use ratatui::style::{Color, Style};
use ratatui::text::Span;

use super::{Author, Panel, PanelRow, wrap_line};
use crate::render::CODE_BG;

/// The name a card shows for a comment's author.
fn author_name(author: &Author) -> &str {
    match author {
        Author::User => "You",
        // Title-case the unnamed-agent fallback so it reads as a role beside "You";
        // real supplied names render verbatim. The chat payload keeps lowercase "agent".
        Author::Agent(n) if n == "agent" => "Agent",
        Author::Agent(n) => n.as_str(),
    }
}

impl Panel {
    /// Card rows for the expanded thread keyed on line `key` (empty when none/collapsed), tagged
    /// `line: at` — the block's last row, so the card follows the whole block (#70). A rounded gray
    /// box grown to the widest note; human notes are inline-code chips, agent notes plain fg.
    /// Emitted at column 0 — `view::panel_lines` prefixes the decoration, so `pad_w` is only a wrap budget.
    pub(super) fn thread_rows(&self, key: usize, at: usize, inner_w: usize) -> Vec<PanelRow> {
        let Some(t) = self.thread_at(key).filter(|t| !t.collapsed) else {
            return vec![];
        };
        let gutter_width = self.source_lines.len().max(1).to_string().len();
        let pad_w = gutter_width + 3; // number + sep + change + sep — card's left edge sits under the caret
        let cap = inner_w.saturating_sub(pad_w + 4).max(12); // two border + two padding columns
        let border = Style::default().fg(Color::Rgb(120, 120, 120)); // rounded box gray
        let plain = Style::default().fg(Color::Rgb(171, 178, 191)); // one dark fg
        let chip = Style::default().fg(Color::Rgb(180, 180, 180)).bg(CODE_BG);

        let width = |spans: &[Span<'static>]| -> usize {
            spans.iter().map(|s| s.content.chars().count()).sum()
        };

        // Inner note lines: wrap each line of `{name} · body`; continuations align under the note's
        // start (connector width only), not under the body.
        let mut inner: Vec<Vec<Span<'static>>> = vec![];
        for (idx, note) in t.notes().enumerate() {
            let is_user = matches!(note.author, Author::User);
            let connector = if idx == 0 { "" } else { "└─ " };
            let cw = connector.chars().count();
            let chip_pad = if is_user { 2 } else { 0 }; // the chip pads one space each side
            let full = format!("{} · {}", author_name(&note.author), note.body);
            for (j, seg) in full
                .lines()
                .flat_map(|l| wrap_line(l, cap.saturating_sub(cw + chip_pad)))
                .enumerate()
            {
                let mut spans = vec![];
                let lead = if j == 0 {
                    connector.to_string()
                } else {
                    " ".repeat(cw)
                };
                if !lead.is_empty() {
                    spans.push(Span::styled(lead, border));
                }
                spans.push(if is_user {
                    Span::styled(format!(" {seg} "), chip) // whole note as an inline-code chip
                } else {
                    Span::styled(seg, plain)
                });
                inner.push(spans);
            }
        }

        // Draw the rounded box, growing to the widest inner line. `line: at` so scroll/copy keep the
        // card with the block's last row; `comment: true` so cosmetics skip it and `view::panel_lines` indents it.
        let w = inner.iter().map(|s| width(s)).max().unwrap_or(0);
        let mk = |spans: Vec<Span<'static>>| PanelRow {
            line: at,
            gutter: None,
            spans,
            comment: true,
            change: None,
            review: None,
        };
        let mut rows = vec![mk(vec![Span::styled(
            format!("╭{}╮", "─".repeat(w + 2)),
            border,
        )])];
        for line in inner {
            let used = width(&line);
            let mut spans = vec![Span::styled("│ ".to_string(), border)];
            spans.extend(line);
            spans.push(Span::styled(format!("{} │", " ".repeat(w - used)), border));
            rows.push(mk(spans));
        }
        rows.push(mk(vec![Span::styled(
            format!("╰{}╯", "─".repeat(w + 2)),
            border,
        )]));
        rows
    }
}
