use std::{collections::HashMap, io, path::Path, process::Command};

pub fn status(root: &Path) -> io::Result<HashMap<String, char>> {
    let top = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .output();
    let top = match top {
        Ok(top) if top.status.success() => top,
        _ => return Ok(HashMap::new()),
    };
    let top = String::from_utf8_lossy(&top.stdout);
    if Path::new(top.trim_end()).canonicalize()? != root.canonicalize()? {
        return Ok(HashMap::new());
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output();
    let output = match output {
        Ok(o) => o,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(e),
    };
    if !output.status.success() {
        return Ok(HashMap::new());
    }
    let mut status = HashMap::new();
    let mut records = output.stdout.split(|b| *b == 0);
    while let Some(record) = records.next() {
        if record.len() < 4 {
            continue;
        }
        let code = if &record[..2] == b"??" { 'U' } else { 'M' };
        status.insert(String::from_utf8_lossy(&record[3..]).into_owned(), code);
        if record[..2].contains(&b'R') || record[..2].contains(&b'C') {
            records.next();
        }
    }
    Ok(status)
}

/// Dirty and untracked paths, sorted, so the changes list does not jump.
pub fn changed_paths(status: &HashMap<String, char>) -> Vec<(String, char)> {
    let mut files: Vec<_> = status
        .iter()
        .map(|(path, kind)| (path.clone(), *kind))
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// The branch a review against "main" should use: `origin/HEAD`, then
/// `main`, then `master`. None if the repo has no such name.
pub fn review_branch(root: &Path) -> Option<String> {
    if let Some(name) = git_stdout(root, &["rev-parse", "--abbrev-ref", "origin/HEAD"]) {
        // Keep `origin/main`, not local `main`: the local branch may
        // not exist, and the remote is what a review against trunk means.
        let name = name.trim().to_string();
        if !name.is_empty() && name != "HEAD" {
            return Some(name);
        }
    }
    for name in ["main", "master"] {
        if git_stdout(root, &["rev-parse", "--verify", name]).is_some() {
            return Some(name.to_string());
        }
    }
    None
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_branch_uses_the_repo_default() {
        use std::process::Command;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .output()
                .unwrap()
        };
        assert!(git(&["init", "-q"]).status.success());
        std::fs::write(root.join("a"), "a\n").unwrap();
        assert!(git(&["add", "."]).status.success());
        assert!(
            git(&[
                "-c",
                "user.name=Folio Test",
                "-c",
                "user.email=test@localhost",
                "commit",
                "-qm",
                "fixture"
            ])
            .status
            .success()
        );
        let name = review_branch(root).expect("a new repo has a default branch");
        assert!(
            name == "main" || name == "master",
            "unexpected default branch {name}"
        );
    }

    #[test]
    fn changed_paths_are_sorted() {
        let mut status = HashMap::new();
        status.insert("src/b.rs".into(), 'M');
        status.insert("a.rs".into(), 'U');
        status.insert("src/a.rs".into(), 'M');
        assert_eq!(
            changed_paths(&status),
            vec![
                ("a.rs".into(), 'U'),
                ("src/a.rs".into(), 'M'),
                ("src/b.rs".into(), 'M'),
            ]
        );
    }
}
