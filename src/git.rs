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
