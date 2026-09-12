//! A file's changes against HEAD, for the read-only diff view.
//!
//! Git does the diffing. This only asks it about one file and lays the answer
//! out as rows the view can draw, so the parsing is testable without a window
//! and without a repository.

use crate::buffer;
use std::{io, path::Path, process::Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The `@@` line that opens a hunk, drawn as a row in its own right.
    Hunk,
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub change: Change,
    /// Line number in HEAD. Set for context and removed lines.
    pub old: Option<u32>,
    /// Line number in the working tree. Set for context and added lines.
    pub new: Option<u32>,
    pub text: String,
}

/// What the view has to show for one file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Diff {
    pub lines: Vec<Line>,
    /// Git does not track the file yet, so every line counts as an addition
    /// and there is nothing to compare them against.
    pub untracked: bool,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Diff one file against HEAD.
pub fn for_file(root: &Path, path: &Path) -> io::Result<Diff> {
    for_file_against(root, path, "HEAD")
}

/// Diff one file against `base` (`HEAD`, `main`, …).
pub fn for_file_against(root: &Path, path: &Path, base: &str) -> io::Result<Diff> {
    let Ok(relative) = path.strip_prefix(root) else {
        // Outside the project there is nothing to compare against.
        return Ok(Diff::default());
    };
    if is_untracked(root, relative)? {
        // Nothing to diff against, so the file is shown as all new. This is
        // the one case the view reads the file itself.
        return Ok(Diff {
            lines: all_added(&buffer::read(path)?),
            untracked: true,
        });
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["diff", "--no-color", "--unified=3", base, "--"])
        .arg(relative)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output();
    match output {
        Ok(output) if output.status.success() => Ok(Diff {
            lines: parse(&String::from_utf8_lossy(&output.stdout)),
            untracked: false,
        }),
        // Not a repository, a repository with no commit yet, or a file git
        // refuses to diff. There is nothing to show rather than something to
        // complain about.
        Ok(_) => Ok(Diff::default()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Diff::default()),
        Err(error) => Err(error),
    }
}

/// Whether git has never seen this path. Asked before diffing, because
/// `git diff HEAD` says nothing at all about an untracked file — which reads
/// exactly like a file with no changes.
fn is_untracked(root: &Path, relative: &Path) -> io::Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain=v1", "-z", "--"])
        .arg(relative)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output();
    match output {
        Ok(output) if output.status.success() => Ok(output.stdout.starts_with(b"??")),
        Ok(_) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Every line of a file git does not track, numbered as additions.
fn all_added(text: &str) -> Vec<Line> {
    text.lines()
        .enumerate()
        .map(|(index, text)| Line {
            change: Change::Added,
            old: None,
            new: Some(index as u32 + 1),
            text: text.to_string(),
        })
        .collect()
}

/// Lay out `git diff` output for one file as rows. The preamble naming the
/// file is dropped: the caller already knows which file it asked about.
pub fn parse(raw: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let (mut old, mut new) = (0u32, 0u32);
    let mut in_hunk = false;
    for raw in raw.lines() {
        if raw.starts_with("@@") {
            let Some((old_start, new_start)) = hunk_start(raw) else {
                in_hunk = false;
                continue;
            };
            old = old_start;
            new = new_start;
            in_hunk = true;
            lines.push(Line {
                change: Change::Hunk,
                old: None,
                new: None,
                text: raw.to_string(),
            });
            continue;
        }
        if !in_hunk {
            continue;
        }
        let Some((marker, text)) = raw.split_at_checked(1) else {
            continue;
        };
        let line = match marker {
            "+" => {
                let line = Line {
                    change: Change::Added,
                    old: None,
                    new: Some(new),
                    text: text.to_string(),
                };
                new += 1;
                line
            }
            "-" => {
                let line = Line {
                    change: Change::Removed,
                    old: Some(old),
                    new: None,
                    text: text.to_string(),
                };
                old += 1;
                line
            }
            " " => {
                let line = Line {
                    change: Change::Context,
                    old: Some(old),
                    new: Some(new),
                    text: text.to_string(),
                };
                old += 1;
                new += 1;
                line
            }
            // `\ No newline at end of file` and anything else is not a row.
            _ => continue,
        };
        lines.push(line);
    }
    lines
}

/// One row of a side-by-side view. A hunk header spans both columns;
/// a replace puts the deleted line on the left and the added one on
/// the right so they line up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitRow {
    pub hunk: Option<String>,
    pub old: Option<SplitCell>,
    pub new: Option<SplitCell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitCell {
    pub number: Option<u32>,
    pub text: String,
    pub change: Change,
}

/// Fold a unified diff into split rows. A run of removals followed by
/// additions is zipped; leftovers keep an empty opposite cell.
pub fn split_rows(lines: &[Line]) -> Vec<SplitRow> {
    let mut rows = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        match lines[index].change {
            Change::Hunk => {
                rows.push(SplitRow {
                    hunk: Some(lines[index].text.clone()),
                    old: None,
                    new: None,
                });
                index += 1;
            }
            Change::Context => {
                rows.push(SplitRow {
                    hunk: None,
                    old: Some(SplitCell {
                        number: lines[index].old,
                        text: lines[index].text.clone(),
                        change: Change::Context,
                    }),
                    new: Some(SplitCell {
                        number: lines[index].new,
                        text: lines[index].text.clone(),
                        change: Change::Context,
                    }),
                });
                index += 1;
            }
            Change::Removed | Change::Added => {
                let start = index;
                while index < lines.len() && lines[index].change == Change::Removed {
                    index += 1;
                }
                let removed = &lines[start..index];
                let added_at = index;
                while index < lines.len() && lines[index].change == Change::Added {
                    index += 1;
                }
                let added = &lines[added_at..index];
                for offset in 0..removed.len().max(added.len()) {
                    rows.push(SplitRow {
                        hunk: None,
                        old: removed.get(offset).map(|line| SplitCell {
                            number: line.old,
                            text: line.text.clone(),
                            change: Change::Removed,
                        }),
                        new: added.get(offset).map(|line| SplitCell {
                            number: line.new,
                            text: line.text.clone(),
                            change: Change::Added,
                        }),
                    });
                }
            }
        }
    }
    rows
}

/// Byte offsets of every hunk header, for next / previous.
pub fn hunk_indices(lines: &[Line]) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.change == Change::Hunk)
        .map(|(index, _)| index)
        .collect()
}

/// The starting line numbers out of `@@ -old,count +new,count @@ title`.
fn hunk_start(header: &str) -> Option<(u32, u32)> {
    let mut parts = header.strip_prefix("@@ -")?.split(' ');
    let old = parts.next()?.split(',').next()?.parse().ok()?;
    let new = parts
        .next()?
        .strip_prefix('+')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    Some((old, new))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(lines: &[Line]) -> Vec<(Option<u32>, Option<u32>)> {
        lines.iter().map(|line| (line.old, line.new)).collect()
    }

    #[test]
    fn a_hunk_numbers_both_sides_as_it_goes() {
        let raw = "\
diff --git a/src/main.rs b/src/main.rs
index 1111111..2222222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -10,5 +10,6 @@ fn main() {
 let one = 1;
-let two = 2;
+let two = 22;
+let three = 3;
 let four = 4;
";
        let lines = parse(raw);
        assert_eq!(lines[0].change, Change::Hunk);
        assert_eq!(lines[0].text, "@@ -10,5 +10,6 @@ fn main() {");
        // The preamble above the first hunk is dropped.
        assert!(lines.iter().all(|line| !line.text.starts_with("index ")));
        assert_eq!(
            numbers(&lines),
            vec![
                (None, None),         // the hunk header
                (Some(10), Some(10)), // context
                (Some(11), None),     // removed
                (None, Some(11)),     // added
                (None, Some(12)),     // added
                (Some(12), Some(13)), // context
            ]
        );
        assert_eq!(lines[3].text, "let two = 22;");
    }

    #[test]
    fn a_second_hunk_restarts_the_counters() {
        let raw = "@@ -1,2 +1,2 @@\n a\n-b\n+c\n@@ -50,1 +50,2 @@\n d\n+e\n";
        let lines = parse(raw);
        assert_eq!(
            numbers(&lines),
            vec![
                (None, None),
                (Some(1), Some(1)),
                (Some(2), None),
                (None, Some(2)),
                (None, None),
                (Some(50), Some(50)),
                (None, Some(51)),
            ]
        );
    }

    #[test]
    fn markers_that_are_not_rows_are_left_out() {
        // A hunk with no explicit counts, and git's end-of-file marker.
        let raw = "@@ -1 +1 @@\n-a\n+b\n\\ No newline at end of file\n";
        let lines = parse(raw);
        assert_eq!(lines.len(), 3);
        assert!(
            lines
                .iter()
                .all(|line| line.text != "\\ No newline at end of file")
        );
        assert_eq!(lines[0].text, "@@ -1 +1 @@");
    }

    #[test]
    fn an_empty_or_header_only_diff_has_no_rows() {
        assert!(parse("").is_empty());
        assert!(parse("diff --git a/x b/x\n--- a/x\n+++ b/x\n").is_empty());
        // A malformed hunk header stops the parse rather than guessing.
        assert_eq!(parse("@@ nonsense\n+a\n").len(), 0);
    }

    #[test]
    fn a_replace_lines_up_in_a_split() {
        let lines = parse("@@ -1,3 +1,3 @@\n one\n-old\n+new\n two\n");
        let rows = split_rows(&lines);
        assert_eq!(rows[0].hunk.as_deref(), Some("@@ -1,3 +1,3 @@"));
        assert_eq!(rows[1].old.as_ref().map(|cell| cell.text.as_str()), Some("one"));
        assert_eq!(rows[1].new.as_ref().map(|cell| cell.text.as_str()), Some("one"));
        assert_eq!(rows[2].old.as_ref().unwrap().text, "old");
        assert_eq!(rows[2].new.as_ref().unwrap().text, "new");
        assert_eq!(rows[2].old.as_ref().unwrap().change, Change::Removed);
        assert_eq!(rows[2].new.as_ref().unwrap().change, Change::Added);
        assert_eq!(hunk_indices(&lines), vec![0]);
    }

    #[test]
    fn a_pure_addition_leaves_the_left_empty() {
        let lines = parse("@@ -1,1 +1,2 @@\n keep\n+add\n");
        let rows = split_rows(&lines);
        assert!(rows[2].old.is_none());
        assert_eq!(rows[2].new.as_ref().unwrap().text, "add");
    }

    #[test]
    fn an_untracked_file_is_all_additions() {
        let lines = all_added("one\ntwo\n\nfour\n");
        assert_eq!(lines.len(), 4);
        assert!(lines.iter().all(|line| line.change == Change::Added));
        assert!(lines.iter().all(|line| line.old.is_none()));
        assert_eq!(
            lines.iter().map(|line| line.new).collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(3), Some(4)]
        );
    }
}
