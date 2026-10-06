//! Human tables: a column list rendered with display-width alignment, an optional
//! leading marker column, terminal-only color and a trailing summary line.
//!
//! Callers build a `Table` by appending columns; each column is a header plus a
//! cell function over the caller's row type, so features add columns without
//! touching the renderer.
use serde_json::Value;
use std::ffi::OsStr;

/// Terminal styling for one cell. Ignored unless color is enabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Plain,
    Bold,
    Dim,
    Green,
    Yellow,
    Red,
}
impl Style {
    fn code(self) -> Option<&'static str> {
        match self {
            Style::Plain => None,
            Style::Bold => Some("1"),
            Style::Dim => Some("2"),
            Style::Green => Some("32"),
            Style::Yellow => Some("33"),
            Style::Red => Some("31"),
        }
    }
}

/// Text of one cell plus its terminal style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub style: Style,
}
impl Cell {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
    pub fn plain(text: impl Into<String>) -> Self {
        Self::new(text, Style::Plain)
    }
}

type CellFn<'a, R> = Box<dyn Fn(&R) -> Cell + 'a>;

struct Column<'a, R> {
    header: Cell,
    cell: CellFn<'a, R>,
}

/// An ordered column list over rows of type `R`.
pub struct Table<'a, R> {
    marker: Option<CellFn<'a, R>>,
    columns: Vec<Column<'a, R>>,
    summary: Option<String>,
    hidden: Vec<String>,
    width: Option<usize>,
}
impl<'a, R> Default for Table<'a, R> {
    fn default() -> Self {
        Self {
            marker: None,
            columns: Vec::new(),
            summary: None,
            hidden: Vec::new(),
            width: None,
        }
    }
}
impl<'a, R> Table<'a, R> {
    pub fn new() -> Self {
        Self::default()
    }
    /// Adds an unlabeled leading marker column (for example `@`/`^`).
    pub fn marker(mut self, cell: impl Fn(&R) -> Cell + 'a) -> Self {
        self.marker = Some(Box::new(cell));
        self
    }
    /// Appends a column with a bold header.
    pub fn column(mut self, header: &str, cell: impl Fn(&R) -> Cell + 'a) -> Self {
        self.columns.push(Column {
            header: Cell::new(header, Style::Bold),
            cell: Box::new(cell),
        });
        self
    }
    /// Sets the summary line printed after a blank line.
    pub fn summary(mut self, text: impl Into<String>) -> Self {
        self.summary = Some(text.into());
        self
    }
    /// Names a column left out of this view; the summary lists hidden columns.
    pub fn hidden(mut self, header: &str) -> Self {
        self.hidden.push(header.to_owned());
        self
    }
    /// Limits rendered lines to `width` display columns (None: unlimited) by
    /// hiding columns from the right; the first column is always shown.
    pub fn width(mut self, width: Option<usize>) -> Self {
        self.width = width;
        self
    }
    /// Renders header, rows and optional summary as lines without trailing padding.
    pub fn render(&self, rows: &[R], color: bool) -> Vec<String> {
        let mut grid: Vec<Vec<Cell>> = Vec::with_capacity(rows.len() + 1);
        let marker = |cell: Cell| self.marker.as_ref().map(|_| cell);
        grid.push(
            marker(Cell::plain(""))
                .into_iter()
                .chain(self.columns.iter().map(|c| c.header.clone()))
                .collect(),
        );
        for row in rows {
            grid.push(
                self.marker
                    .as_ref()
                    .map(|f| f(row))
                    .into_iter()
                    .chain(self.columns.iter().map(|c| (c.cell)(row)))
                    .collect(),
            );
        }
        let count = grid.first().map_or(0, Vec::len);
        let widths: Vec<usize> = (0..count)
            .map(|i| {
                grid.iter()
                    .map(|line| display_width(&line[i].text))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let gap = |i: usize| {
            if i == 0 && self.marker.is_some() {
                1
            } else {
                2
            }
        };
        let mut hidden = self.hidden.clone();
        if let Some(limit) = self.width {
            let line_width =
                |k: usize| widths[..k].iter().sum::<usize>() + (0..k - 1).map(gap).sum::<usize>();
            let first = usize::from(self.marker.is_some()) + 1;
            let mut keep = count;
            while keep > first && line_width(keep) > limit {
                keep -= 1;
            }
            hidden.extend(grid[0][keep..].iter().map(|c| c.text.clone()));
            for line in &mut grid {
                line.truncate(keep);
            }
        }
        let mut lines: Vec<String> = grid
            .iter()
            .map(|line| {
                let mut out = String::new();
                for (i, cell) in line.iter().enumerate() {
                    out.push_str(&paint(cell, color));
                    if i + 1 < line.len() {
                        let pad = widths[i] - display_width(&cell.text) + gap(i);
                        out.extend(std::iter::repeat_n(' ', pad));
                    }
                }
                out.truncate(out.trim_end_matches(' ').len());
                out
            })
            .collect();
        if let Some(summary) = &self.summary {
            let mut summary = summary.clone();
            if !hidden.is_empty() {
                summary.push_str(" · hidden: ");
                summary.push_str(&hidden.join(", "));
            }
            lines.push(String::new());
            lines.push(summary);
        }
        lines
    }
}

fn paint(cell: &Cell, color: bool) -> String {
    match cell.style.code() {
        Some(code) if color && !cell.text.is_empty() => {
            format!("\x1b[{code}m{}\x1b[0m", cell.text)
        }
        _ => cell.text.clone(),
    }
}

/// Color only on a terminal and only when `NO_COLOR` is unset or empty.
pub fn color_enabled(terminal: bool, no_color: Option<&OsStr>) -> bool {
    terminal && no_color.is_none_or(OsStr::is_empty)
}

/// Color decision for standard output in the current process.
pub fn stdout_color() -> bool {
    use std::io::IsTerminal;
    color_enabled(
        std::io::stdout().is_terminal(),
        std::env::var_os("NO_COLOR").as_deref(),
    )
}

/// The human `list` table over ProfileRecord values. Later columns append here.
pub fn profiles(records: &[Value], full: bool, color: bool, width: Option<usize>) -> Vec<String> {
    let upstream = records.iter().filter(|r| r["kind"] == "upstream").count();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut table = Table::new()
        .marker(|r: &Value| {
            if r["selected"] == true {
                Cell::new("@", Style::Green)
            } else if r["most_recent"] == true {
                Cell::new("^", Style::Yellow)
            } else {
                Cell::plain(" ")
            }
        })
        .column("Profile", |r: &Value| {
            Cell::new(r["name"].as_str().unwrap_or("?"), Style::Bold)
        })
        .column("Kind", |r: &Value| {
            Cell::plain(r["kind"].as_str().unwrap_or("?"))
        })
        .column("Token", |r: &Value| match r["token_present"].as_bool() {
            Some(true) => Cell::new("✓", Style::Green),
            Some(false) => Cell::new("–", Style::Dim),
            None => Cell::new("?", Style::Yellow),
        })
        .column("Launchers", |r: &Value| {
            let condition = launcher_summary(&r["launchers"]);
            let style = match condition {
                "ready" => Style::Green,
                "not_required" => Style::Dim,
                "missing" | "stale" => Style::Yellow,
                _ => Style::Red,
            };
            Cell::new(condition, style)
        })
        .column("Sets", |r: &Value| {
            let names: Vec<&str> = r["sets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            if names.is_empty() {
                Cell::new("—", Style::Dim)
            } else {
                Cell::plain(names.join(","))
            }
        })
        .column("Last used", move |r: &Value| {
            match r["last_used"].as_u64() {
                Some(at) => Cell::plain(relative_time(at, now)),
                None => Cell::new("never", Style::Dim),
            }
        });
    if full {
        table = table
            .column("Login", |r: &Value| {
                probe_cell(r, |p| match p["reported_logged_in"].as_bool()? {
                    true => Some(Cell::new("yes", Style::Green)),
                    false => Some(Cell::new("no", Style::Yellow)),
                })
            })
            .column("Auth", |r: &Value| {
                probe_cell(r, |p| p["auth_method"].as_str().map(Cell::plain))
            })
            .column("Claude dir", |r: &Value| {
                probe_cell(r, |p| p["config_directory"].as_str().map(Cell::plain))
            });
    }
    table
        .hidden("Path")
        .summary(format!(
            "○ {} profile{} · {upstream} upstream · @ selected here · ^ last used",
            records.len(),
            if records.len() == 1 { "" } else { "s" }
        ))
        .width(width)
        .render(records, color)
}

/// Relative age of a Unix-seconds time: `now` under a minute, then `Nm`, `Nh`, `Nd`.
/// A time in the future counts as `now`.
pub fn relative_time(at: u64, now: u64) -> String {
    match now.saturating_sub(at) {
        age if age < 60 => "now".into(),
        age if age < 3600 => format!("{}m", age / 60),
        age if age < 86400 => format!("{}h", age / 3600),
        age => format!("{}d", age / 86400),
    }
}

/// A probed cell: `—` when the row is not probed (aliases, retained records),
/// `?` when the probe failed or did not report the field.
fn probe_cell(record: &Value, field: impl Fn(&Value) -> Option<Cell>) -> Cell {
    match &record["probe"] {
        Value::Null => Cell::new("—", Style::Dim),
        probe => field(probe).unwrap_or_else(|| Cell::new("?", Style::Yellow)),
    }
}

/// `ready` when every required launcher is ready, else the worst condition
/// (`not_required` only when no launcher is required, as for retained records).
fn launcher_summary(launchers: &Value) -> &'static str {
    const RANK: [&str; 5] = ["ready", "missing", "stale", "collision", "unsafe"];
    let all = launchers.as_array().map_or(&[][..], Vec::as_slice);
    let required: Vec<&Value> = all
        .iter()
        .filter(|l| l["condition"] != "not_required")
        .collect();
    if required.is_empty() && !all.is_empty() {
        return "not_required";
    }
    required
        .iter()
        .map(|l| {
            RANK.iter()
                .position(|c| l["condition"] == *c)
                .unwrap_or(RANK.len() - 1)
        })
        .max()
        .map_or("ready", |i| RANK[i])
}

/// Terminal column width: East Asian wide/fullwidth and emoji count 2, combining
/// marks, zero-width and control characters 0, everything else 1.
pub fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}
fn char_width(c: char) -> usize {
    let c = c as u32;
    let zero = c < 0x20
        || (0x7f..0xa0).contains(&c)
        || (0x300..=0x36f).contains(&c)
        || (0x483..=0x489).contains(&c)
        || (0x591..=0x5bd).contains(&c)
        || (0x200b..=0x200f).contains(&c)
        || (0x2028..=0x202e).contains(&c)
        || (0x2060..=0x2064).contains(&c)
        || (0x20d0..=0x20ff).contains(&c)
        || (0xfe00..=0xfe0f).contains(&c)
        || (0xfe20..=0xfe2f).contains(&c)
        || c == 0xfeff
        || (0xe0100..=0xe01ef).contains(&c);
    let wide = (0x1100..=0x115f).contains(&c)
        || (0x2e80..=0x303e).contains(&c)
        || (0x3041..=0x33ff).contains(&c)
        || (0x3400..=0x4dbf).contains(&c)
        || (0x4e00..=0x9fff).contains(&c)
        || (0xa000..=0xa4cf).contains(&c)
        || (0xac00..=0xd7a3).contains(&c)
        || (0xf900..=0xfaff).contains(&c)
        || (0xfe30..=0xfe4f).contains(&c)
        || (0xff00..=0xff60).contains(&c)
        || (0xffe0..=0xffe6).contains(&c)
        || (0x1f300..=0x1f64f).contains(&c)
        || (0x1f900..=0x1f9ff).contains(&c)
        || (0x20000..=0x3fffd).contains(&c);
    if zero {
        0
    } else if wide {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_use_is_shown_as_a_coarse_relative_age() {
        let now = 1_800_000_000;
        for (age, text) in [
            (0, "now"),
            (59, "now"),
            (60, "1m"),
            (3599, "59m"),
            (3600, "1h"),
            (86399, "23h"),
            (86400, "1d"),
            (30 * 86400, "30d"),
        ] {
            assert_eq!(relative_time(now - age, now), text, "{age}");
        }
        assert_eq!(relative_time(now + 10, now), "now");
    }

    struct Row {
        name: &'static str,
        token: bool,
        marker: char,
    }
    fn rows() -> Vec<Row> {
        vec![
            Row {
                name: "personal",
                token: true,
                marker: '@',
            },
            Row {
                name: "w",
                token: false,
                marker: ' ',
            },
        ]
    }
    fn table<'a>() -> Table<'a, Row> {
        Table::new()
            .marker(|r: &Row| Cell::plain(r.marker.to_string()))
            .column("Profile", |r: &Row| Cell::new(r.name, Style::Bold))
            .column("Token", |r: &Row| {
                if r.token {
                    Cell::new("✓", Style::Green)
                } else {
                    Cell::new("–", Style::Dim)
                }
            })
            .column("Kind", |_: &Row| Cell::plain("owned"))
    }

    #[test]
    fn plain_rendering_aligns_by_display_width_and_ends_with_summary() {
        let lines = table()
            .summary("○ 2 profiles · @ selected here")
            .render(&rows(), false);
        assert_eq!(
            lines,
            [
                "  Profile   Token  Kind",
                "@ personal  ✓      owned",
                "  w         –      owned",
                "",
                "○ 2 profiles · @ selected here",
            ]
        );
    }

    #[test]
    fn a_width_limit_hides_trailing_columns_and_names_them_in_the_summary() {
        let lines = table()
            .hidden("Path")
            .summary("○ 2 profiles")
            .width(Some(17))
            .render(&rows(), false);
        assert_eq!(
            lines,
            [
                "  Profile   Token",
                "@ personal  ✓",
                "  w         –",
                "",
                "○ 2 profiles · hidden: Path, Kind",
            ]
        );
        let lines = table().width(Some(24)).render(&rows(), false);
        assert_eq!(lines[0], "  Profile   Token  Kind");
        let lines = table().width(Some(1)).render(&rows(), false);
        assert_eq!(lines[0], "  Profile");
    }

    #[test]
    fn without_marker_or_summary_there_is_no_extra_column_or_blank_line() {
        let lines = Table::new()
            .column("Name", |r: &Row| Cell::plain(r.name))
            .column("Wide", |_: &Row| Cell::plain("終x"))
            .render(&rows(), false);
        assert_eq!(lines, ["Name      Wide", "personal  終x", "w         終x"]);
    }

    #[test]
    fn color_wraps_styled_cells_without_changing_alignment() {
        let lines = table().render(&rows(), true);
        assert_eq!(
            lines[0],
            "  \x1b[1mProfile\x1b[0m   \x1b[1mToken\x1b[0m  \x1b[1mKind\x1b[0m"
        );
        assert_eq!(
            lines[1],
            "@ \x1b[1mpersonal\x1b[0m  \x1b[32m✓\x1b[0m      owned"
        );
        assert_eq!(
            lines[2],
            "  \x1b[1mw\x1b[0m         \x1b[2m–\x1b[0m      owned"
        );
    }

    #[test]
    fn color_only_on_a_terminal_without_no_color() {
        assert!(color_enabled(true, None));
        assert!(color_enabled(true, Some("".as_ref())));
        assert!(!color_enabled(true, Some("1".as_ref())));
        assert!(!color_enabled(false, None));
    }

    #[test]
    fn display_width_counts_wide_and_zero_width_characters() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("✓–—"), 3);
        assert_eq!(display_width("終端"), 4);
        assert_eq!(display_width("e\u{301}"), 1);
    }
}
