//! Project-wide content search and replace.
//!
//! Pure text work with no GPUI or filesystem-walking dependencies: the caller
//! supplies the file list and the text of any buffer that is open and possibly
//! dirty, and decides on which thread this runs. `src/app.rs` runs it on the
//! background executor and debounces input.

use regex::{Regex, RegexBuilder};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Files larger than this are skipped. Reading a 32 MiB file to show one match
/// costs the user more than not searching it.
pub const FILE_BYTES_LIMIT: u64 = 2 * 1024 * 1024;

/// Upper bounds on one search. Hitting either marks the outcome as truncated.
pub const MATCH_LIMIT: usize = 1_000;
pub const FILE_LIMIT: usize = 200;
pub const HITS_PER_FILE: usize = 100;

/// Context kept before a match when a long line has to be windowed.
const PREVIEW_LEAD: usize = 24;
/// Upper bound on one preview, in bytes.
const PREVIEW_LIMIT: usize = 300;

/// Which of the editor's search toggles are on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// The `Aa` toggle. Off by default, like most editors: searching code
    /// case-insensitively finds more than it hides.
    pub case_sensitive: bool,
    /// The `ab` toggle: only match where neither side is a word character.
    pub whole_word: bool,
    /// The `.*` toggle: treat the query as a regular expression.
    pub regex: bool,
}

/// One match, with everything a results row needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// 0-based line and character column of the match start.
    pub line: u32,
    pub column: u32,
    /// Line preview, indentation stripped and long lines windowed around the
    /// match. Tabs are expanded so columns line up in a proportional row.
    pub preview: String,
    /// Byte range of the match inside `preview`.
    pub start: usize,
    pub end: usize,
}

/// All hits in one file, in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHits {
    pub path: PathBuf,
    pub hits: Vec<Hit>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub files: Vec<FileHits>,
    /// The search stopped at [`MATCH_LIMIT`] or [`FILE_LIMIT`].
    pub truncated: bool,
}

/// A compiled search pattern. Cheap to clone and safe to share across threads.
#[derive(Debug)]
pub struct Matcher {
    regex: Regex,
    options: Options,
}

impl Matcher {
    pub fn new(query: &str, options: Options) -> Result<Self, String> {
        // Literal queries are escaped and go through the same engine, so
        // whole-word filtering and replacement behave identically in both modes.
        let pattern = if options.regex {
            query.to_string()
        } else {
            regex::escape(query)
        };
        let regex = RegexBuilder::new(&pattern)
            .case_insensitive(!options.case_sensitive)
            .build()
            .map_err(|error| {
                format!(
                    "正则表达式无效：{}",
                    error.to_string().replace('\n', " ").trim()
                )
            })?;
        Ok(Self { regex, options })
    }

    /// Whether a match survives the whole-word toggle.
    ///
    /// Done here rather than with `\b` in the pattern because the `regex` crate
    /// has no look-around, and `\b` would also bind to the wrong side when the
    /// query starts or ends with a non-word character.
    fn keeps(&self, text: &str, start: usize, end: usize) -> bool {
        if !self.options.whole_word {
            return true;
        }
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        !word(text[..start].chars().next_back()) && !word(text[end..].chars().next())
    }

    /// Every match in `text`, up to `budget` of them.
    pub fn hits(&self, text: &str, budget: usize) -> Vec<Hit> {
        let mut hits = Vec::new();
        if budget == 0 {
            return hits;
        }
        let line_starts = line_starts(text);
        for found in self.regex.find_iter(text) {
            let (start, end) = (found.start(), found.end());
            // Zero-width matches would highlight nothing and make replacement
            // ambiguous, so they are ignored.
            if start == end || !self.keeps(text, start, end) {
                continue;
            }
            let line = line_starts.partition_point(|&offset| offset <= start) - 1;
            let line_start = line_starts[line];
            let line_end = line_end(text, line_start);
            let (preview, mark, mark_end) = preview(
                &text[line_start..line_end],
                start - line_start,
                end.min(line_end) - line_start,
            );
            hits.push(Hit {
                line: line as u32,
                column: text[line_start..start].chars().count() as u32,
                preview,
                start: mark,
                end: mark_end,
            });
            if hits.len() >= budget {
                break;
            }
        }
        hits
    }

    /// Replace every match, returning the new text and how many were replaced.
    ///
    /// In regex mode `$1`, `${name}` and `$$` in `replacement` follow the
    /// `regex` crate's expansion rules; in literal mode it is inserted verbatim.
    pub fn replace(&self, text: &str, replacement: &str) -> (String, usize) {
        let mut updated = String::with_capacity(text.len());
        let mut copied = 0;
        let mut count = 0;
        for captures in self.regex.captures_iter(text) {
            let Some(found) = captures.get(0) else {
                continue;
            };
            if found.start() == found.end() || !self.keeps(text, found.start(), found.end()) {
                continue;
            }
            updated.push_str(&text[copied..found.start()]);
            if self.options.regex {
                captures.expand(replacement, &mut updated);
            } else {
                updated.push_str(replacement);
            }
            copied = found.end();
            count += 1;
        }
        updated.push_str(&text[copied..]);
        (updated, count)
    }
}

/// Search `files` for `matcher`.
///
/// `overlay` holds the text of open buffers: a dirty buffer must be searched as
/// it is on screen, not as it is on disk, or the reported positions would point
/// at text the user cannot see. Files missing from the overlay are read from
/// disk and skipped when they are unreadable, too large, or not UTF-8 text.
///
/// `should_continue` is polled once per file so a superseded search stops early
/// instead of competing with the next keystroke.
pub fn search_files(
    files: &[PathBuf],
    overlay: &std::collections::HashMap<PathBuf, String>,
    matcher: &Matcher,
    should_continue: impl Fn() -> bool,
) -> Outcome {
    let mut outcome = Outcome::default();
    let mut total = 0;
    for path in files {
        if !should_continue() {
            // Superseded; the caller discards this result anyway.
            return Outcome::default();
        }
        let text = match overlay.get(path) {
            Some(text) => std::borrow::Cow::Borrowed(text.as_str()),
            None => match read(path) {
                Ok(text) => std::borrow::Cow::Owned(text),
                Err(_) => continue,
            },
        };
        let hits = matcher.hits(&text, HITS_PER_FILE.min(MATCH_LIMIT - total));
        if hits.is_empty() {
            continue;
        }
        total += hits.len();
        outcome.files.push(FileHits {
            path: path.clone(),
            hits,
        });
        if total >= MATCH_LIMIT || outcome.files.len() >= FILE_LIMIT {
            outcome.truncated = true;
            break;
        }
    }
    outcome
}

fn read(path: &Path) -> io::Result<String> {
    if fs::metadata(path)?.len() > FILE_BYTES_LIMIT {
        return Err(io::Error::other("文件过大，已跳过搜索"));
    }
    crate::buffer::read(path)
}

fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(index, _)| index + 1))
        .collect()
}

fn line_end(text: &str, line_start: usize) -> usize {
    let end = text[line_start..]
        .find('\n')
        .map(|index| line_start + index)
        .unwrap_or(text.len());
    // Tolerate CRLF without leaving the carriage return in the preview.
    if end > line_start && text.as_bytes()[end - 1] == b'\r' {
        end - 1
    } else {
        end
    }
}

/// Trim a line to what a results row can show, keeping the match in view.
///
/// Returns the preview plus the match's byte range inside it. Leading
/// indentation is dropped silently; anything else that had to be cut is marked
/// with an ellipsis, and tabs are expanded to four spaces so a preview lines up
/// with the row above it.
fn preview(line: &str, start: usize, end: usize) -> (String, usize, usize) {
    let indent = line.len() - line.trim_start().len();
    // Keep indentation when the match itself lives inside it.
    let base = indent.min(start);
    let (window_start, window_end) = if line.len() - base <= PREVIEW_LIMIT {
        (base, line.len())
    } else {
        let mut window_start = base + (start - base).saturating_sub(PREVIEW_LEAD);
        while window_start > base && !line.is_char_boundary(window_start) {
            window_start -= 1;
        }
        let mut window_end = (window_start + PREVIEW_LIMIT).max(end).min(line.len());
        while window_end < line.len() && !line.is_char_boundary(window_end) {
            window_end += 1;
        }
        (window_start, window_end)
    };

    let mut text = String::with_capacity(window_end - window_start + 8);
    if window_start > base {
        text.push('…');
    }
    let mut mark = text.len();
    let mut mark_end = text.len();
    for (offset, character) in line[window_start..window_end].char_indices() {
        let absolute = window_start + offset;
        if absolute == start {
            mark = text.len();
        }
        if absolute == end {
            mark_end = text.len();
        }
        if character == '\t' {
            text.push_str("    ");
        } else {
            text.push(character);
        }
    }
    if end >= window_end {
        mark_end = text.len();
    }
    if window_end < line.len() {
        text.push('…');
    }
    (text, mark, mark_end.max(mark))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(regex: bool) -> Options {
        Options {
            regex,
            ..Options::default()
        }
    }

    fn lines(hits: &[Hit]) -> Vec<u32> {
        hits.iter().map(|hit| hit.line).collect()
    }

    #[test]
    fn literal_search_is_case_insensitive_until_asked_otherwise() {
        let text = "let Foo = 1;\nlet foo = 2;\nlet food = 3;\n";
        let hits = Matcher::new("foo", options(false)).unwrap().hits(text, 10);
        assert_eq!(lines(&hits), vec![0, 1, 2]);
        assert_eq!(hits[0].column, 4);

        let sensitive = Options {
            case_sensitive: true,
            ..Options::default()
        };
        let hits = Matcher::new("foo", sensitive).unwrap().hits(text, 10);
        assert_eq!(lines(&hits), vec![1, 2]);

        // Regex metacharacters are literal unless the `.*` toggle is on.
        let text = "a.b\naxb\n";
        assert_eq!(
            Matcher::new("a.b", options(false))
                .unwrap()
                .hits(text, 10)
                .len(),
            1
        );
        assert_eq!(
            Matcher::new("a.b", options(true))
                .unwrap()
                .hits(text, 10)
                .len(),
            2
        );
    }

    #[test]
    fn whole_word_bounds_both_sides() {
        let text = "food foo _foo foo_ (foo)\n";
        let whole = Options {
            whole_word: true,
            ..Options::default()
        };
        let hits = Matcher::new("foo", whole).unwrap().hits(text, 10);
        assert_eq!(
            hits.iter().map(|hit| hit.column).collect::<Vec<_>>(),
            vec![5, 20]
        );

        // A pattern with a non-word edge still gets bounded correctly, which a
        // `\b`-wrapped pattern could not do: `\b(foo\b` would not match "(foo)".
        let hits = Matcher::new("(foo", whole).unwrap().hits(text, 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].column, 19);
    }

    #[test]
    fn a_line_is_windowed_around_a_far_match() {
        let line = format!("{}needle{}", "-".repeat(400), "-".repeat(400));
        let text = format!("{line}\n");
        let hits = Matcher::new("needle", options(false))
            .unwrap()
            .hits(&text, 10);
        assert_eq!(hits.len(), 1);
        let hit = &hits[0];
        assert_eq!(&hit.preview[hit.start..hit.end], "needle");
        assert!(hit.preview.starts_with('…'), "{}", hit.preview);
        assert!(hit.preview.ends_with('…'));
        assert!(hit.preview.len() < 330);
    }

    #[test]
    fn indentation_is_stripped_and_tabs_expand() {
        let text = "\t\tlet value = 1;\n";
        let hits = Matcher::new("value", options(false))
            .unwrap()
            .hits(text, 10);
        assert_eq!(hits[0].preview, "let value = 1;");
        assert_eq!(&hits[0].preview[hits[0].start..hits[0].end], "value");
        assert_eq!(hits[0].column, 6);

        // Multi-byte text keeps byte ranges on character boundaries.
        let text = "  中文变量 = 1;\n";
        let hits = Matcher::new("中文", options(false)).unwrap().hits(text, 10);
        assert_eq!(hits[0].column, 2);
        assert_eq!(&hits[0].preview[hits[0].start..hits[0].end], "中文");
    }

    #[test]
    fn replacement_counts_matches_and_expands_captures() {
        let text = "let a = 1;\nlet b = 2;\n";
        let matcher = Matcher::new(r"let (\w+) =", options(true)).unwrap();
        let (updated, count) = matcher.replace(text, "const $1 =");
        assert_eq!(count, 2);
        assert_eq!(updated, "const a = 1;\nconst b = 2;\n");

        // Literal mode never expands `$`.
        let matcher = Matcher::new("a", options(false)).unwrap();
        let (updated, count) = matcher.replace("a", "$1");
        assert_eq!(count, 1);
        assert_eq!(updated, "$1");

        // Whole word applies to replacement too.
        let whole = Options {
            whole_word: true,
            ..Options::default()
        };
        let matcher = Matcher::new("foo", whole).unwrap();
        assert_eq!(matcher.replace("food foo", "X"), ("food X".into(), 1));
    }

    #[test]
    fn crlf_and_empty_queries_are_handled() {
        let hits = Matcher::new("b", options(false))
            .unwrap()
            .hits("a\r\nb\r\n", 10);
        assert_eq!(lines(&hits), vec![1]);
        assert_eq!(hits[0].preview, "b");

        // An empty query matches nothing, and zero-width matches are dropped so
        // that highlighting and replacement stay unambiguous.
        assert!(
            Matcher::new("", options(false))
                .unwrap()
                .hits("abc", 10)
                .is_empty()
        );
        assert!(
            Matcher::new("x*", options(true))
                .unwrap()
                .hits("abc", 10)
                .is_empty()
        );
        let hits = Matcher::new("a*", options(true)).unwrap().hits("aaa", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(&hits[0].preview[hits[0].start..hits[0].end], "aaa");
    }

    #[test]
    fn invalid_regex_reports_a_single_line_error() {
        let error = Matcher::new("(unclosed", options(true)).unwrap_err();
        assert!(error.starts_with("正则表达式无效："));
        assert!(!error.contains('\n'));
    }
}
