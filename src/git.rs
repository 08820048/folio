use std::{
    collections::HashMap,
    io::{self, Read, Seek},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

const OUTPUT_LIMIT: u64 = 32 * 1024 * 1024;
const QUERY_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const LINE_LIMIT: usize = 100_000;

fn base_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .args([
            "--no-pager",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=",
            "-c",
            "core.useBuiltinFSMonitor=false",
            "-c",
            NO_HOOKS,
            "-c",
            "core.pager=cat",
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
            "-c",
            "protocol.allow=never",
        ])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env_remove("GIT_EXTERNAL_DIFF");
    hide_console(&mut command);
    command
}

/// Git is a console program. Without this, every status poll flashes a
/// terminal on Windows.
fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

#[cfg(windows)]
const NO_HOOKS: &str = "core.hooksPath=nul";
#[cfg(not(windows))]
const NO_HOOKS: &str = "core.hooksPath=/dev/null";

/// Read-only Git must not run repository-provided hooks, diff drivers, or filters.
/// Config enumeration itself only reads data; bound it just like every other query.
pub(crate) fn command(root: &Path) -> io::Result<Command> {
    let names = run_limited(
        base_command(root).args([
            "config",
            "--includes",
            "--null",
            "--name-only",
            "--get-regexp",
            "^filter\\..*\\.(clean|smudge|process|required)$",
        ]),
        256 * 1024,
        QUERY_TIMEOUT,
    )?;
    if !names.status.success() && names.status.code() != Some(1) {
        return Err(io::Error::other("Could not read repository configuration"));
    }
    let mut command = base_command(root);
    for name in names
        .stdout
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .take(64)
    {
        let name = std::str::from_utf8(name).map_err(io::Error::other)?;
        command.arg("-c").arg(format!(
            "{name}={}",
            if name.ends_with(".required") {
                "false"
            } else {
                ""
            }
        ));
    }
    Ok(command)
}

pub(crate) fn output(command: &mut Command) -> io::Result<Output> {
    run_limited(command, OUTPUT_LIMIT, QUERY_TIMEOUT)
}

/// Spool output off-heap, checking size and deadline while the child runs.
/// The final bounded reads also catch a child that exits between checks.
fn run_limited(command: &mut Command, limit: u64, timeout: Duration) -> io::Result<Output> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let started = Instant::now();
    let status = (|| loop {
        if stdout.metadata()?.len() > limit || stderr.metadata()?.len() > limit {
            return Err(io::Error::other("Git output exceeds the size limit"));
        }
        if started.elapsed() >= timeout {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Git query timed out",
            ));
        }
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(10));
    })();
    if status.is_err() {
        #[cfg(unix)]
        if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    let status = status?;
    let read = |file: &mut std::fs::File| -> io::Result<Vec<u8>> {
        file.rewind()?;
        let mut bytes = Vec::new();
        file.take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(io::Error::other("Git output exceeds the size limit"));
        }
        Ok(bytes)
    };
    Ok(Output {
        status,
        stdout: read(&mut stdout)?,
        stderr: read(&mut stderr)?,
    })
}

pub(crate) fn check_lines(text: &str) -> io::Result<()> {
    if text.lines().take(LINE_LIMIT + 1).count() > LINE_LIMIT {
        return Err(io::Error::other("Git preview exceeds 100,000 lines"));
    }
    Ok(())
}

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
    let mut command = Command::new("git");
    command
        .args(["clone", "--", url])
        .arg(dest)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0");
    hide_console(&mut command);
    let output = run_limited(&mut command, OUTPUT_LIMIT, Duration::from_secs(600));
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
    let top = output(base_command(root).args(["rev-parse", "--show-toplevel"]));
    let top = match top {
        Ok(top) if top.status.success() => top,
        _ => return Ok(HashMap::new()),
    };
    let top = String::from_utf8_lossy(&top.stdout);
    if Path::new(top.trim_end()).canonicalize()? != root.canonicalize()? {
        return Ok(HashMap::new());
    }
    let output =
        output(command(root)?.args(["status", "--porcelain=v1", "-z", "--untracked-files=all"]));
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
        if status.len() >= LINE_LIMIT {
            return Err(io::Error::other("Git status exceeds 100,000 paths"));
        }
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

/// The file as `base` knew it (`HEAD:src/app.rs`). None if that revision
/// never had the path — a new file, or a name git will not show.
pub fn show_file(root: &Path, base: &str, path: &Path) -> io::Result<Option<String>> {
    let Ok(relative) = path.strip_prefix(root) else {
        return Ok(None);
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    if base.is_empty() || relative.is_empty() || base.starts_with('-') {
        return Ok(None);
    }
    let spec = format!("{base}:{relative}");
    let size = output(command(root)?.args(["cat-file", "-s", &spec]))?;
    if !size.status.success() {
        return Ok(None);
    }
    let size = String::from_utf8_lossy(&size.stdout)
        .trim()
        .parse::<u64>()
        .map_err(io::Error::other)?;
    if size > OUTPUT_LIMIT {
        return Err(io::Error::other("Git file exceeds 32 MB"));
    }
    let result = output(command(root)?.args(["cat-file", "blob", &spec]))?;
    if !result.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8(result.stdout).map_err(io::Error::other)?;
    check_lines(&text)?;
    Ok(Some(text))
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let output = output(command(root).ok()?.args(args)).ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn queries_do_not_execute_hooks_filters_or_diff_drivers() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let run = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(root)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        };
        run(&["init", "-q"]);
        std::fs::write(root.join("a.txt"), "before\n").unwrap();
        run(&["add", "a.txt"]);
        run(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@localhost",
            "commit",
            "-qm",
            "fixture",
        ]);
        std::fs::write(
            root.join("hook"),
            "#!/bin/sh\ntouch EXECUTED\nprintf 'token\\0'\n",
        )
        .unwrap();
        std::fs::set_permissions(root.join("hook"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        for key in [
            "core.fsmonitor",
            "diff.external",
            "diff.test.textconv",
            "filter.test.clean",
            "filter.test.process",
        ] {
            run(&["config", key, "./hook"]);
        }
        run(&["config", "filter.test.required", "true"]);
        std::fs::write(root.join(".gitattributes"), "*.txt filter=test diff=test\n").unwrap();
        std::fs::write(root.join("a.txt"), "after and a changed length\n").unwrap();
        assert!(status(root).unwrap().contains_key("a.txt"));
        assert!(
            !crate::diff::for_file(root, &root.join("a.txt"))
                .unwrap()
                .is_empty()
        );
        assert!(crate::blame::run(&root.join("a.txt"), "after\n").is_ok());
        assert!(!root.join("EXECUTED").exists());
    }

    #[cfg(unix)]
    #[test]
    fn child_output_and_runtime_are_bounded() {
        let overflow = run_limited(
            Command::new("sh").args(["-c", "printf '123456789'"]),
            8,
            Duration::from_secs(2),
        );
        assert!(overflow.is_err());
        let started = Instant::now();
        let timeout = run_limited(
            Command::new("sh").args(["-c", "sleep 30"]),
            128,
            Duration::from_millis(40),
        );
        assert_eq!(timeout.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(check_lines(&"x\n".repeat(LINE_LIMIT + 1)).is_err());
    }

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
        assert_eq!(
            show_file(root, "HEAD", &root.join("a")).unwrap().as_deref(),
            Some("a\n")
        );
        assert!(
            show_file(root, "HEAD", &root.join("missing"))
                .unwrap()
                .is_none()
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
        assert_eq!(
            std::fs::read_to_string(dest.join("readme")).unwrap(),
            "hi\n"
        );
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
