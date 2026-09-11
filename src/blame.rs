//! `git blame`, for the line under the cursor.
//!
//! Read through `git blame --porcelain`, which reports one block per commit and
//! the lines it covers, parsed into one entry per line of the file.
//!
//! The text blamed is the buffer's, not the file's. They differ as soon as
//! anything is typed, and blame is looked up by line number — a number that
//! means the buffer has to be answered against the buffer, or every line below
//! an edit would be attributed to the wrong commit. `git blame --contents` is
//! what makes that possible: git blames the contents it is handed, against the
//! history of the file's path.

use std::{
    collections::HashMap,
    io::{self, Write},
    path::Path,
    process::Command,
};

/// The commit that last wrote one line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    /// The full hash; [`Line::short`] is what is shown.
    pub commit: String,
    pub author: String,
    /// Seconds since the epoch, which is what `author-time` reports.
    pub time: u64,
    pub summary: String,
}

impl Line {
    /// The first eight characters of the hash, which is what git itself prints
    /// when it is asked for a short one.
    pub fn short(&self) -> &str {
        self.commit.get(..8).unwrap_or(&self.commit)
    }

    /// Whether the line is not committed at all: blame reports those with a
    /// hash of nothing but zeroes.
    pub fn uncommitted(&self) -> bool {
        self.commit.is_empty() || self.commit.chars().all(|c| c == '0')
    }
}

/// Every line of a file, in order, with the commit that last wrote it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Blame {
    pub lines: Vec<Line>,
}

impl Blame {
    /// The blame for a line, counting from zero.
    pub fn line(&self, index: usize) -> Option<&Line> {
        self.lines.get(index)
    }
}

/// Blame `text` as if it were the contents of `path`.
pub fn run(path: &Path, text: &str) -> io::Result<Blame> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Invalid file path"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("Invalid file path"))?;

    // `--contents` reads a file, so the buffer is written to one first. It does
    // not have to be anywhere in particular, and it is gone by the time this
    // returns.
    let mut contents = tempfile::NamedTempFile::new()?;
    contents.write_all(text.as_bytes())?;
    contents.flush()?;

    let output = Command::new("git")
        .arg("-C")
        .arg(parent)
        .args(["blame", "--porcelain", "--contents"])
        .arg(contents.path())
        .arg("--")
        .arg(name)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(io::Error::other(if message.is_empty() {
            "Not a git repository, or the file is not tracked".to_string()
        } else {
            message
        }));
    }
    Ok(parse(&String::from_utf8_lossy(&output.stdout)))
}

/// Read `git blame --porcelain` output.
///
/// A commit's details are written out the first time it appears and left out
/// afterwards — a file where one commit wrote twenty lines in a row has them
/// once — so what has been seen is kept by hash and filled in again.
pub fn parse(output: &str) -> Blame {
    let rows: Vec<&str> = output.lines().collect();
    let mut seen: HashMap<String, Line> = HashMap::new();
    let mut lines = Vec::new();
    let mut at = 0;

    while at < rows.len() {
        let Some((hash, _)) = header(rows[at]) else {
            at += 1;
            continue;
        };
        let mut line = seen.get(&hash).cloned().unwrap_or_default();
        line.commit = hash.clone();
        at += 1;

        // The details, until the line's own text or the next line's header.
        while at < rows.len() && !rows[at].starts_with('\t') && header(rows[at]).is_none() {
            if let Some((key, value)) = rows[at].split_once(' ') {
                match key {
                    "author" => line.author = value.to_string(),
                    "author-time" => line.time = value.parse().unwrap_or(0),
                    "summary" => line.summary = value.to_string(),
                    _ => {}
                }
            }
            at += 1;
        }
        // The line itself, which is not needed: the file being blamed is the
        // one being shown.
        if at < rows.len() && rows[at].starts_with('\t') {
            at += 1;
        }

        seen.insert(hash, line.clone());
        lines.push(line);
    }

    Blame { lines }
}

/// The `<hash> <original line> <final line> [<lines in this group>]` that starts
/// every entry, if this is one.
fn header(row: &str) -> Option<(String, usize)> {
    let mut fields = row.split(' ');
    let hash = fields.next()?;
    if hash.len() != 40 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let _original = fields.next()?;
    let final_line = fields.next()?.parse().ok()?;
    Some((hash.to_string(), final_line))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two commits: the first wrote three lines, the second rewrote one.
    const PORCELAIN: &str = "\
d1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0 1 1 3
author Ada Lovelace
author-mail <ada@example.com>
author-time 1700000000
author-tz +0000
committer Ada Lovelace
committer-mail <ada@example.com>
committer-time 1700000000
committer-tz +0000
summary Write the first version
filename main.rs
\tfn main() {
d1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0 2 2
\t    println!(\"hi\");
0f1e2d3c4b5a69788796a5b4c3d2e1f009182736 3 3 1
author Grace Hopper
author-mail <grace@example.com>
author-time 1710000000
author-tz +0000
committer Grace Hopper
committer-mail <grace@example.com>
committer-time 1710000000
committer-tz +0000
summary Say hi properly
filename main.rs
\t    println!(\"hello\");
0000000000000000000000000000000000000000 4 4 1
author Not Committed Yet
author-mail <not.committed.yet>
author-time 1720000000
author-tz +0000
committer Not Committed Yet
committer-mail <not.committed.yet>
committer-time 1720000000
committer-tz +0000
summary Version of main.rs from main.rs
filename main.rs
\t}
";

    #[test]
    fn a_commit_is_described_once_and_reused() {
        let blame = parse(PORCELAIN);
        assert_eq!(blame.lines.len(), 4);

        // The second line belongs to the same commit as the first, and its
        // details were not written out again.
        assert_eq!(blame.lines[0].author, "Ada Lovelace");
        assert_eq!(blame.lines[1].author, "Ada Lovelace");
        assert_eq!(blame.lines[1].summary, "Write the first version");
        assert_eq!(blame.lines[0].time, 1700000000);

        assert_eq!(blame.lines[2].author, "Grace Hopper");
        assert_eq!(blame.lines[2].summary, "Say hi properly");
        assert_eq!(blame.line(2).map(Line::short), Some("0f1e2d3c"));
    }

    #[test]
    fn a_line_that_is_not_committed_says_so() {
        let blame = parse(PORCELAIN);
        assert!(blame.lines[3].uncommitted());
        assert!(!blame.lines[0].uncommitted());
        assert_eq!(blame.line(9), None);
    }

    #[test]
    fn nothing_to_blame_is_no_lines() {
        // Outside a repository, or with the file untracked, the output is empty.
        assert_eq!(parse(""), Blame::default());
        assert_eq!(parse("fatal: not a git repository\n"), Blame::default());
    }
}
