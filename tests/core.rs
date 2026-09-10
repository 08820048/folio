use folio::{buffer, fs_op, recent, search, tree, workspace::Workspace};
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
    assert_eq!(copy.file_name().unwrap().to_string_lossy(), "笔记 副本.md");
    assert_eq!(fs::read_to_string(&copy).unwrap(), "hello");
    let second = fs_op::duplicate(&file).unwrap();
    assert_eq!(
        second.file_name().unwrap().to_string_lossy(),
        "笔记 副本 2.md"
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
