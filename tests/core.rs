use folio::{buffer, fs_op, recent, search, settings::Settings, tree, workspace::Workspace};
use std::fs;

/// The ignore list the app starts from, which most tests do not care about.
fn ignored() -> Vec<String> {
    Settings::default().ignored
}

#[test]
fn project_read_edit_save_and_recent_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("项目\twith spaces");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("target")).unwrap();
    let file = root.join("src/main.rs");
    fs::write(&file, "// 你好\r\nfn main() {}\r\n").unwrap();
    let ws = Workspace::open(&root).unwrap();
    assert_eq!(tree::children(&root, &ignored()).unwrap().len(), 1);
    assert_eq!(tree::index(&root, &ignored()).unwrap(), vec![file.clone()]);
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
fn project_search_prefers_open_buffers_and_skips_unusable_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let text = root.join("text.rs");
    let binary = root.join("binary.bin");
    let huge = root.join("huge.rs");
    let dirty = root.join("dirty.rs");
    fs::write(&text, "let needle = 1;\n").unwrap();
    fs::write(&binary, b"needle\0\xff").unwrap();
    fs::write(
        &huge,
        format!("needle\n{}", "x".repeat(search::FILE_BYTES_LIMIT as usize)),
    )
    .unwrap();
    fs::write(&dirty, "let needle = 1;\n").unwrap();

    let files = vec![text.clone(), binary, huge, dirty.clone()];
    // The open buffer differs from disk: the search must use the buffer.
    let overlay = [(dirty.clone(), "let edited = 2;\nneedle here\n".to_string())]
        .into_iter()
        .collect();
    let matcher = search::Matcher::new("needle", search::Options::default()).unwrap();
    let outcome = search::search_files(&files, &overlay, &matcher, || true);

    assert_eq!(
        outcome
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>(),
        vec![text, dirty],
        "binary and oversized files are skipped without failing the search"
    );
    assert_eq!(
        outcome.files[1].hits[0].line, 1,
        "the buffer on screen wins"
    );
    assert!(!outcome.truncated);

    // A cancelled scan reports nothing rather than a partial list.
    let cancelled = search::search_files(&files, &overlay, &matcher, || false);
    assert!(cancelled.files.is_empty());
}

#[test]
fn language_mapping_names_registered_grammars() {
    let case = |path: &str, expected: &str| {
        assert_eq!(
            buffer::language(std::path::Path::new(path)),
            expected,
            "{path}"
        );
    };
    // Whole-file names, including ones with no extension at all.
    case("Dockerfile", "dockerfile");
    case("Dockerfile.dev", "dockerfile");
    case("Containerfile", "dockerfile");
    case("Makefile", "make");
    case("CMakeLists.txt", "cmake");
    case("Jenkinsfile", "text"); // no Groovy grammar is available
    case("Gemfile", "ruby");
    case(".zshrc", "bash");
    case(".editorconfig", "ini");
    case(".vimrc", "vim");
    case(".Rprofile", "r");
    case("Cargo.lock", "toml");
    case(".env", "ini");
    case(".env.local", "ini");
    case("COMMIT_EDITMSG", "gitcommit");
    // Extensions.
    case("a.xml", "xml");
    case("a.xaml", "xml");
    case("a.dtd", "dtd");
    case("a.scss", "scss");
    case("a.sass", "scss");
    case("a.less", "less");
    case("a.vue", "vue");
    case("a.nix", "nix");
    case("a.dart", "dart");
    case("a.r", "r");
    case("a.hs", "haskell");
    case("a.ml", "ocaml");
    case("a.mli", "ocaml");
    case("a.erl", "erlang");
    case("a.elm", "elm");
    case("a.gleam", "gleam");
    case("a.sol", "solidity");
    case("a.m", "objc");
    case("a.s", "asm");
    case("a.ps1", "powershell");
    case("config.fish", "fish");
    case("a.regex", "regex");
    case("a.ipynb", "json");
    case("a.json5", "json");
    // Uppercase .C / .H stay C++, everything else is case-insensitive.
    case("a.C", "cpp");
    case("a.H", "cpp");
    case("a.c", "c");
    case("a.XML", "xml");
    // Unknown extensions must not go through SyntaxHighlighter's error path.
    case("a.unknown-extension", "text");
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

#[test]
fn context_menu_file_operations_never_clobber() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("项目 ops");
    fs::create_dir_all(&root).unwrap();

    // Names that would escape the containing folder, or address nothing.
    assert!(fs_op::validate_name("").is_err());
    assert!(fs_op::validate_name("   ").is_err());
    assert!(fs_op::validate_name("..").is_err());
    assert!(fs_op::validate_name("a/b").is_err());
    assert!(fs_op::validate_name("笔记.md").is_ok());

    let file = fs_op::create_file(&root, "笔记.md").unwrap();
    // Creating again must fail rather than truncate what is already there.
    assert!(fs_op::create_file(&root, "笔记.md").is_err());
    assert!(fs_op::create_dir(&root, "笔记.md").is_err());
    fs::write(&file, "hello").unwrap();

    assert!(fs_op::create_dir(&root, "稿件").is_ok());
    assert!(fs_op::create_dir(&root, "稿件").is_err());

    // Duplicates land beside the original and bump the suffix instead of
    // colliding with an earlier copy.
    let copy = fs_op::duplicate(&file).unwrap();
    assert_eq!(copy.file_name().unwrap().to_string_lossy(), "笔记 copy.md");
    assert_eq!(fs::read_to_string(&copy).unwrap(), "hello");
    let second = fs_op::duplicate(&file).unwrap();
    assert_eq!(
        second.file_name().unwrap().to_string_lossy(),
        "笔记 copy 2.md"
    );

    // A folder duplicate carries its whole tree.
    fs::create_dir_all(root.join("src/inner")).unwrap();
    fs::write(root.join("src/inner/deep.txt"), "deep").unwrap();
    let tree_copy = fs_op::duplicate(&root.join("src")).unwrap();
    assert_eq!(
        fs::read_to_string(tree_copy.join("inner/deep.txt")).unwrap(),
        "deep"
    );

    let dest = root.join("dest");
    fs::create_dir(&dest).unwrap();
    let pasted = fs_op::paste(&copy, &dest, false).unwrap();
    assert!(copy.is_file(), "a copy leaves the source in place");
    assert_eq!(pasted.file_name().unwrap(), copy.file_name().unwrap());
    // The name is taken, so a second paste refuses instead of overwriting.
    assert!(fs_op::paste(&copy, &dest, false).is_err());

    // A cut moves the entry and leaves nothing behind.
    fs::remove_file(&pasted).unwrap();
    let moved = fs_op::paste(&copy, &dest, true).unwrap();
    assert!(!copy.exists());
    assert!(moved.is_file());

    assert!(fs_op::paste(&moved, &dest, false).is_err());
    let folder = root.join("src");
    assert!(fs_op::paste(&folder, &folder, false).is_err());
    assert!(fs_op::paste(&folder, &folder.join("inner"), false).is_err());
}

#[test]
fn rename_refuses_to_clobber_and_delete_removes_whole_trees() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("项目 rename");
    fs::create_dir_all(root.join("src/inner")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("src/inner/deep.txt"), "deep").unwrap();
    fs::write(root.join("taken.rs"), "// taken\n").unwrap();

    // A rename inside the same folder keeps the contents.
    let renamed = fs_op::rename(&root.join("taken.rs"), "free.rs").unwrap();
    assert_eq!(renamed, root.join("free.rs"));
    assert_eq!(fs::read_to_string(&renamed).unwrap(), "// taken\n");
    assert!(!root.join("taken.rs").exists());

    // Names that are already taken, or that cannot exist, are refused.
    fs::write(root.join("other.rs"), "").unwrap();
    assert!(fs_op::rename(&root.join("free.rs"), "other.rs").is_err());
    assert!(fs_op::rename(&root.join("free.rs"), "a/b").is_err());
    assert!(fs_op::rename(&root.join("free.rs"), "..").is_err());
    assert!(root.join("free.rs").is_file());

    // A no-op rename is allowed rather than treated as a collision.
    assert_eq!(
        fs_op::rename(&root.join("free.rs"), "free.rs").unwrap(),
        root.join("free.rs")
    );

    // Folders rename with their subtree.
    let moved = fs_op::rename(&root.join("src"), "source").unwrap();
    assert_eq!(
        fs::read_to_string(moved.join("inner/deep.txt")).unwrap(),
        "deep"
    );
    assert!(!root.join("src").exists());

    // Delete takes a whole tree, and a missing path is an error rather than a
    // silent success.
    fs_op::delete(&moved).unwrap();
    assert!(!moved.exists());
    assert!(fs_op::delete(&moved).is_err());

    #[cfg(unix)]
    {
        // A case-only rename asks for a name that already "exists" on a
        // case-insensitive volume, but it is the entry being renamed.
        fs::write(root.join("case.rs"), "x").unwrap();
        let upper = fs_op::rename(&root.join("case.rs"), "Case.rs").unwrap();
        assert_eq!(upper.file_name().unwrap().to_string_lossy(), "Case.rs");
    }
}

#[test]
fn settings_round_trip_defaults_and_clamping() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("config/settings.json");

    // A fresh install has no file, and that is not an error.
    let defaults = folio::settings::load(&file).unwrap();
    assert_eq!(defaults, Settings::default());
    assert_eq!(defaults.tab_size, 4);
    assert!(!defaults.hard_tabs);
    assert!(defaults.sidebar);
    assert!(defaults.activity_bar);
    assert!(defaults.ignored.contains(&"node_modules".to_string()));

    let mut edited = defaults.clone();
    edited.font_family = Some("Inter".into());
    edited.code_font_family = Some("Geist Mono".into());
    edited.font_size = 15.;
    edited.tab_size = 2;
    edited.hard_tabs = true;
    edited.sidebar = false;
    edited.ignored = vec!["dist".into(), "vendor".into()];
    folio::settings::save(&file, &edited).unwrap();
    assert_eq!(folio::settings::load(&file).unwrap(), edited);

    // A hand-edited file that asks for something unusable is pulled back into
    // range rather than rejected.
    fs::write(
        &file,
        r#"{"font_size": 400.0, "code_font_size": 1.0, "tab_size": 0,
            "font_family": "   ", "ignored": ["  dist  ", "", "  "]}"#,
    )
    .unwrap();
    let clamped = folio::settings::load(&file).unwrap();
    assert_eq!(clamped.font_size, folio::settings::FONT_SIZE.1);
    assert_eq!(clamped.code_font_size, folio::settings::CODE_FONT_SIZE.0);
    assert_eq!(clamped.tab_size, folio::settings::TAB_SIZE.0);
    assert_eq!(clamped.font_family, None);
    assert_eq!(clamped.ignored, vec!["dist".to_string()]);
    // Keys the file does not mention keep their defaults, so a file written by
    // an older version still loads.
    assert!(clamped.sidebar);

    // A corrupt file is reported and left alone rather than silently replaced.
    fs::write(&file, "not json at all").unwrap();
    assert!(folio::settings::load(&file).is_err());
    assert_eq!(fs::read_to_string(&file).unwrap(), "not json at all");
}

#[test]
fn the_tree_honours_a_custom_ignore_list() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for name in ["src", "node_modules", "dist", "notes"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.join("dist/bundle.js"), "// built\n").unwrap();
    fs::write(root.join("notes/todo.md"), "todo\n").unwrap();

    let names = |entries: Vec<tree::Entry>| {
        entries
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>()
    };

    // The default list hides node_modules and dist, but not notes.
    assert_eq!(
        names(tree::children(root, &ignored()).unwrap()),
        vec!["notes", "src"]
    );

    // Narrowing it to one name brings the others back.
    assert_eq!(
        names(tree::children(root, &["notes".to_string()]).unwrap()),
        vec!["dist", "node_modules", "src"]
    );

    // The index follows the same list.
    assert_eq!(
        tree::index(root, &["notes".to_string()]).unwrap(),
        vec![root.join("dist/bundle.js"), root.join("src/main.rs")]
    );
}

#[test]
fn a_file_diff_reports_edits_and_untracked_files() {
    use folio::diff::{self, Change};
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
    let file = root.join("main.rs");
    fs::write(&file, "let one = 1;\nlet two = 2;\n").unwrap();
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

    // A tracked file with no edits has nothing to show, and is not untracked.
    let clean = diff::for_file(root, &file).unwrap();
    assert!(clean.is_empty());
    assert!(!clean.untracked);

    // An edit shows as the line that went and the line that arrived.
    fs::write(&file, "let one = 1;\nlet two = 22;\n").unwrap();
    let changed = diff::for_file(root, &file).unwrap();
    assert!(!changed.untracked);
    assert!(
        changed
            .lines
            .iter()
            .any(|line| { line.change == Change::Removed && line.text == "let two = 2;" })
    );
    assert!(
        changed
            .lines
            .iter()
            .any(|line| { line.change == Change::Added && line.text == "let two = 22;" })
    );

    // A file git has never seen is shown as all new, which is the only case
    // the diff reads the file itself.
    let fresh = root.join("未跟踪.rs");
    fs::write(&fresh, "// new\n").unwrap();
    let untracked = diff::for_file(root, &fresh).unwrap();
    assert!(untracked.untracked);
    assert_eq!(untracked.lines.len(), 1);
    assert_eq!(untracked.lines[0].change, Change::Added);
    assert_eq!(untracked.lines[0].new, Some(1));

    // A path outside the project has nothing to be compared against.
    let outside = diff::for_file(root, std::path::Path::new("/etc/hosts")).unwrap();
    assert!(outside.is_empty());
    assert!(!outside.untracked);
}

/// Blame is read against the buffer rather than the file on disk: they differ
/// as soon as anything is typed, and a line number that means the buffer has to
/// be answered against the buffer.
#[test]
fn blame_reports_the_commit_that_wrote_each_line() {
    use folio::blame;
    use std::process::Command;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q"]);
    let file = root.join("main.rs");
    fs::write(&file, "let one = 1;\nlet two = 2;\n").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Folio Test",
        "-c",
        "user.email=test@localhost",
        "commit",
        "-qm",
        "fixture",
    ]);
    fs::write(&file, "let one = 1;\nlet two = 22;\n").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Ada Lovelace",
        "-c",
        "user.email=ada@example.com",
        "commit",
        "-qm",
        "second pass",
    ]);

    let text = fs::read_to_string(&file).unwrap();
    let read = blame::run(&file, &text).unwrap();
    assert_eq!(read.lines.len(), 2);
    assert_eq!(read.lines[0].summary, "fixture");
    assert_eq!(read.lines[0].author, "Folio Test");
    assert_eq!(read.lines[1].summary, "second pass");
    assert_eq!(read.lines[1].author, "Ada Lovelace");
    // Both commits are made in the same second, so this is the field having
    // been read at all rather than an ordering.
    assert!(read.lines[0].time > 0);
    assert!(read.lines[1].time >= read.lines[0].time);

    // A buffer with a line that was never committed: blame is looked up by
    // line, so the extra line is the third one and it belongs to no commit.
    let written = "let one = 1;\nlet two = 22;\nlet three = 3;\n";
    let read = blame::run(&file, written).unwrap();
    assert_eq!(read.lines.len(), 3);
    assert!(read.lines[2].uncommitted());
    assert_eq!(read.lines[1].summary, "second pass");

    // A file git has never seen has no history to read.
    let fresh = root.join("未跟踪.rs");
    fs::write(&fresh, "// new\n").unwrap();
    assert!(blame::run(&fresh, "// new\n").is_err());
}
