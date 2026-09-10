use folio::{buffer, recent, tree, workspace::Workspace};
use std::fs;

#[test]
fn project_read_edit_save_and_recent_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("项目\twith spaces");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("target")).unwrap();
    let file = root.join("src/main.rs");
    fs::write(&file, "// 你好\r\nfn main() {}\r\n").unwrap();
    let ws = Workspace::open(&root).unwrap();
    assert_eq!(tree::children(&root).unwrap().len(), 1);
    assert_eq!(tree::index(&root).unwrap(), vec![file.clone()]);
    assert!(ws.resolve(temp.path()).is_err());
    let original = buffer::read(&file).unwrap();
    let edited = original.replace("你好", "Folio");
    buffer::save(&file, &edited, &original).unwrap();
    assert_eq!(buffer::read(&file).unwrap(), edited);
    assert!(buffer::save(&file, "stale overwrite", &original).is_err());
    assert_eq!(buffer::read(&file).unwrap(), edited);
    fs::write(&file, [0xff, 0xfe]).unwrap();
    assert!(buffer::read(&file).is_err());
    fs::write(&file, b"binary\0").unwrap();
    assert!(buffer::read(&file).is_err());
    let config = temp.path().join("settings/recent.json");
    for i in 0..10 {
        let p = root.join(i.to_string());
        fs::create_dir(&p).unwrap();
        recent::record(&p, &config).unwrap();
    }
    recent::record(&root, &config).unwrap();
    let items = recent::record(&root, &config).unwrap();
    assert_eq!(items.len(), 8);
    assert_eq!(items[0].path, root.canonicalize().unwrap());
    assert_eq!(recent::remove(&items[0].path, &config).unwrap().len(), 7);
    assert!(tree::fuzzy_match("sMR", "src/main.rs"));
    assert!(!tree::fuzzy_match("rsm", "src/main.rs"));
    assert_eq!(buffer::language(&file), "rust");
}

#[test]
fn git_status_and_save_failures_keep_existing_data() {
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
    fs::write(root.join("old name.rs"), "fn main() {}\n").unwrap();
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
    assert!(git(&["mv", "old name.rs", "new name.rs"]).status.success());
    fs::write(root.join("未跟踪.txt"), "hello").unwrap();
    let status = folio::git::status(root).unwrap();
    assert_eq!(status.get("new name.rs"), Some(&'M'));
    assert_eq!(status.get("未跟踪.txt"), Some(&'U'));
    fs::create_dir(root.join("nested")).unwrap();
    assert!(folio::git::status(&root.join("nested")).unwrap().is_empty());
    let config = root.join("recent.json");
    fs::write(&config, "corrupt but recoverable").unwrap();
    assert!(recent::record(root, &config).is_err());
    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        "corrupt but recoverable"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let path = root.join("new name.rs");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        buffer::save(&path, "// edited\n", "fn main() {}\n").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o755
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        assert!(buffer::save(&path, "lost", "// edited\n").is_err());
        assert_eq!(buffer::read(&path).unwrap(), "// edited\n");
        symlink(root.parent().unwrap(), root.join("escape")).unwrap();
        assert!(
            Workspace::open(root)
                .unwrap()
                .resolve(&root.join("escape"))
                .is_err()
        );
    }
}
