use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    process::Command,
};

/// A GitHub repository the user asked to clone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubRepo {
    /// What `git clone` is given. HTTPS unless they pasted SSH.
    pub url: String,
    /// Folder name under the directory they pick.
    pub name: String,
}

/// Read a GitHub URL or `owner/repo`. Anything else is rejected — Folio
/// does not log in or talk to the GitHub API, it only hands `git` an address.
pub fn parse_github(input: &str) -> Result<GithubRepo, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Paste a GitHub URL or owner/repo".into());
    }
    if input.starts_with('-') {
        return Err("Not a GitHub repository".into());
    }

    if let Some(rest) = input.strip_prefix("git@github.com:") {
        return owner_repo(rest, CloneUrl::Ssh);
    }
    if let Some(rest) = input
        .strip_prefix("ssh://git@github.com/")
        .or_else(|| input.strip_prefix("ssh://github.com/"))
    {
        return owner_repo(rest, CloneUrl::Ssh);
    }

    let rest = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .unwrap_or(input);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    if let Some(rest) = rest.strip_prefix("github.com/") {
        return owner_repo(rest, CloneUrl::Https);
    }
    if rest.contains("://") || rest.contains('@') || rest.contains("github.com") {
        return Err("Only GitHub repositories".into());
    }
    owner_repo(rest, CloneUrl::Https)
}

enum CloneUrl {
    Https,
    Ssh,
}

fn owner_repo(rest: &str, kind: CloneUrl) -> Result<GithubRepo, String> {
    let rest = rest.trim_matches('/');
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let mut parts = rest.split('/').filter(|part| !part.is_empty());
    let owner = parts.next().unwrap_or("");
    let repo = parts
        .next()
        .map(|name| name.strip_suffix(".git").unwrap_or(name))
        .unwrap_or("");
    // Users and orgs have no dots; a dotted first segment is a hostname
    // (`gitlab.com/…`), not `owner/repo`.
    if !github_owner(owner) || !github_segment(repo) {
        return Err("Paste a GitHub URL or owner/repo".into());
    }
    let url = match kind {
        CloneUrl::Https => format!("https://github.com/{owner}/{repo}.git"),
        CloneUrl::Ssh => format!("git@github.com:{owner}/{repo}.git"),
    };
    Ok(GithubRepo {
        url,
        name: repo.to_string(),
    })
}

fn github_owner(name: &str) -> bool {
    github_segment(name) && !name.contains('.')
}

fn github_segment(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// `git clone -- url dest`. Fails if `dest` already exists, the same
/// rule New Project uses: that folder is Open, not Clone.
pub fn clone(url: &str, dest: &Path) -> io::Result<PathBuf> {
    if url.is_empty() || url.starts_with('-') {
        return Err(io::Error::other("Not a GitHub repository"));
    }
    if dest.exists() {
        return Err(io::Error::other("That folder already exists"));
    }
    let Some(parent) = dest.parent() else {
        return Err(io::Error::other("Choose a folder to put the project in"));
    };
    if !parent.is_dir() {
        return Err(io::Error::other("Choose a folder to put the project in"));
    }
    let output = Command::new("git")
        .args(["clone", "--", url])
        .arg(dest)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output();
    let output = match output {
        Ok(output) => output,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(io::Error::other("git is not installed"));
        }
        Err(e) => return Err(e),
    };
    if !output.status.success() {
        return Err(io::Error::other(clone_error(&output.stderr)));
    }
    Ok(dest.to_path_buf())
}

fn clone_error(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .map(str::trim)
        .rev()
        .find(|line| !line.is_empty() && !line.starts_with("Cloning into"))
        .unwrap_or("Could not clone the repository")
        .trim_start_matches("fatal: ")
        .to_string()
}

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
    fn parse_github_reads_a_url_or_owner_repo() {
        let https = GithubRepo {
            url: "https://github.com/zed-industries/zed.git".into(),
            name: "zed".into(),
        };
        for input in [
            "https://github.com/zed-industries/zed",
            "https://github.com/zed-industries/zed.git",
            "https://github.com/zed-industries/zed/",
            "https://github.com/zed-industries/zed/tree/main",
            "https://www.github.com/zed-industries/zed",
            "github.com/zed-industries/zed",
            "  zed-industries/zed  ",
        ] {
            assert_eq!(parse_github(input).unwrap(), https, "{input}");
        }
        assert_eq!(
            parse_github("git@github.com:zed-industries/zed.git").unwrap(),
            GithubRepo {
                url: "git@github.com:zed-industries/zed.git".into(),
                name: "zed".into(),
            }
        );
        assert_eq!(
            parse_github("ssh://git@github.com/zed-industries/zed.git").unwrap(),
            GithubRepo {
                url: "git@github.com:zed-industries/zed.git".into(),
                name: "zed".into(),
            }
        );
        assert!(parse_github("").is_err());
        assert!(parse_github("zed").is_err());
        assert!(parse_github("https://gitlab.com/zed-industries/zed").is_err());
        assert!(parse_github("-e github.com/zed-industries/zed").is_err());
    }

    #[test]
    fn clone_copies_a_local_repo_and_refuses_an_existing_folder() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&source)
                .args(args)
                .output()
                .unwrap()
        };
        assert!(git(&["init", "-q"]).status.success());
        std::fs::write(source.join("readme"), "hi\n").unwrap();
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
        let dest = temp.path().join("zed");
        clone(source.to_str().unwrap(), &dest).unwrap();
        assert_eq!(std::fs::read_to_string(dest.join("readme")).unwrap(), "hi\n");
        assert!(clone(source.to_str().unwrap(), &dest).is_err());
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
