use super::*;

#[test]
fn search_filter_tokens_include_and_exclude() {
    let root = Path::new("/repo");
    let path = Path::new("/repo/src/app.rs");
    assert!(search_filter_matches(path, Some(root), ""));
    assert!(search_filter_matches(path, Some(root), "src"));
    assert!(search_filter_matches(path, Some(root), "APP"));
    assert!(!search_filter_matches(path, Some(root), "tests"));
    assert!(search_filter_matches(path, Some(root), "src tests"));
    assert!(search_filter_matches(path, Some(root), "src !test"));
    assert!(!search_filter_matches(path, Some(root), "src !app"));
    // Only an exclusion means "everything except this".
    assert!(search_filter_matches(path, Some(root), "!test"));
    assert!(search_filter_matches(path, None, "repo/src"));
}
use core::prelude::v1::test;

#[gpui::test]
fn multiple_projects_keep_buffers_and_guard_all_unsaved_changes(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let a = root.join("A");
    let b = root.join("B");
    for dir in [&a, &b] {
        std::fs::create_dir(dir).unwrap();
        std::fs::write(dir.join("main.rs"), "// original\n").unwrap();
    }
    std::fs::create_dir(a.join("src")).unwrap();
    std::fs::write(a.join("src/lib.rs"), "// child\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            // Never read or write the real settings file from a test.
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(a.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(a.join("main.rs"), window, cx)
    });
    cx.run_until_parked();
    let original_editor = view.update_in(cx, |app, window, cx| {
        let editor = app.project.documents[&a.join("main.rs")].editor.clone();
        editor
            .read(cx)
            .base_state()
            .clone()
            .update(cx, |base, cx| base.replace_all("// edit A\n", window, cx));
        app.toggle_directory(a.join("src"), cx);
        // Switch before the child directory read finishes.
        app.request(Next::Open(b.clone()), window, cx);
        editor
    });
    cx.run_until_parked();
    assert!(
        !cx.has_pending_prompt(),
        "adding a project must preserve dirty buffers without prompting"
    );
    view.update_in(cx, |app, window, cx| {
        assert_eq!(app.project_order, vec![a.clone(), b.clone()]);
        assert_eq!(app.parked[0].directories[&a.join("src")].len(), 1);
        app.open_file(b.join("main.rs"), window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.project.documents[&b.join("main.rs")]
            .editor
            .read(cx)
            .base_state()
            .clone()
            .update(cx, |base, cx| base.replace_all("// edit B\n", window, cx));
        app.select_project(&a, window, cx);
        assert_eq!(
            app.project.documents[&a.join("main.rs")].editor,
            original_editor
        );
        assert!(app.project.expanded.contains(&a.join("src")));
        assert!(app.project.documents[&a.join("main.rs")].dirty);
        app.request(Next::Open(a.join(".")), window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        assert_eq!(
            app.project_order.len(),
            2,
            "canonical paths must not create duplicate projects"
        );
        app.request(Next::Quit, window, cx);
    });
    assert!(cx.has_pending_prompt());
    assert!(cx.pending_prompt().unwrap().0.contains('2'));
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    std::fs::write(b.join("main.rs"), "// external edit\n").unwrap();
    view.update_in(cx, |app, window, cx| {
        app.save_documents(Some(Next::Quit), window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(a.join("main.rs")).unwrap(),
        "// edit A\n"
    );
    assert_eq!(
        std::fs::read_to_string(b.join("main.rs")).unwrap(),
        "// external edit\n"
    );
    view.update_in(cx, |app, window, cx| {
        assert!(app.parked[0].documents[&b.join("main.rs")].dirty);
        assert!(app.notice.is_some(), "a failed save must prevent quitting");
        app.request(Next::Close, window, cx);
        assert_eq!(app.project.workspace.as_ref().unwrap().root, b);
        assert_eq!(app.project_order, vec![b.clone()]);
        app.request(Next::Close, window, cx);
    });
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(app.project.documents[&b.join("main.rs")].dirty)
    });
}

#[gpui::test]
fn system_appearance_updates_chrome_and_editor(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let window = cx.add_window(|window, cx| {
        let mut app = Folio::new(window, cx);
        app.recent_task = None;
        app.settings_file = std::env::temp_dir().join("folio-test-settings.json");
        app.window_file = std::env::temp_dir().join("folio-test-window.json");
        app.settings = Settings::default();
        app.applied_ignored = app.settings.ignored.clone();
        app
    });
    for appearance in [
        WindowAppearance::Dark,
        WindowAppearance::Light,
        WindowAppearance::Dark,
    ] {
        window
            .update(cx, |app, window, cx| {
                sync_appearance(appearance, &app.settings, Some(window), cx);
                let theme = cx.theme();
                assert_eq!(theme.is_dark(), appearance == WindowAppearance::Dark);
                assert_eq!(theme.title_bar, theme.background);
                assert_eq!(
                    theme.highlight_theme.style.editor_background,
                    Some(theme.background)
                );
                assert_eq!(
                    theme.highlight_theme.style.editor_foreground,
                    Some(theme.foreground)
                );
                assert_ne!(theme.foreground, theme.background);
            })
            .unwrap();
    }
    let options = TitleBar::window_options();
    assert!(options.app_owns_titlebar_drag);
    let titlebar = options.titlebar.unwrap();
    assert!(titlebar.appears_transparent);
    assert!(titlebar.title.is_none());
}

#[gpui::test]
fn asynchronous_navigation_preserves_latest_intent(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let dir = root.join("src");
    std::fs::create_dir(&dir).unwrap();
    let file = dir.join("main.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    let config = root.join("recent.json");
    cx.update(gpui_component::init);
    // Exercise the real entities and executor without native input rendering.
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            // Cancel startup loading before the test executor runs; use isolated settings.
            app.recent_task = None;
            app.recent_file = config.clone();
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, _, cx| {
        app.project.workspace = Some(Workspace::open(&root).unwrap());
        let ignored = app.settings.ignored.clone();
        app.project
            .directories
            .insert(root.clone(), tree::children(&root, &ignored).unwrap());
        app.toggle_directory(dir.clone(), cx);
        app.toggle_directory(dir.clone(), cx);
        app.refresh_recent(RecentAction::Open(root.clone()), cx);
        app.refresh_recent(RecentAction::Open(dir.clone()), cx);
        app.refresh_recent(RecentAction::Remove(root.clone()), cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        assert!(
            !app.project.expanded.contains(&dir),
            "late directory reads must not expand a collapsed row"
        );
        assert_eq!(app.project.directories[&dir].len(), 1);
        app.toggle_directory(root.clone(), cx);
        assert_eq!(app.project.rows.len(), 1);
        app.toggle_directory(dir.clone(), cx);
        assert_eq!(app.project.rows.len(), 2);
        app.toggle_directory(root.clone(), cx);
        assert!(
            app.project.rows.is_empty(),
            "collapsing the root hides all descendants"
        );
        assert!(
            app.project.expanded.contains(&dir),
            "child expansion state is retained"
        );
        app.toggle_directory(root.clone(), cx);
        assert_eq!(
            app.project.rows.len(),
            2,
            "expanding the root restores its descendants"
        );
        assert_eq!(app.recent.len(), 1);
        assert_eq!(app.recent[0].path, dir);
        assert_eq!(recent::load(&config).unwrap()[0].path, dir);
        app.open_file(file.clone(), window, cx);
        app.request(Next::Open(dir.clone()), window, cx);
        app.open_file(file.clone(), window, cx);
        assert!(app.project_loading);
        assert!(
            app.loading,
            "file navigation must not clear project loading"
        );
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        assert_eq!(app.project.workspace.as_ref().unwrap().root, dir);
        assert!(
            app.project.documents.is_empty(),
            "old file results must not enter the new workspace"
        );
        assert!(!app.loading);
        app.request(Next::Picker, window, cx);
        app.request(Next::Close, window, cx);
        assert!(app.prompting);
        assert!(
            app.project.workspace.is_some(),
            "project actions must wait for the picker"
        );
    });
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| assert!(!app.prompting));
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        let doc = app.project.documents.get_mut(&file).unwrap();
        doc.editor
            .read(cx)
            .base_state()
            .clone()
            .update(cx, |base, cx| {
                base.replace_all("// first edit\n", window, cx);
            });
    });
    view.update_in(cx, |app, window, cx| {
        assert!(app.project.documents[&file].dirty);
        app.save_documents(Some(Next::Close), window, cx);
        // An edit already queued before read-only rendering must survive the save result.
        app.project.documents[&file]
            .editor
            .read(cx)
            .base_state()
            .clone()
            .update(cx, |base, cx| {
                base.replace_all("// newer edit\n", window, cx);
            });
    });
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "// first edit\n");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    view.update_in(cx, |app, _, cx| {
        assert!(app.project.documents[&file].dirty);
        assert_eq!(
            app.project.documents[&file]
                .editor
                .read(cx)
                .value()
                .as_ref(),
            "// newer edit\n"
        );
    });
    let picture = dir.join("preview.png");
    image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]))
        .save(&picture)
        .unwrap();
    view.update_in(cx, |app, window, cx| {
        app.open_file(picture.clone(), window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        assert_eq!(app.project.active(), Some(&picture));
        assert!(app.project.image.is_some());
        assert!(!app.project.documents.contains_key(&picture));
        assert!(app.project.documents[&file].dirty);
        app.save_documents(None, window, cx);
    });
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "// first edit\n");
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx);
        assert_eq!(app.project.active(), Some(&file));
        assert!(app.project.documents[&file].dirty);
        assert_eq!(
            app.project.documents[&file]
                .editor
                .read(cx)
                .value()
                .as_ref(),
            "// newer edit\n"
        );
    });
}

#[gpui::test]
fn a_new_project_is_an_empty_folder(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let created = root.join("notes");
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::NewProject, window, cx);
        assert!(app.prompting);
        assert!(app.project.workspace.is_none());
    });
    cx.simulate_new_path_selection(|_| Some(created.clone()));
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(created.is_dir());
        assert_eq!(
            app.project.workspace.as_ref().map(|w| w.root.clone()),
            Some(created.clone())
        );
        assert!(app.project.tabs().is_empty());
        assert!(!app.prompting);
    });

    let taken = root.join("taken");
    std::fs::create_dir(&taken).unwrap();
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Close, window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        assert!(app.project.workspace.is_none());
        app.request(Next::NewProject, window, cx);
    });
    cx.simulate_new_path_selection(|_| Some(taken.clone()));
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(app.project.workspace.is_none());
        assert!(
            app.notice.as_ref().is_some_and(|n| !n.text.is_empty()),
            "an existing folder is not reused as a new project"
        );
    });
}

#[gpui::test]
fn clone_from_github_asks_for_a_folder_after_the_url(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::CloneFromGithub, window, cx);
        assert!(app.clone_open);
        assert!(app.project.workspace.is_none());
        app.clone_url
            .update(cx, |input, cx| input.set_value("not-a-repo", window, cx));
        app.confirm_clone_url(window, cx);
        assert!(app.clone_open);
        assert!(app.notice.as_ref().is_some_and(|n| !n.text.is_empty()));
        app.clone_url.update(cx, |input, cx| {
            input.set_value("08820048/folio", window, cx)
        });
        app.confirm_clone_url(window, cx);
        assert!(!app.clone_open);
        assert!(app.prompting);
    });
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(!app.prompting);
        assert!(app.project.workspace.is_none());
    });
}

#[gpui::test]
fn clone_from_github_opens_the_repo(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("source");
    std::fs::create_dir(&source).unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&source)
            .args(args)
            .output()
            .unwrap()
    };
    assert!(git(&["init", "-q"]).status.success());
    std::fs::write(source.join("readme"), "from remote\n").unwrap();
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
    let dest = root.join("folio");
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.start_clone(
            source.to_string_lossy().into_owned(),
            dest.clone(),
            window,
            cx,
        );
        assert!(app.cloning);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(!app.cloning);
        assert_eq!(
            app.project.workspace.as_ref().map(|w| w.root.clone()),
            Some(dest.clone())
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("readme")).unwrap(),
            "from remote\n"
        );
    });

    let taken = root.join("taken");
    std::fs::create_dir(&taken).unwrap();
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Close, window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        assert!(app.project.workspace.is_none());
        app.start_clone(
            source.to_string_lossy().into_owned(),
            taken.clone(),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(app.project.workspace.is_none());
        assert!(
            app.notice
                .as_ref()
                .is_some_and(|n| n.text.contains("already exists")),
            "an existing folder is not reused as a clone"
        );
    });
}

#[gpui::test]
fn project_search_reads_dirty_buffers_and_replace_splits_open_and_closed_files(
    cx: &mut TestAppContext,
) {
    // The scan is debounced, so a test has to move its clock past the delay.
    fn settle(cx: &VisualTestContext) {
        cx.executor().advance_clock(SEARCH_DEBOUNCE * 2);
        cx.run_until_parked();
    }

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let open = root.join("open.rs");
    let closed = root.join("closed.rs");
    std::fs::write(&open, "let value = 1;\nlet other = 2;\n").unwrap();
    std::fs::write(&closed, "let value = 3;\n").unwrap();
    std::fs::write(root.join("notes.md"), "value in prose\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            // Never read or write the real settings file from a test.
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(open.clone(), window, cx)
    });
    cx.run_until_parked();

    // Edit the buffer without saving: the search must see what is on screen,
    // not what is on disk, or every reported position would be wrong.
    view.update_in(cx, |app, window, cx| {
        app.project.documents[&open]
            .editor
            .read(cx)
            .base_state()
            .clone()
            .update(cx, |base, cx| {
                base.replace_all("let value = 1;\nlet added = 5;\n", window, cx)
            });
        app.show_search(true, window, cx);
        app.search_query
            .update(cx, |query, cx| query.set_value("added", window, cx));
        app.start_search(cx);
    });
    settle(cx);
    view.update_in(cx, |app, _, _| {
        assert!(!app.search.running);
        assert!(app.search.error.is_none());
        assert_eq!(app.total_hits(), 1, "the unsaved line must be searchable");
        assert_eq!(app.search.results[0].path, open);
        assert_eq!(app.search.results[0].hits[0].line, 1);
    });

    // Case sensitivity is a filter over the same code path.
    view.update_in(cx, |app, window, cx| {
        app.search_query
            .update(cx, |query, cx| query.set_value("Value", window, cx));
        app.search.options.case_sensitive = true;
        app.start_search(cx);
    });
    settle(cx);
    view.update_in(cx, |app, _, _| assert_eq!(app.total_hits(), 0));
    view.update_in(cx, |app, window, cx| {
        app.search.options.case_sensitive = false;
        app.search_query
            .update(cx, |query, cx| query.set_value("value", window, cx));
        app.start_search(cx);
    });
    settle(cx);
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.total_hits(), 3, "open.rs, closed.rs and notes.md");
        assert_eq!(app.search.results.len(), 3);
    });

    // The path filter is a second, cheaper narrowing over the same scan.
    view.update_in(cx, |app, window, cx| {
        app.search_filter
            .update(cx, |filter, cx| filter.set_value("closed", window, cx));
        app.start_search(cx);
    });
    settle(cx);
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.search.results.len(), 1);
        assert_eq!(app.search.results[0].path, closed);
    });
    view.update_in(cx, |app, window, cx| {
        app.search_filter
            .update(cx, |filter, cx| filter.set_value("!open", window, cx));
        app.start_search(cx);
    });
    settle(cx);
    view.update_in(cx, |app, _, _| {
        let paths = app
            .search
            .results
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&closed));
        assert!(paths.iter().any(|path| path.ends_with("notes.md")));
    });
    view.update_in(cx, |app, window, cx| {
        app.search_filter
            .update(cx, |filter, cx| filter.set_value("", window, cx));
        app.start_search(cx);
    });
    settle(cx);

    // Opening a hit leaves the panel up so the next match is one key away.
    view.update_in(cx, |app, window, cx| {
        let file = app
            .search
            .results
            .iter()
            .position(|file| file.path == open)
            .unwrap();
        let row = app
            .search
            .rows
            .iter()
            .position(
                |row| matches!(row, SearchRow::Hit { file: f, hit } if *f == file && *hit == 0),
            )
            .unwrap();
        app.search.selected = row;
        app.open_selected_hit(window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, cx| {
        assert!(app.goto.is_none(), "the pending jump must be consumed");
        assert_eq!(app.panel, Some(Panel::Search));
        let cursor = app.project.documents[&open]
            .editor
            .read(cx)
            .base_state()
            .read(cx)
            .cursor_position();
        assert_eq!((cursor.line, cursor.character), (0, 4));
    });

    // Replace all: the open buffer is edited in memory and left dirty while
    // the files without an editor are rewritten on disk.
    view.update_in(cx, |app, window, cx| {
        app.show_search(true, window, cx);
        app.search_query
            .update(cx, |query, cx| query.set_value("value", window, cx));
        app.replace_query
            .update(cx, |query, cx| query.set_value("const value", window, cx));
        app.start_search(cx);
    });
    settle(cx);
    view.update_in(cx, |app, window, cx| {
        assert_eq!(app.total_hits(), 3);
        app.replace_project(window, cx);
    });
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Replace All");
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(&closed).unwrap(),
        "let const value = 3;\n",
        "a closed file is rewritten in place"
    );
    assert_eq!(
        std::fs::read_to_string(&open).unwrap(),
        "let value = 1;\nlet other = 2;\n",
        "an open buffer must not be written behind the user's back"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("notes.md")).unwrap(),
        "const value in prose\n"
    );
    view.update_in(cx, |app, _, cx| {
        let document = &app.project.documents[&open];
        assert!(document.dirty);
        assert_eq!(
            document.editor.read(cx).value().as_ref(),
            "let const value = 1;\nlet added = 5;\n"
        );
    });
    settle(cx);
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.total_hits(), 3, "the search re-runs after a replace");
        assert_eq!(app.search.results.len(), 3);
    });
}

/// The tree context menu end to end: the row it splices in, the entry it
/// writes, and the clipboard it moves entries with. Reveal / Open in
/// Default App / Open in Terminal are left alone — they hand the path to
/// the OS, which during a test run would open Finder and a terminal.
/// The saved geometry: what round-trips, what an older file still loads as,
/// and what is too far out of range to restore.
#[gpui::test]
fn window_geometry_round_trips(_cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("config/window.json");

    // Nothing saved yet is not an error.
    assert!(WindowState::load(&file).main.is_none());

    WindowState {
        main: Some([10., 20., 1200., 800.]),
        settings: Some([30., 40., 900., 660.]),
    }
    .save(&file);
    let loaded = WindowState::load(&file);
    assert_eq!(loaded.main, Some([10., 20., 1200., 800.]));
    assert_eq!(loaded.settings, Some([30., 40., 900., 660.]));

    // The bare array older versions wrote still loads, as the main window.
    std::fs::write(&file, "[1.0, 2.0, 1000.0, 700.0]").unwrap();
    let legacy = WindowState::load(&file);
    assert_eq!(legacy.main, Some([1., 2., 1000., 700.]));
    assert!(legacy.settings.is_none());

    // A rectangle too small for the window, or not finite, is not restored.
    let minimum = size(px(640.), px(480.));
    assert!(restore_bounds([0., 0., 320., 200.], minimum).is_none());
    assert!(restore_bounds([0., 0., f32::NAN, 800.], minimum).is_none());
    assert!(restore_bounds([0., 0., 1200., 800.], minimum).is_some());

    // A file that is not JSON at all counts as nothing saved.
    std::fs::write(&file, "not json").unwrap();
    assert!(WindowState::load(&file).main.is_none());
}

/// Both windows are recorded when the app writes its geometry, and it goes
/// to the file the app was pointed at rather than the real one.
#[gpui::test]
fn both_windows_geometry_is_recorded(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("window.json");
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = file.clone();
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, _, _| {
        app.main_bounds = Bounds::new(point(px(10.), px(20.)), size(px(1000.), px(700.)));
        app.remember_settings_bounds(Bounds::new(
            point(px(30.), px(40.)),
            size(px(900.), px(660.)),
        ));
    });
    let state = WindowState::load(&file);
    assert_eq!(state.main, Some([10., 20., 1000., 700.]));
    assert_eq!(state.settings, Some([30., 40., 900., 660.]));
}

/// A dragged tab lands where the tab it was dropped on sits, and that
/// works the same in either direction — the leftward one-slot drag is the
/// case a pointer-following rule could not be relied on for.
#[gpui::test]
fn dragging_a_tab_reorders_the_strip(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let first = root.join("first.rs");
    let second = root.join("second.rs");
    let third = root.join("third.rs");
    for file in [&first, &second, &third] {
        std::fs::write(file, "// code\n").unwrap();
    }
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    for file in [&first, &second, &third] {
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();
    }

    // One slot to the right, onto the tab it should trade with.
    view.update_in(cx, |app, _, cx| app.move_tab_to(&first, 0, 1, cx));
    view.update_in(cx, |app, _, _| {
        assert_eq!(
            app.project.tabs().to_vec(),
            vec![second.clone(), first.clone(), third.clone()]
        )
    });

    // And one slot back to the left, which is the drag that used to do
    // nothing.
    view.update_in(cx, |app, _, cx| app.move_tab_to(&first, 0, 0, cx));
    view.update_in(cx, |app, _, _| {
        assert_eq!(
            app.project.tabs().to_vec(),
            vec![first.clone(), second.clone(), third.clone()]
        )
    });

    // Right across the strip, and back to the front.
    view.update_in(cx, |app, _, cx| app.move_tab_to(&first, 0, 2, cx));
    view.update_in(cx, |app, _, _| {
        assert_eq!(
            app.project.tabs().to_vec(),
            vec![second.clone(), third.clone(), first.clone()]
        )
    });
    view.update_in(cx, |app, _, cx| app.move_tab_to(&first, 0, 0, cx));
    view.update_in(cx, |app, _, _| {
        assert_eq!(
            app.project.tabs().to_vec(),
            vec![first.clone(), second.clone(), third.clone()]
        )
    });

    // Past the end lands at the end; dropping on itself changes nothing.
    view.update_in(cx, |app, _, cx| app.move_tab_to(&first, 0, 99, cx));
    view.update_in(cx, |app, _, _| {
        assert_eq!(
            app.project.tabs().to_vec(),
            vec![second.clone(), third.clone(), first.clone()]
        )
    });
    view.update_in(cx, |app, _, cx| app.move_tab_to(&first, 0, 2, cx));
    view.update_in(cx, |app, _, _| {
        assert_eq!(
            app.project.tabs().to_vec(),
            vec![second.clone(), third.clone(), first.clone()]
        )
    });

    // A pinned tab stays in front whatever the drop says, and an unpinned
    // one cannot be dropped ahead of it.
    view.update_in(cx, |app, _, cx| {
        app.project.pinned.insert(second.clone());
        app.reorder_tabs();
        app.move_tab_to(&second, 0, 2, cx);
    });
    view.update_in(cx, |app, _, _| {
        assert_eq!(
            app.project.tabs().first(),
            Some(&second),
            "a pinned tab cannot be dragged out of the front"
        );
    });
    view.update_in(cx, |app, _, cx| app.move_tab_to(&first, 0, 0, cx));
    view.update_in(cx, |app, _, _| {
        assert_eq!(
            app.project.tabs().to_vec(),
            vec![second.clone(), first.clone(), third.clone()],
            "an unpinned tab cannot be dropped in front of a pinned one"
        );
    });

    // The caret draws between tabs while a drag is in flight.
    view.update_in(cx, |app, window, cx| {
        app.tab_drop = Some((0, 1));
        let _ = app.render(window, cx);
    });

    // A path that is not in the strip changes nothing.
    view.update_in(cx, |app, _, cx| {
        app.move_tab_to(&root.join("ghost.rs"), 0, 0, cx)
    });
    view.update_in(cx, |app, _, _| {
        assert_eq!(
            app.project.tabs().to_vec(),
            vec![second.clone(), first.clone(), third.clone()]
        )
    });
}

/// The tab strip's menu: the closes act on the tab that was clicked, and
/// the bulk ones leave pinned tabs alone.
#[gpui::test]
fn the_tab_menu_closes_pins_and_locks(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let first = root.join("first.rs");
    let second = root.join("second.rs");
    let third = root.join("third.rs");
    for file in [&first, &second, &third] {
        std::fs::write(file, "// code\n").unwrap();
    }
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    for file in [&first, &second, &third] {
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();
    }

    // The strip offers its own table, and closing leftwards off the first
    // tab is one of the things that has nothing to do.
    view.update_in(cx, |app, window, cx| {
        app.open_menu(
            MenuTarget::Tab {
                path: first.clone(),
                index: 0,
            },
            point(px(60.), px(40.)),
            window,
            cx,
        );
        let target = app.menu.as_ref().unwrap().target.clone();
        assert_eq!(visible_menu_items(&target).len(), TAB_MENU.len());
        assert!(!app.menu_item_enabled(MenuItem::CloseLeft, &target, cx));
        assert!(app.menu_item_enabled(MenuItem::CloseRight, &target, cx));
        // The strip draws under it.
        let _ = app.render(window, cx);
        app.run_menu_item(MenuItem::CloseRight, window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.project.tabs().to_vec(), vec![first.clone()]);
    });

    // Close Others keeps the tab the menu was opened on, and takes the
    // rest — including the one that was showing.
    view.update_in(cx, |app, window, cx| {
        app.open_file(second.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_menu(
            MenuTarget::Tab {
                path: first.clone(),
                index: 0,
            },
            point(px(60.), px(40.)),
            window,
            cx,
        );
        app.run_menu_item(MenuItem::CloseOthers, window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.project.tabs().to_vec(), vec![first.clone()]);
        assert_eq!(app.project.active(), Some(&first));
    });

    // Pinning moves the tab to the front and takes it out of the bulk
    // closes' reach.
    view.update_in(cx, |app, window, cx| {
        app.open_file(third.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_menu(
            MenuTarget::Tab {
                path: third.clone(),
                index: 1,
            },
            point(px(60.), px(40.)),
            window,
            cx,
        );
        app.run_menu_item(MenuItem::PinTab, window, cx);
        assert!(app.is_pinned(&third));
        assert_eq!(app.project.tabs().first(), Some(&third));
        // And the entry says what it will do next time.
        assert_eq!(
            app.menu_label(MenuItem::PinTab, &third, "Pin Tab").as_ref(),
            "Unpin Tab"
        );
    });
    view.update_in(cx, |app, window, cx| {
        app.open_menu(
            MenuTarget::Tab {
                path: first.clone(),
                index: 1,
            },
            point(px(60.), px(40.)),
            window,
            cx,
        );
        let target = app.menu.as_ref().unwrap().target.clone();
        assert!(!app.menu_item_enabled(MenuItem::CloseOthers, &target, cx));
        app.run_menu_item(MenuItem::CloseOthers, window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert_eq!(
            app.project.tabs().to_vec(),
            vec![third.clone(), first.clone()]
        );
    });

    // Locking a tab is view state: it flips the label and refuses edits,
    // and nothing about it reaches the file.
    view.update_in(cx, |app, window, cx| {
        app.open_menu(
            MenuTarget::Tab {
                path: first.clone(),
                index: 1,
            },
            point(px(60.), px(40.)),
            window,
            cx,
        );
        app.run_menu_item(MenuItem::ToggleReadOnly, window, cx);
        assert!(app.is_read_only(&first));
        assert_eq!(
            app.menu_label(MenuItem::ToggleReadOnly, &first, "Make Tab Read-Only")
                .as_ref(),
            "Make Tab Writable"
        );
        let _ = app.render(window, cx);
    });

    // Copying a relative path puts it on the clipboard, without the root.
    view.update_in(cx, |app, window, cx| {
        app.open_menu(
            MenuTarget::Tab {
                path: first.clone(),
                index: 1,
            },
            point(px(60.), px(40.)),
            window,
            cx,
        );
        app.run_menu_item(MenuItem::CopyRelativePath, window, cx);
    });
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("first.rs")
    );

    // Revealing opens the folders above the file — its row does not exist
    // until they are — and puts the tree's cursor on it.
    let nested = root.join("nested");
    let deep = nested.join("deep.rs");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(&deep, "// deep\n").unwrap();
    view.update_in(cx, |app, window, cx| {
        app.open_file(deep.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        assert!(!app.project.expanded.contains(&nested));
        app.open_menu(
            MenuTarget::Tab {
                path: deep.clone(),
                index: 2,
            },
            point(px(60.), px(40.)),
            window,
            cx,
        );
        app.run_menu_item(MenuItem::RevealInTree, window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(app.project.expanded.contains(&nested));
        let row = &app.project.rows[app.project.selected_row];
        assert_eq!(row.entry.path, deep, "the cursor is on the revealed file");
    });
}

/// Tabs: opening adds them, closing one releases its buffer and leaves a
/// neighbour showing, and a dirty one is asked about first.
#[gpui::test]
fn tabs_open_close_and_guard_unsaved_changes(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let first = root.join("first.rs");
    let second = root.join("second.rs");
    std::fs::write(&first, "// one\n").unwrap();
    std::fs::write(&second, "// two\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |app, window, cx| {
        app.open_file(first.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.project.tabs().to_vec(), vec![first.clone()]);
        assert_eq!(app.project.active(), Some(&first));
    });

    view.update_in(cx, |app, window, cx| {
        app.open_file(second.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        assert_eq!(
            app.project.tabs().to_vec(),
            vec![first.clone(), second.clone()]
        );
        assert_eq!(app.project.active(), Some(&second));
        // The strip draws; the harness never draws on its own.
        let _ = app.render(window, cx);
    });

    // Opening a file that already has a tab focuses it rather than adding
    // a second one.
    view.update_in(cx, |app, window, cx| {
        app.open_file(first.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.project.tabs().len(), 2);
        assert_eq!(app.project.active(), Some(&first));
    });

    // Closing the active tab releases its buffer and shows the neighbour.
    view.update_in(cx, |app, window, cx| {
        app.close_tabs(vec![first.clone()], window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.project.tabs().to_vec(), vec![second.clone()]);
        assert!(!app.project.documents.contains_key(&first));
        assert_eq!(app.project.active(), Some(&second));
    });

    // An untouched tab closes without a word.
    view.update_in(cx, |app, window, cx| {
        app.request(Next::CloseTabs(vec![second.clone()]), window, cx)
    });
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    view.update_in(cx, |app, _, _| {
        assert!(app.project.tabs().is_empty());
        assert!(app.project.active().is_none());
        assert!(app.project.documents.is_empty());
    });

    // A dirty tab is asked about, and cancelling leaves it alone.
    view.update_in(cx, |app, window, cx| {
        app.open_file(second.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.project.documents.get_mut(&second).unwrap().dirty = true;
        app.request(Next::CloseTabs(vec![second.clone()]), window, cx);
        assert!(app.prompting);
    });
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.project.tabs().to_vec(), vec![second.clone()]);
        assert!(app.project.documents.contains_key(&second));
    });

    // Discarding closes it, and putting the tab back works the same way.
    view.update_in(cx, |app, window, cx| {
        app.request(Next::CloseTabs(vec![second.clone()]), window, cx)
    });
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Don't Save");
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(app.project.tabs().is_empty());
        assert!(app.project.documents.is_empty());
    });
}

/// A closed tab is remembered and can be put back, unless its file has
/// gone from disk in the meantime.
#[gpui::test]
fn a_closed_tab_can_be_reopened(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let first = root.join("first.rs");
    let second = root.join("second.rs");
    std::fs::write(&first, "// one\n").unwrap();
    std::fs::write(&second, "// two\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    for file in [&first, &second] {
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();
    }

    view.update_in(cx, |app, window, cx| {
        app.close_tabs(vec![first.clone()], window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.project.tabs().to_vec(), vec![second.clone()]);
    });

    // The reopened file goes back into the focused strip and becomes the
    // one on screen.
    view.update_in(cx, |app, window, cx| app.reopen_closed_tab(window, cx));
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert_eq!(
            app.project.tabs().to_vec(),
            vec![second.clone(), first.clone()]
        );
        assert_eq!(app.project.active(), Some(&first));
    });

    // A file deleted while it was closed is skipped rather than opening
    // an editor over nothing.
    view.update_in(cx, |app, window, cx| {
        app.close_tabs(vec![first.clone()], window, cx)
    });
    cx.run_until_parked();
    std::fs::remove_file(&first).unwrap();
    view.update_in(cx, |app, window, cx| app.reopen_closed_tab(window, cx));
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.project.tabs().to_vec(), vec![second.clone()]);
        assert_eq!(app.project.active(), Some(&second));
    });
}

/// Typing in the tree selects the next prefix match, starts at the current
/// row, and leaves the selection alone when nothing matches.
#[gpui::test]
fn tree_typeahead_selects_by_prefix(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app
        })
    });
    view.update_in(cx, |app, _, cx| {
        let row = |name: &str| Row {
            entry: tree::Entry {
                path: PathBuf::from("/repo").join(name),
                name: name.to_string(),
                kind: EntryKind::File,
            },
            depth: 0,
        };
        app.project.rows = vec![row("alpha.rs"), row("beta.rs"), row("gamma.rs")];
        app.project.selected_row = 0;

        app.project.tree_query = "gam".into();
        app.tree_typeahead(cx);
        assert_eq!(app.project.selected_row, 2);

        app.project.tree_query = "be".into();
        app.project.selected_row = 0;
        app.tree_typeahead(cx);
        assert_eq!(app.project.selected_row, 1);

        app.project.tree_query = "zzz".into();
        app.project.selected_row = 1;
        app.tree_typeahead(cx);
        assert_eq!(app.project.selected_row, 1, "no match leaves the row alone");
    });
}

/// A binary file opens in the read-only page viewer, with hex offsets and no
/// Document behind it.
#[gpui::test]
fn a_binary_file_opens_in_the_page_viewer(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let blob = root.join("blob.bin");
    std::fs::write(&blob, [0x41, 0x00, 0x42, 0xff].repeat(5_000)).unwrap();

    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(blob.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        let large = app.project.large_file.as_ref().expect("binary page viewer");
        assert_eq!(large.path, blob);
        assert!(large.binary);
        assert_eq!(large.pages, 2);
        assert_eq!(large.page, 0);
        assert!(large.text.starts_with("00000000"));
        assert!(
            app.project.documents.is_empty(),
            "no editor behind the page viewer"
        );
        let _ = app.render(window, cx);
    });

    view.update_in(cx, |app, _, cx| app.load_large_page(1, cx));
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        let large = app.project.large_file.as_ref().unwrap();
        assert_eq!(large.page, 1);
        assert!(large.text.starts_with("00004000"), "{}", large.text);
    });
}

/// Image zoom is bounded and Fit returns to the fitted view.
#[gpui::test]
fn image_zoom_clamps_and_fit_resets(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app
        })
    });
    view.update_in(cx, |app, _, cx| {
        assert!((app.project.image_zoom - 1.0).abs() < f32::EPSILON);
        app.zoom_image(-10.0, cx);
        assert!((app.project.image_zoom - 0.25).abs() < f32::EPSILON);
        app.zoom_image(100.0, cx);
        assert!((app.project.image_zoom - 8.0).abs() < f32::EPSILON);
        app.reset_image_view(cx);
        assert!((app.project.image_zoom - 1.0).abs() < f32::EPSILON);
    });
}

/// A relative Markdown link opens the target in Folio, with a fragment
/// stripped before the path is resolved.
#[gpui::test]
fn markdown_links_open_notes_in_the_app(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let docs = root.join("docs");
    std::fs::create_dir(&docs).unwrap();
    let first = docs.join("first.md");
    let second = docs.join("second.md");
    std::fs::write(&first, "# First\n\n[Next](second.md#part)\n").unwrap();
    std::fs::write(&second, "# Second\n\n## Part\n").unwrap();

    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(first.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_markdown_link(Some(&docs), "second.md#part", window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.project.active(), Some(&second));
        assert!(app.project.documents.contains_key(&second));
    });
}

/// An empty quick open is the recent files, and a typed one puts name
/// quality ahead of a mere substring or a deep path.
#[gpui::test]
fn quick_open_prefers_recent_and_ranks_names(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("deep/a/b")).unwrap();
    let recent = root.join("domain.rs");
    let shallow = root.join("src/main.rs");
    let prefixed = root.join("main_test.rs");
    let deep = root.join("deep/a/b/main.rs");
    let readme = root.join("README.md");
    std::fs::write(&recent, "// recent\n").unwrap();
    std::fs::write(&shallow, "fn main() {}\n").unwrap();
    std::fs::write(&prefixed, "// test\n").unwrap();
    std::fs::write(&deep, "fn main() {}\n").unwrap();
    std::fs::write(&readme, "# readme\n").unwrap();

    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(recent.clone(), window, cx)
    });
    cx.run_until_parked();

    // No query: the file just opened comes first.
    view.update_in(cx, |app, window, cx| {
        app.show_quick_open(false, window, cx);
    });
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.matches.first(), Some(&recent));
    });

    // A typed query sorts a basename prefix over a substring and a deep path,
    // and a recent boost does not lift a weak name over a prefix.
    view.update_in(cx, |app, window, cx| {
        app.query
            .update(cx, |query, cx| query.set_value("main", window, cx));
        app.filter(cx);
    });
    view.update_in(cx, |app, _, _| {
        let names = app
            .matches
            .iter()
            .map(|path| path.strip_prefix(&root).unwrap().to_path_buf())
            .collect::<Vec<_>>();
        assert_eq!(
            names.first().map(|path| path.as_path()),
            Some(std::path::Path::new("src/main.rs")),
            "{names:?}"
        );
        let shallow_at = names
            .iter()
            .position(|path| path == std::path::Path::new("src/main.rs"))
            .unwrap();
        let substring_at = names
            .iter()
            .position(|path| path == std::path::Path::new("domain.rs"))
            .unwrap();
        let deep_at = names
            .iter()
            .position(|path| path == std::path::Path::new("deep/a/b/main.rs"))
            .unwrap();
        assert!(shallow_at < substring_at, "{names:?}");
        assert!(shallow_at < deep_at, "{names:?}");
        assert!(!names.contains(&readme), "{names:?}");
        assert!(
            names.contains(&prefixed.strip_prefix(&root).unwrap().to_path_buf()),
            "{names:?}"
        );
    });
}

/// A file-level create/remove updates the quick-open index without
/// walking the project again; a file the `.gitignore` excludes is skipped.
#[gpui::test]
fn index_delta_adds_and_removes_files(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let first = root.join("first.rs");
    let second = root.join("second.rs");
    let third = root.join("third.rs");
    std::fs::write(&first, "// one\n").unwrap();
    std::fs::write(&second, "// two\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(app.project.files.contains(&first));
        assert!(app.project.files.contains(&second));
    });

    // Remove one file and add another in the same delta.
    std::fs::remove_file(&first).unwrap();
    std::fs::write(&third, "// three\n").unwrap();
    view.update_in(cx, |app, _, cx| {
        let project_id = app.project.id;
        app.apply_index_delta(
            project_id,
            root.clone(),
            vec![third.clone()],
            vec![first.clone()],
            cx,
        );
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(!app.project.files.contains(&first));
        assert!(app.project.files.contains(&second));
        assert!(app.project.files.contains(&third));
        assert_eq!(
            app.project.files.len(),
            app.project.file_names_lower.len(),
            "the quick-open names stay aligned with the paths"
        );
    });

    // A new file named by `.gitignore` is not indexed.
    std::fs::write(root.join(".gitignore"), "ignored.rs\n").unwrap();
    let ignored = root.join("ignored.rs");
    std::fs::write(&ignored, "// ignored\n").unwrap();
    view.update_in(cx, |app, _, cx| {
        let project_id = app.project.id;
        app.apply_index_delta(
            project_id,
            root.clone(),
            vec![ignored.clone()],
            Vec::new(),
            cx,
        );
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(!app.project.files.contains(&ignored));
    });

    // A removed directory takes everything under it.
    std::fs::create_dir(root.join("sub")).unwrap();
    let nested = root.join("sub/nested.rs");
    std::fs::write(&nested, "// nested\n").unwrap();
    view.update_in(cx, |app, _, cx| {
        let project_id = app.project.id;
        app.apply_index_delta(
            project_id,
            root.clone(),
            vec![nested.clone()],
            Vec::new(),
            cx,
        );
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(app.project.files.contains(&nested));
    });
    std::fs::remove_dir_all(root.join("sub")).unwrap();
    view.update_in(cx, |app, _, cx| {
        let project_id = app.project.id;
        app.apply_index_delta(
            project_id,
            root.clone(),
            Vec::new(),
            vec![root.join("sub")],
            cx,
        );
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(!app.project.files.contains(&nested));
        assert!(app.project.files.contains(&second));
    });
}

/// The arrows on the left of the strip walk the open files in order
/// and stop at the ends.
#[gpui::test]
fn tab_arrows_step_through_open_files(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let first = root.join("first.rs");
    let second = root.join("second.rs");
    let third = root.join("third.rs");
    std::fs::write(&first, "// one\n").unwrap();
    std::fs::write(&second, "// two\n").unwrap();
    std::fs::write(&third, "// three\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(first.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(second.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(third.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        assert_eq!(app.project.active(), Some(&third));
        assert!(app.tab_step_target(0, 1).is_none());
        app.step_tab(0, 1, window, cx);
        assert_eq!(app.project.active(), Some(&third), "the last tab stays put");
        app.step_tab(0, -1, window, cx);
        assert_eq!(app.project.active(), Some(&second));
        app.step_tab(0, -1, window, cx);
        assert_eq!(app.project.active(), Some(&first));
        app.step_tab(0, -1, window, cx);
        assert_eq!(
            app.project.active(),
            Some(&first),
            "the first tab stays put"
        );
        app.step_tab(0, 1, window, cx);
        assert_eq!(app.project.active(), Some(&second));
        let _ = app.render(window, cx);
    });
}

#[gpui::test]
fn a_split_puts_the_other_tab_on_the_right(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let first = root.join("first.rs");
    let second = root.join("second.rs");
    std::fs::write(&first, "// one\n").unwrap();
    std::fs::write(&second, "// two\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(first.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(second.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.split_editor(window, cx);
        assert_eq!(app.project.panes.len(), 2);
        assert_eq!(app.project.panes[0].tabs, vec![second.clone()]);
        assert_eq!(app.project.panes[1].tabs, vec![first.clone()]);
        assert_eq!(app.project.focused, 0);
        assert_eq!(app.project.active(), Some(&second));
        assert_eq!(app.project.split_axis, SplitAxis::Horizontal);
        let _ = app.render(window, cx);
    });
}

#[gpui::test]
fn a_split_left_puts_the_other_tab_first(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let first = root.join("first.rs");
    let second = root.join("second.rs");
    std::fs::write(&first, "// one\n").unwrap();
    std::fs::write(&second, "// two\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(first.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(second.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.split_in(SplitDirection::Left, window, cx);
        assert_eq!(app.project.panes.len(), 2);
        assert_eq!(app.project.panes[0].tabs, vec![first.clone()]);
        assert_eq!(app.project.panes[1].tabs, vec![second.clone()]);
        assert_eq!(app.project.focused, 1);
        assert_eq!(app.project.split_axis, SplitAxis::Horizontal);
        app.split_in(SplitDirection::Down, window, cx);
        assert_eq!(app.project.split_axis, SplitAxis::Vertical);
        assert_eq!(app.project.panes[0].tabs, vec![second.clone()]);
        assert_eq!(app.project.panes[1].tabs, vec![first.clone()]);
        assert_eq!(app.project.focused, 0);
        let _ = app.render(window, cx);
    });
}

#[gpui::test]
fn a_tab_can_be_dragged_into_the_other_pane(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let first = root.join("first.rs");
    let second = root.join("second.rs");
    let third = root.join("third.rs");
    for file in [&first, &second, &third] {
        std::fs::write(file, "// code\n").unwrap();
    }
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    for file in [&first, &second, &third] {
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();
    }
    view.update_in(cx, |app, window, cx| {
        app.split_editor(window, cx);
        // [third] | [second] after taking the neighbor of the active tab;
        // first stays with third if the neighbor was second... open order
        // first, second, third — active third, neighbor second →
        // left keeps third, right takes second. first is still on the left.
        assert_eq!(app.project.panes.len(), 2);
        assert!(app.project.panes[0].tabs.contains(&third));
        assert!(app.project.panes[1].tabs.contains(&second));
        app.move_tab_to(&third, 1, 0, cx);
        assert_eq!(app.project.panes.len(), 2);
        assert!(
            app.project.panes[1].tabs.contains(&third),
            "the tab crossed to the other pane"
        );
        assert!(
            !app.project.panes[0].tabs.contains(&third),
            "it left the pane it came from"
        );
        assert_eq!(app.project.focused, 1);
        assert_eq!(app.project.panes[1].active.as_ref(), Some(&third));
        let other = app.project.panes[1]
            .tabs
            .iter()
            .find(|tab| tab.as_path() != third.as_path())
            .cloned()
            .expect("the destination still has its own tab");
        app.move_tab_to(&other, 0, 0, cx);
        assert_eq!(app.project.panes.len(), 2);
        assert!(app.project.panes[0].tabs.contains(&other));
        assert!(app.project.panes[1].tabs.contains(&third));
        assert!(!app.project.panes[1].tabs.contains(&other));
    });
}

#[gpui::test]
fn opening_a_file_already_in_the_other_pane_moves_focus(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let first = root.join("first.rs");
    let second = root.join("second.rs");
    let third = root.join("third.rs");
    std::fs::write(&first, "// one\n").unwrap();
    std::fs::write(&second, "// two\n").unwrap();
    std::fs::write(&third, "// three\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(first.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(second.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| app.split_editor(window, cx));
    view.update_in(cx, |app, window, cx| {
        app.open_file(first.clone(), window, cx);
        assert_eq!(app.project.focused, 1, "the other column already has it");
        assert_eq!(app.project.active(), Some(&first));
        assert_eq!(app.project.panes[0].tabs, vec![second.clone()]);
        assert_eq!(app.project.panes[1].tabs, vec![first.clone()]);
        app.open_file(third.clone(), window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        assert_eq!(app.project.focused, 1);
        assert!(app.project.panes[1].tabs.contains(&third));
        assert!(!app.project.panes[0].tabs.contains(&third));
        app.close_tabs(vec![first.clone(), third.clone()], window, cx);
        assert_eq!(app.project.panes.len(), 1);
        assert_eq!(app.project.tabs().to_vec(), vec![second.clone()]);
    });
}

/// Left and right in the panel switch the mode tabs from the caret's
/// edges, and keep their ordinary meaning everywhere else.
#[gpui::test]
fn panel_arrows_switch_the_mode_tabs(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("main.rs"), "// original\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();

    // The query is focused and empty when the panel opens, so the caret
    // sits on both edges at once: right takes Files to Contents, and
    // left, which would leave the first tab, does not move.
    view.update_in(cx, |app, window, cx| {
        app.show_quick_open(false, window, cx);
        assert_eq!(app.panel, Some(Panel::Files));
        assert!(
            !app.panel_arrow("left", window, cx),
            "left at the first tab stays"
        );
        assert_eq!(app.panel, Some(Panel::Files));
        assert!(
            app.panel_arrow("right", window, cx),
            "right at the edge takes the tab to Contents"
        );
        assert_eq!(app.panel, Some(Panel::Search));
        assert!(
            !app.panel_arrow("right", window, cx),
            "right at the last tab stays"
        );
        assert!(
            app.panel_arrow("left", window, cx),
            "left at the edge takes the tab back to Files"
        );
        assert_eq!(app.panel, Some(Panel::Files));
    });

    // A caret inside the text keeps the key, and so does a selection.
    view.update_in(cx, |app, window, cx| {
        app.show_quick_open(false, window, cx);
        let query = app.query.clone();
        query.update(cx, |query, cx| {
            query.set_value("ab", window, cx);
            query
                .base_state()
                .update(cx, |base, cx| base.set_selected_range(1..1, cx));
        });
        assert!(
            !app.panel_arrow("right", window, cx),
            "a caret inside the text keeps right"
        );
        assert_eq!(app.panel, Some(Panel::Files));
        query.update(cx, |query, cx| query.select_all(window, cx));
        assert!(
            !app.panel_arrow("right", window, cx),
            "a selection keeps the key"
        );
        assert_eq!(app.panel, Some(Panel::Files));
        query.update(cx, |query, cx| {
            query
                .base_state()
                .update(cx, |base, cx| base.set_selected_range(2..2, cx));
        });
        assert!(
            app.panel_arrow("right", window, cx),
            "the caret at the end spends the key on the tab"
        );
        assert_eq!(app.panel, Some(Panel::Search));
    });

    // A focus outside the panel inputs keeps the key too: the editor
    // behind the overlay must not spend its arrows on the tabs.
    view.update_in(cx, |app, window, cx| {
        app.open_file(root.join("main.rs"), window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.show_search(false, window, cx);
        app.project.documents[&root.join("main.rs")]
            .editor
            .focus_handle(cx)
            .focus(window, cx);
        assert!(
            !app.panel_arrow("left", window, cx),
            "a focus outside the panel keeps left"
        );
        assert_eq!(app.panel, Some(Panel::Search));
        app.search_query
            .update(cx, |query, cx| query.focus(window, cx));
        assert!(
            app.panel_arrow("left", window, cx),
            "the query's empty text puts the caret on both edges again"
        );
        assert_eq!(app.panel, Some(Panel::Files));
    });
    cx.run_until_parked();
}

/// The terminal drawer: tabs come and go per project, the children run
/// while the drawer is hidden, the last close takes the drawer, and
/// closing the project takes everything.
#[gpui::test]
fn the_terminal_dock_follows_the_project(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("main.rs"), "// original\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();

    // Opening the drawer spawns the project's terminal and focuses it.
    view.update_in(cx, |app, window, cx| {
        app.toggle_terminal(window, cx);
        assert!(app.terminal_open);
        let tabs = app.terminals.get(&root).expect("a terminal was spawned");
        assert_eq!(tabs.views.len(), 1);
        assert_eq!(tabs.active, 0);
    });
    cx.run_until_parked();

    // Hiding the drawer keeps the children running.
    view.update_in(cx, |app, window, cx| {
        app.toggle_terminal(window, cx);
        assert!(!app.terminal_open);
        assert_eq!(app.terminals[&root].views.len(), 1);
    });

    // Reopening reuses it rather than spawning a second one.
    view.update_in(cx, |app, window, cx| {
        app.toggle_terminal(window, cx);
        assert_eq!(app.terminals[&root].views.len(), 1);
    });
    cx.run_until_parked();

    // A new tab goes to the front of the drawer and takes the focus.
    view.update_in(cx, |app, window, cx| {
        app.new_terminal(window, cx);
        let tabs = app.terminals.get(&root).unwrap();
        assert_eq!(tabs.views.len(), 2);
        assert_eq!(tabs.active, 1);
    });
    cx.run_until_parked();

    // Closing the active tab hands the drawer back to the first one.
    view.update_in(cx, |app, window, cx| {
        app.close_terminal_tab(1, window, cx);
        let tabs = app.terminals.get(&root).unwrap();
        assert_eq!(tabs.views.len(), 1);
        assert_eq!(tabs.active, 0);
        assert!(app.terminal_open);
    });
    cx.run_until_parked();

    // Closing the last tab closes the drawer with it.
    view.update_in(cx, |app, window, cx| {
        app.close_terminal_tab(0, window, cx);
        assert!(!app.terminals.contains_key(&root));
        assert!(!app.terminal_open);
    });
    cx.run_until_parked();

    // Closing the project releases any terminal it still had.
    view.update_in(cx, |app, window, cx| {
        app.toggle_terminal(window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| app.request(Next::Close, window, cx));
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(app.terminals.is_empty());
    });
}

/// A Run tab: one per project, busy from spawn until the command exits,
/// reused by the next run, and never handed a second command while
/// the first is up. `⇧⌘R` repeats the whole command, directory included.
#[gpui::test]
fn the_run_tab_is_busy_until_the_command_exits(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let sub = root.join("sub");
    std::fs::create_dir(&sub).unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();

    // Real children on a real pty: the test executor's clock does not
    // move them, so wait on wall time and let the poll run in between.
    let settle =
        |cx: &mut VisualTestContext, view: &Entity<Folio>, until: &dyn Fn(&Folio, &App) -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                cx.executor().advance_clock(Duration::from_millis(50));
                cx.run_until_parked();
                if view.read_with(cx, |app, cx| until(app, cx)) {
                    return;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the run never got there"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        };

    let command = |shell: &str, cwd: &Path| run::Command {
        label: "Run".into(),
        shell: shell.into(),
        cwd: cwd.to_path_buf(),
    };

    // Starting a run opens the drawer on a fresh Run tab that is busy.
    view.update_in(cx, |app, window, cx| {
        app.start_run_command(command("printf READY; sleep 30", &sub), window, cx);
        assert!(app.terminal_open);
        let tabs = &app.terminals[&root];
        assert_eq!(tabs.views.len(), 1);
        assert_eq!(tabs._watches.len(), 1);
        assert!(tabs.views[0].read(cx).is_run());
        assert!(app.run_is_busy(cx));
        assert_eq!(
            tabs.last_run.as_ref().map(|c| c.cwd.as_path()),
            Some(sub.as_path())
        );
    });
    settle(cx, &view, &|app, cx| {
        app.run_terminal(cx)
            .is_some_and(|view| view.read(cx).snapshot_text().contains("READY"))
    });

    // A second run while busy is refused: still one tab, still the
    // same child, and the warning says why.
    view.update_in(cx, |app, window, cx| {
        app.start_run_command(command("printf SECOND", &root), window, cx);
        assert_eq!(app.terminals[&root].views.len(), 1);
        assert!(app.run_is_busy(cx));
        assert!(
            app.notice
                .as_ref()
                .is_some_and(|n| n.text.contains("Already running")),
            "the refusal has to be said out loud"
        );
    });
    // A first Stop is Ctrl-C, and the sleep dies of it.
    view.update_in(cx, |app, window, cx| app.stop_run(window, cx));
    settle(cx, &view, &|app, cx| !app.run_is_busy(cx));
    view.update_in(cx, |app, _, cx| {
        let tab = app.run_terminal(cx).unwrap().read(cx);
        assert!(tab.is_run());
        assert!(tab.exit().is_some(), "a stopped run is an exited tab");
    });

    // Stop with nothing running does not touch the drawer.
    view.update_in(cx, |app, window, cx| {
        app.terminal_open = false;
        app.stop_run(window, cx);
        assert!(!app.terminal_open);
    });

    // The next run reuses the tab and runs from its own directory.
    view.update_in(cx, |app, window, cx| {
        app.start_run_command(command("pwd; printf DONE", &sub), window, cx);
        assert!(app.terminal_open);
        assert_eq!(app.terminals[&root].views.len(), 1);
        assert!(app.run_is_busy(cx));
    });
    settle(cx, &view, &|app, cx| !app.run_is_busy(cx));
    view.update_in(cx, |app, _, cx| {
        let text = app.run_terminal(cx).unwrap().read(cx).snapshot_text();
        assert!(text.contains("DONE"), "{text}");
        assert!(
            text.contains("/sub"),
            "the pty must start in the command's cwd: {text}"
        );
    });

    // Rerun repeats the last command from the same directory.
    view.update_in(cx, |app, window, cx| {
        app.rerun(window, cx);
        assert!(app.run_is_busy(cx));
    });
    settle(cx, &view, &|app, cx| !app.run_is_busy(cx));
    view.update_in(cx, |app, _, cx| {
        let text = app.run_terminal(cx).unwrap().read(cx).snapshot_text();
        assert!(text.contains("DONE"), "{text}");
    });

    // Closing the Run tab drops its watch with it.
    view.update_in(cx, |app, window, cx| {
        let index = app.run_terminal_index(cx).unwrap();
        app.close_terminal_tab(index, window, cx);
        assert!(!app.terminals.contains_key(&root));
    });
    cx.run_until_parked();
}

/// The icon rail and the file tree are two toggles. The title-bar
/// button owns the rail; `⌘B` and the first rail icon own the tree.
#[gpui::test]
fn the_activity_bar_and_the_file_tree_are_separate(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("main.rs"), "// original\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();

    view.update(cx, |app, _| {
        assert!(app.activity_bar);
        assert!(app.sidebar);
    });

    view.update(cx, |app, cx| {
        app.toggle_activity_bar(cx);
        assert!(!app.activity_bar);
        assert!(app.sidebar, "hiding the rail leaves the tree");
    });

    view.update(cx, |app, cx| {
        app.toggle_file_tree(cx);
        assert!(!app.sidebar);
        assert!(!app.activity_bar, "hiding the tree leaves the rail");
    });

    view.update(cx, |app, cx| {
        app.toggle_activity_bar(cx);
        app.toggle_file_tree(cx);
        assert!(app.activity_bar);
        assert!(app.sidebar);
    });
}

/// The changes view: `⇧⌘D` shows the active file's diff against HEAD,
/// follows the file it is pointed at, and goes away again.
#[gpui::test]
fn the_changes_view_shows_the_active_files_diff(cx: &mut TestAppContext) {
    use std::process::Command;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .unwrap()
    };
    assert!(git(&["init", "-q"]).status.success());
    let file = root.join("main.rs");
    let other = root.join("other.rs");
    std::fs::write(&file, "let one = 1;\nlet two = 2;\n").unwrap();
    std::fs::write(&other, "// untouched\n").unwrap();
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
    // One line changed after the commit, which is what the view shows.
    std::fs::write(&file, "let one = 1;\nlet two = 22;\n").unwrap();

    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |app, window, cx| app.toggle_diff(window, cx));
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        let shown = app.project.diff.as_ref().expect("the changes view is on");
        assert_eq!(shown.path, file);
        assert!(!shown.loading);
        assert!(shown.error.is_none());
        assert!(
            shown
                .lines
                .iter()
                .any(|line| { line.change == diff::Change::Added && line.text == "let two = 22;" })
        );
        assert!(
            shown.lines.iter().any(|line| {
                line.change == diff::Change::Removed && line.text == "let two = 2;"
            })
        );
        let added = shown
            .lines
            .iter()
            .find(|line| line.change == diff::Change::Added)
            .expect("the new line");
        let styles = shown
            .new_styles
            .get(&added.new.expect("added lines are numbered"))
            .expect("the new side is highlighted");
        assert!(
            styles.iter().any(|(_, style)| style.color.is_some()),
            "diff lines use the same colors as the editor"
        );
        // The editor area draws the list instead of the editor.
        let _ = app.render(window, cx);
    });

    // Opening another file moves the view onto that file's changes.
    view.update_in(cx, |app, window, cx| {
        app.open_file(other.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        let shown = app.project.diff.as_ref().expect("still on");
        assert_eq!(shown.path, other);
        assert!(!shown.loading);
        assert!(shown.lines.is_empty(), "the second file has no changes");
    });

    // Right-clicking the changes view offers the way back. It has to: the
    // editor's own menu cannot open while the editor is off screen, and
    // nothing else on this surface says how to leave.
    view.update_in(cx, |app, window, cx| {
        app.open_menu(
            MenuTarget::Changes {
                path: other.clone(),
            },
            point(px(200.), px(200.)),
            window,
            cx,
        );
        let menu = app.menu.as_ref().expect("the menu is open");
        assert_eq!(
            menu.target,
            MenuTarget::Changes {
                path: other.clone()
            }
        );
        assert_eq!(CHANGES_MENU[menu.selected].0, MenuItem::HideChanges);
        // The menu draws over the view.
        let _ = app.render(window, cx);
        app.run_menu_item(MenuItem::HideChanges, window, cx);
    });
    view.update_in(cx, |app, window, cx| {
        assert!(app.project.diff.is_none(), "the way back works");
        assert!(app.menu.is_none());
        // And the editor is what the area shows again.
        let _ = app.render(window, cx);
    });
}

#[gpui::test]
fn the_changes_list_opens_a_file_and_can_split(cx: &mut TestAppContext) {
    use std::process::Command;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .unwrap()
    };
    assert!(git(&["init", "-q"]).status.success());
    let file = root.join("main.rs");
    let extra = root.join("extra.rs");
    std::fs::write(&file, "let one = 1;\nlet two = 2;\n").unwrap();
    std::fs::write(&extra, "fn extra() {}\n").unwrap();
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
    std::fs::write(&file, "let one = 1;\nlet two = 22;\n").unwrap();
    std::fs::write(&extra, "fn extra() {}\nfn more() {}\n").unwrap();

    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();

    view.update(cx, |app, cx| {
        app.toggle_changes_list(cx);
        assert_eq!(app.sidebar_kind, SidebarKind::Changes);
        assert!(app.sidebar);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        let files = app.changed_files();
        assert!(
            files.iter().any(|(path, _)| path == &file)
                && files.iter().any(|(path, _)| path == &extra),
            "both dirty files show in the list: {files:?}"
        );
        // The review mark is per file and reversible.
        app.toggle_reviewed(&file, cx);
        assert!(app.project.reviewed.contains(&file));
        app.toggle_reviewed(&file, cx);
        assert!(!app.project.reviewed.contains(&file));
        app.toggle_reviewed(&file, cx);
        app.open_changed_file(file.clone(), window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, cx| {
        {
            let shown = app.project.diff.as_ref().expect("the file's diff is on");
            assert_eq!(shown.path, file);
            assert!(shown.lines.iter().any(|line| {
                line.change == diff::Change::Added && line.text == "let two = 22;"
            }));
            let replace = shown
                .split
                .iter()
                .find(|row| {
                    row.old
                        .as_ref()
                        .is_some_and(|cell| cell.text == "let two = 2;")
                        && row
                            .new
                            .as_ref()
                            .is_some_and(|cell| cell.text == "let two = 22;")
                })
                .expect("the replace sits on one split row");
            assert_eq!(replace.old.as_ref().unwrap().change, diff::Change::Removed);
            assert_eq!(replace.new.as_ref().unwrap().change, diff::Change::Added);
        }
        app.set_diff_layout(DiffLayout::Split, cx);
        assert_eq!(app.diff_layout, DiffLayout::Split);
        app.move_hunk(true, cx);
        assert_eq!(app.project.diff.as_ref().unwrap().hunk, 0);
    });
}

/// The settings window is created and drawn from inside `Folio`'s own
/// update — that is where the app menu dispatches `OpenSettings`, and
/// where `open_window` builds the root. Rendering the form must not read
/// the app view while that is in flight.
#[gpui::test]
fn the_settings_form_renders_from_inside_the_app_view_update(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        let folio = cx.entity().downgrade();
        let settings = cx.new(|cx| SettingsView::new(folio, app.settings.clone(), window, cx));
        settings.update(cx, |form, cx| {
            let _ = form.render(window, cx);
        });
    });
    // The form took the settings it was handed, without asking for them.
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.settings, Settings::default())
    });
}

/// The two predicates the pairing rules are built on, which are the part
/// worth pinning down: both come from Zed's editor, and both stop the
/// shortcut from doing something the user did not ask for.
#[test]
fn pairing_knows_where_a_closer_fits() {
    // Nothing after the caret, whitespace, or a closer already there.
    assert!(allows_autoclose(None));
    assert!(allows_autoclose(Some(' ')));
    assert!(allows_autoclose(Some(')')));
    assert!(allows_autoclose(Some(']')));
    assert!(allows_autoclose(Some('}')));
    // Anything else is where the rest of a word has to go, and a pair
    // opened there would swallow it.
    assert!(!allows_autoclose(Some('f')));
    assert!(!allows_autoclose(Some('1')));
    assert!(!allows_autoclose(Some('_')));
    assert!(!allows_autoclose(Some('"')));

    // A quote after a word is an apostrophe or a lifetime, not an opening
    // quote.
    assert!(is_word_char(Some('n')));
    assert!(is_word_char(Some('9')));
    assert!(is_word_char(Some('_')));
    assert!(!is_word_char(Some(' ')));
    assert!(!is_word_char(Some('(')));
    assert!(!is_word_char(None));
}

/// An opening bracket makes a pair with the caret between the two, a
/// closing one steps over a closer already there, and a selection is
/// wrapped rather than replaced.
#[gpui::test]
fn brackets_pair_and_step_over(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("main.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |app, window, cx| {
        let base = app.project.documents[&file]
            .editor
            .read(cx)
            .base_state()
            .clone();
        let value = |cx: &App| base.read(cx).value().to_string();
        let caret = |cx: &App| base.read(cx).selected_range();
        // Every case starts from a known buffer, so each one reads on its
        // own rather than off whatever the one before it left behind.
        // Takes the window rather than capturing it, so the calls below
        // can borrow it in turn.
        let start = |text: &str,
                     selection: std::ops::Range<usize>,
                     window: &mut Window,
                     cx: &mut Context<Folio>| {
            base.update(cx, |base, cx| {
                base.set_value(text, window, cx);
                base.set_selected_range(selection, cx);
            });
        };

        // An empty pair at the end of a line, with the caret between the
        // two characters.
        start("fn main() {}\n", 12..12, window, cx);
        app.open_pair("(", ")", window, cx);
        assert_eq!(value(cx), "fn main() {}()\n");
        assert_eq!(caret(cx), 13..13);

        // In front of a word it is typed alone: the closer would land
        // where the rest of the word has to go.
        start("fn main() {}\n", 0..0, window, cx);
        app.open_pair("(", ")", window, cx);
        assert_eq!(value(cx), "(fn main() {}\n");

        // A closing bracket steps over the one that is already there
        // instead of adding a second.
        start("()fn main() {}\n", 1..1, window, cx);
        app.skip_closer(")", window, cx);
        assert_eq!(value(cx), "()fn main() {}\n");
        assert_eq!(caret(cx), 2..2);

        // With nothing to step over it is just typed.
        start("()fn main() {}\n", 0..0, window, cx);
        app.skip_closer("}", window, cx);
        assert_eq!(value(cx), "}()fn main() {}\n");

        // A selection is wrapped, and the caret lands after the pair.
        start("fn main() {}\n", 0..2, window, cx);
        app.open_pair("(", ")", window, cx);
        assert_eq!(value(cx), "(fn) main() {}\n");
        assert_eq!(caret(cx), 4..4);

        // A quote makes a pair, and the next one steps over it.
        start(" \n", 1..1, window, cx);
        app.pair_quote("\"", window, cx);
        assert_eq!(value(cx), " \"\"\n");
        assert_eq!(caret(cx), 2..2);
        app.pair_quote("\"", window, cx);
        assert_eq!(value(cx), " \"\"\n");
        assert_eq!(caret(cx), 3..3);

        // A quote after a word is not opening a quote — an apostrophe
        // stays an apostrophe.
        start("don\n", 3..3, window, cx);
        app.pair_quote("'", window, cx);
        assert_eq!(value(cx), "don'\n");

        // Enter between a fresh pair lays it out over three lines, with
        // the caret on the line between and the closer at the opener's
        // indentation. One newline would leave the closer under the caret,
        // on the line the body was going to go on.
        start("    if x {}\n", 10..10, window, cx);
        base.update(cx, |base, cx| base.insert_line_break(window, cx));
        assert_eq!(value(cx), "    if x {\n        \n    }\n");
        assert_eq!(caret(cx), 19..19);

        // Anywhere else it is a line break and the indentation of the line
        // it breaks, which is what it has always been.
        start("    let x = 1;\n", 14..14, window, cx);
        base.update(cx, |base, cx| base.insert_line_break(window, cx));
        assert_eq!(value(cx), "    let x = 1;\n    \n");
        assert_eq!(caret(cx), 19..19);

        // Quotes are not brackets: a line break inside one is a string
        // being written over two lines, not a block with a body.
        start("let s = \"\";\n", 9..9, window, cx);
        base.update(cx, |base, cx| base.insert_line_break(window, cx));
        assert_eq!(value(cx), "let s = \"\n\";\n");
        assert_eq!(caret(cx), 10..10);
    });
}

#[gpui::test]
fn rainbow_brackets_colour_pairs_and_mark_the_match(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("main.rs");
    std::fs::write(&file, "fn main() { let x = (1); }\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, cx| {
        let doc = &app.project.documents[&file];
        assert_eq!(
            doc.brackets.rainbow.get_ranges(cx).len(),
            6,
            "three pairs, each painted on both sides"
        );
        let pairs = doc.editor.read(cx).bracket_pairs(cx);
        assert_eq!(
            folio::brackets::around_offsets(&pairs, 8),
            Some((7, 8)),
            "a caret on main's closing paren must find that pair"
        );
    });
    view.update_in(cx, |app, window, cx| {
        assert!(app.apply_disk_text(&file, "let x = (1);\n".into(), true, window, cx));
    });
    view.update_in(cx, |app, _, cx| {
        let doc = &app.project.documents[&file];
        assert_eq!(
            doc.brackets.rainbow.get_ranges(cx).len(),
            2,
            "set_value must rescan pairs, not keep the previous file's ranges"
        );
        let pairs = doc.editor.read(cx).bracket_pairs(cx);
        assert_eq!(folio::brackets::around_offsets(&pairs, 8), Some((8, 10)));
        assert_eq!(folio::brackets::around_offsets(&pairs, 11), Some((8, 10)));
    });
}

#[gpui::test]
fn rust_main_is_runnable_from_the_gutter(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    let file = root.join("src/main.rs");
    std::fs::write(&file, "fn main() {}\n#[test]\nfn adds() {}\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, cx| {
        assert!(app.can_run_active(cx));
        let doc = &app.project.documents[&file];
        assert_eq!(doc.editor.read(cx).runnable_lines(cx), vec![0, 2]);
        let text = doc.editor.read(cx).value();
        let runnables = folio::run::find(&file, &text);
        assert_eq!(runnables.len(), 2);
        assert_eq!(runnables[0].name, "main");
        assert_eq!(runnables[1].name, "adds");
        let cmd = folio::run::resolve(&root, &file, &text, Some(2)).unwrap();
        assert_eq!(cmd.shell, "cargo test adds -- --exact");
        assert_eq!(
            cmd.cwd, root,
            "the Run tab starts in the crate, so the command never cds"
        );
    });
    view.update_in(cx, |app, window, cx| {
        assert!(app.apply_disk_text(&file, "fn helper() {}\n".into(), true, window, cx));
    });
    view.update_in(cx, |app, _, cx| {
        assert!(
            app.project.documents[&file]
                .editor
                .read(cx)
                .runnable_lines(cx)
                .is_empty(),
            "set_value must rescan runnables, not keep the previous file's play icons"
        );
        assert!(
            app.can_run_active(cx),
            "a rust crate still runs via cargo run with no main in this file"
        );
    });
}

/// More than one cursor: the commands that make a set of selections, the
/// edit that lands at each of them, and the way back down to one.
///
/// A keystroke never reaches the view in this harness, so what is driven
/// here is what the keybinding's handler calls — one call per press — plus
/// the editor's own entry points for typing, deleting and Enter, which are
/// what the platform's input path calls.
#[gpui::test]
fn multi_cursor_edits_every_selection(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("main.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |app, window, cx| {
        let base = app.project.documents[&file]
            .editor
            .read(cx)
            .base_state()
            .clone();
        let value = |cx: &App| base.read(cx).value().to_string();
        let ranges = |cx: &App| base.read(cx).selected_ranges();
        let start = |text: &str,
                     selection: std::ops::Range<usize>,
                     window: &mut Window,
                     cx: &mut Context<Folio>| {
            base.update(cx, |base, cx| {
                base.set_value(text, window, cx);
                base.set_selected_range(selection, cx);
            });
        };

        // `⌘D` takes the word under the caret, then the next place it
        // appears, and stops when there is nothing left to take.
        let text = "let count = 1;\nlet other = count;\n";
        start(text, 5..5, window, cx);
        app.on_active_buffer(cx, |base, cx| base.select_next_occurrence(cx));
        assert_eq!(ranges(cx), vec![4..9]);
        app.on_active_buffer(cx, |base, cx| base.select_next_occurrence(cx));
        assert_eq!(ranges(cx), vec![4..9, 27..32]);
        app.on_active_buffer(cx, |base, cx| base.select_next_occurrence(cx));
        assert_eq!(ranges(cx), vec![4..9, 27..32]);

        // Typing lands at both, as one edit, and the carets end up after
        // what was typed at each of them.
        base.update(cx, |base, cx| {
            base.replace_in_every_selection("total", window, cx)
        });
        assert_eq!(value(cx), "let total = 1;\nlet other = total;\n");
        assert_eq!(ranges(cx), vec![9..9, 32..32]);

        // `⌘⇧L` takes every occurrence at once, and the one the caret was
        // in stays the one the caret is in.
        start(text, 5..5, window, cx);
        app.on_active_buffer(cx, |base, cx| base.select_all_occurrences(cx));
        assert_eq!(ranges(cx), vec![4..9, 27..32]);
        base.update(cx, |base, cx| assert!(base.collapse_selections(cx)));
        assert_eq!(ranges(cx), vec![4..9]);
        // An escape with one cursor is not the editor's to take.
        base.update(cx, |base, cx| assert!(!base.collapse_selections(cx)));

        // `⌥⌘↓` adds a caret on the line below, in the column the first one
        // was in, and `⌥⌘↑` adds one above the topmost.
        start("x = 1;\ny = 2;\n", 0..0, window, cx);
        app.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx));
        assert_eq!(ranges(cx), vec![0..0, 7..7]);
        base.update(cx, |base, cx| {
            base.replace_in_every_selection("let ", window, cx)
        });
        assert_eq!(value(cx), "let x = 1;\nlet y = 2;\n");
        assert_eq!(ranges(cx), vec![4..4, 15..15]);

        start("x = 1;\ny = 2;\n", 7..7, window, cx);
        app.on_active_buffer(cx, |base, cx| base.add_cursor_above(cx));
        assert_eq!(ranges(cx), vec![0..0, 7..7]);

        // Backspace at two carets takes the character before each of them.
        start("let a = 1;\nlet b = 2;\n", 4..4, window, cx);
        app.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx));
        assert_eq!(ranges(cx), vec![4..4, 15..15]);
        base.update(cx, |base, cx| {
            base.delete_at_every_selection(false, window, cx)
        });
        assert_eq!(value(cx), "leta = 1;\nletb = 2;\n");

        // Enter breaks at each caret, indented to the line it breaks.
        start("  one\n  two\n", 5..5, window, cx);
        app.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx));
        assert_eq!(ranges(cx), vec![5..5, 11..11]);
        base.update(cx, |base, cx| {
            base.insert_line_break_at_every_selection(window, cx)
        });
        assert_eq!(value(cx), "  one\n  \n  two\n  \n");
        assert_eq!(ranges(cx), vec![8..8, 17..17]);

        // Enter between two brackets at once lays out both pairs, and each
        // caret lands on the line between its own — not at the end of what
        // was inserted, which is where a caret goes for every other edit.
        start("if a {}\nif b {}\n", 6..6, window, cx);
        app.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx));
        assert_eq!(ranges(cx), vec![6..6, 14..14]);
        base.update(cx, |base, cx| base.insert_line_break(window, cx));
        assert_eq!(value(cx), "if a {\n    \n}\nif b {\n    \n}\n");
        assert_eq!(ranges(cx), vec![11..11, 25..25]);

        // A bracket with more than one cursor is typed at each of them
        // rather than paired around one selection.
        start("x = 1;\ny = 2;\n", 0..0, window, cx);
        app.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx));
        app.open_pair("(", ")", window, cx);
        assert_eq!(value(cx), "(x = 1;\n(y = 2;\n");
        assert_eq!(ranges(cx), vec![1..1, 9..9]);

        // Commenting is one edit over the block the cursors span.
        start("x = 1;\ny = 2;\n", 0..0, window, cx);
        app.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx));
        app.toggle_comment(window, cx);
        assert_eq!(value(cx), "// x = 1;\n// y = 2;\n");

        start("x = 1;\n", 0..0, window, cx);
        app.toggle_block_comment(window, cx);
        assert_eq!(value(cx), "/* x = 1; */\n");
        app.toggle_block_comment(window, cx);
        assert_eq!(value(cx), "x = 1;\n");

        start("one\ntwo\n", 0..0, window, cx);
        app.duplicate_lines(window, cx);
        assert_eq!(value(cx), "one\none\ntwo\n");

        start("one\ntwo\nthree\n", 0..0, window, cx);
        app.move_lines(true, window, cx);
        assert_eq!(value(cx), "two\none\nthree\n");
        app.move_lines(false, window, cx);
        assert_eq!(value(cx), "one\ntwo\nthree\n");
    });
}

/// Column selection: a rectangle becomes one selection per line it covers,
/// and typing replaces every line of it.
#[gpui::test]
fn a_column_selection_covers_a_rectangle(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("main.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |app, window, cx| {
        let base = app.project.documents[&file]
            .editor
            .read(cx)
            .base_state()
            .clone();
        let value = |cx: &App| base.read(cx).value().to_string();
        let ranges = |cx: &App| base.read(cx).selected_ranges();
        let start = |text: &str,
                     selection: std::ops::Range<usize>,
                     window: &mut Window,
                     cx: &mut Context<Folio>| {
            base.update(cx, |base, cx| {
                base.set_value(text, window, cx);
                base.set_selected_range(selection, cx);
            });
        };

        // The keys grow a column of carets, a line at a time, and take it
        // back the same way.
        start("aaa\nbbb\nccc\n", 1..1, window, cx);
        app.on_active_buffer(cx, |base, cx| {
            base.extend_box(1, cx);
        });
        assert_eq!(ranges(cx), vec![1..1, 5..5]);
        app.on_active_buffer(cx, |base, cx| {
            base.extend_box(1, cx);
        });
        assert_eq!(ranges(cx), vec![1..1, 5..5, 9..9]);
        app.on_active_buffer(cx, |base, cx| {
            base.extend_box(-1, cx);
        });
        assert_eq!(ranges(cx), vec![1..1, 5..5]);

        // The mouse's rectangle: the columns between the corner it started
        // at and the one it was dragged to, on every line between them.
        start("aaa\nbbb\nccc\n", 0..0, window, cx);
        base.update(cx, |base, cx| {
            base.begin_box(0, cx);
            base.drag_box_to(10, cx);
        });
        assert_eq!(ranges(cx), vec![0..2, 4..6, 8..10]);

        // Typing goes into every line of it, as one edit.
        base.update(cx, |base, cx| {
            base.replace_in_every_selection("X", window, cx)
        });
        assert_eq!(value(cx), "Xa\nXb\nXc\n");
        assert_eq!(ranges(cx), vec![1..1, 4..4, 7..7]);

        // A line too short for the rectangle gives up at its own end
        // rather than reaching into the next one.
        start("aaa\nb\nccc\n", 0..0, window, cx);
        base.update(cx, |base, cx| {
            base.begin_box(0, cx);
            base.drag_box_to(8, cx);
        });
        assert_eq!(ranges(cx), vec![0..2, 4..5, 6..8]);

        // A movement is not part of the rectangle, so the keys start a new
        // one from where the caret has gone.
        base.update(cx, |base, cx| base.set_selected_range(1..1, cx));
        app.on_active_buffer(cx, |base, cx| {
            base.extend_box(1, cx);
        });
        assert_eq!(ranges(cx), vec![1..1, 5..5]);
    });
}

/// Folding: the block the caret is in closes, and a caret that was inside
/// what closed goes to the header rather than staying on a line nothing
/// draws.
///
/// The fold candidates are handed in rather than derived: in the app they
/// come from the grammar's fold query, through a highlighter the component
/// library attaches when it renders the input — which this harness cannot
/// do (see the note above `a_keystroke_matches_the_shortcut_it_prints`).
/// That the query produces them is the crate's business; what is checked
/// here is what the editor does with them.
#[gpui::test]
fn folding_closes_the_block_at_the_caret(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("main.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |app, _window, cx| {
        let base = app.project.documents[&file]
            .editor
            .read(cx)
            .base_state()
            .clone();
        let caret = |cx: &App| base.read(cx).selected_range();
        let folded = |cx: &App| base.read(cx).folded_lines();

        // `fn main() {` on line 0 with the `if` block on 1..3 inside it, so
        // the caret in `let y` is in two blocks at once.
        base.update(cx, |base, cx| {
            base.set_value(
                "fn main() {\n    if x {\n        let y = 1;\n    }\n}\n",
                _window,
                cx,
            );
            base.apply_highlighter_fold_candidates(
                vec![input::FoldRange::new(0, 4), input::FoldRange::new(1, 3)],
                cx,
            );
            base.set_selected_range(30..30, cx);
        });
        assert!(folded(cx).is_empty());

        // The innermost block, so pressing again takes the one around it.
        base.update(cx, |base, cx| assert!(base.fold_at_cursor(cx)));
        assert_eq!(folded(cx), vec![1]);
        // The caret was on a line that stopped being drawn.
        assert_eq!(caret(cx), 22..22);

        base.update(cx, |base, cx| assert!(base.unfold_at_cursor(cx)));
        assert!(folded(cx).is_empty());

        // The outer block, from the header line.
        base.update(cx, |base, cx| {
            base.set_selected_range(0..0, cx);
            assert!(base.fold_at_cursor(cx));
        });
        assert_eq!(folded(cx), vec![0]);

        // Nothing to fold and nothing to unfold are both quiet, and the
        // caret is left alone either way.
        base.update(cx, |base, cx| {
            base.set_selected_range(50..50, cx);
            assert!(!base.fold_at_cursor(cx));
            assert!(!base.unfold_at_cursor(cx));
        });
        assert_eq!(folded(cx), vec![0]);
        assert_eq!(caret(cx), 50..50);
    });
}

/// A file is opened with the indentation its project asks for, ahead of
/// what the settings say.
///
/// Read through Enter between a pair of brackets, since the indentation is
/// what that writes and the editor does not expose its tab size otherwise.
#[gpui::test]
fn an_editorconfig_decides_a_files_indentation(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(
        root.join(".editorconfig"),
        "root = true\n\n[*.rs]\nindent_style = space\nindent_size = 2\n",
    )
    .unwrap();
    let file = root.join("main.rs");
    std::fs::write(&file, "if x {}\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            // The settings ask for four spaces; the file asks for two.
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |app, window, cx| {
        assert_eq!(app.settings.tab_size, 4);
        let base = app.project.documents[&file]
            .editor
            .read(cx)
            .base_state()
            .clone();
        base.update(cx, |base, cx| {
            base.set_selected_range(6..6, cx);
            base.insert_line_break(window, cx);
        });
        assert_eq!(base.read(cx).value(), "if x {\n  \n}\n");
    });
}

/// A session is put back: the projects that were open, the files each was
/// showing in the order the strip had them, and the buffer whose edits
/// never reached the disk.
#[gpui::test]
fn a_session_is_put_back(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let a = root.join("A");
    let b = root.join("B");
    for dir in [&a, &b] {
        std::fs::create_dir(dir).unwrap();
    }
    let a_main = a.join("main.rs");
    let a_notes = a.join("notes.md");
    let b_lib = b.join("lib.rs");
    std::fs::write(&a_main, "// a\n").unwrap();
    std::fs::write(&a_notes, "// notes\n").unwrap();
    std::fs::write(&b_lib, "// b\n").unwrap();

    let config = root.join("config");
    std::fs::create_dir(&config).unwrap();
    let session_file = config.join("session.json");
    let unsaved_file = config.join("unsaved.json");
    session::Session {
        projects: vec![
            session::SessionProject {
                root: a.clone(),
                tabs: vec![a_main.clone(), a_notes.clone()],
                active: Some(a_main.clone()),
                ..Default::default()
            },
            session::SessionProject {
                root: b.clone(),
                tabs: vec![b_lib.clone()],
                active: Some(b_lib.clone()),
                ..Default::default()
            },
        ],
    }
    .save(&session_file)
    .unwrap();
    let mut unsaved = session::UnsavedBuffers::new();
    unsaved.insert(a_notes.clone(), "// recovered\n".to_string());
    session::save_unsaved(&unsaved_file, &unsaved).unwrap();

    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = config.join("recent.json");
            app.settings_file = config.join("settings.json");
            app.window_file = config.join("window.json");
            app.session_file = session_file.clone();
            app.unsaved_file = unsaved_file.clone();
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| app.start_session(window, cx));
    // A project and each of its files are separate turns of the executor,
    // so the queue drains over several of them.
    for _ in 0..8 {
        cx.run_until_parked();
    }

    view.update_in(cx, |app, _, cx| {
        assert_eq!(app.project_order, vec![a.clone(), b.clone()]);
        // The second project is the one being shown; the first is parked
        // with everything it had.
        assert_eq!(app.parked.len(), 1);
        let first = &app.parked[0];
        assert_eq!(first.tabs(), [a_main.clone(), a_notes.clone()].as_slice());
        assert_eq!(first.active(), Some(&a_main));
        // The buffer that had unsaved edits came back dirty, with them.
        let notes = &first.documents[&a_notes];
        assert!(notes.dirty);
        assert_eq!(notes.editor.read(cx).value().as_ref(), "// recovered\n");
        // And the one that did not is clean.
        assert!(!first.documents[&a_main].dirty);

        assert_eq!(app.project.tabs().to_vec(), vec![b_lib.clone()]);
        assert_eq!(app.project.active(), Some(&b_lib));
    });
}

#[gpui::test]
fn a_split_session_comes_back_as_two_panes(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let project = root.join("proj");
    std::fs::create_dir(&project).unwrap();
    let left = project.join("left.rs");
    let right = project.join("right.rs");
    std::fs::write(&left, "// L\n").unwrap();
    std::fs::write(&right, "// R\n").unwrap();
    let config = root.join("config");
    std::fs::create_dir(&config).unwrap();
    let session_file = config.join("session.json");
    session::Session {
        projects: vec![session::SessionProject {
            root: project.clone(),
            tabs: vec![left.clone()],
            active: Some(left.clone()),
            right_tabs: vec![right.clone()],
            right_active: Some(right.clone()),
            split_vertical: false,
            reviewed: Vec::new(),
            recent_files: Vec::new(),
        }],
    }
    .save(&session_file)
    .unwrap();

    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = config.join("recent.json");
            app.settings_file = config.join("settings.json");
            app.window_file = config.join("window.json");
            app.session_file = session_file;
            app.unsaved_file = config.join("unsaved.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| app.start_session(window, cx));
    for _ in 0..8 {
        cx.run_until_parked();
    }
    view.update(cx, |app, _| {
        assert_eq!(app.project.panes.len(), 2);
        assert_eq!(app.project.panes[0].tabs, vec![left.clone()]);
        assert_eq!(app.project.panes[1].tabs, vec![right.clone()]);
        assert_eq!(app.project.focused, 0);
        assert_eq!(app.project.active(), Some(&left));
    });
}

/// What is open, and what is unsaved, get written down while the app runs;
/// a clean exit takes the unsaved file with it, which is what leaves the
/// file behind only when there was no clean exit.
#[gpui::test]
fn the_session_and_unsaved_edits_are_written_down(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("main.rs");
    std::fs::write(&file, "// original\n").unwrap();
    let config = root.join("config");
    std::fs::create_dir(&config).unwrap();
    let session_file = config.join("session.json");
    let unsaved_file = config.join("unsaved.json");

    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = config.join("recent.json");
            app.settings_file = config.join("settings.json");
            app.window_file = config.join("window.json");
            app.session_file = session_file.clone();
            app.unsaved_file = unsaved_file.clone();
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();
    // Nothing has been written yet: the timer has not come round.
    assert!(!session_file.exists());

    view.update_in(cx, |app, window, cx| {
        let base = app.project.documents[&file]
            .editor
            .read(cx)
            .base_state()
            .clone();
        base.update(cx, |base, cx| base.replace_all("// edited\n", window, cx));
    });
    cx.run_until_parked();

    view.update_in(cx, |app, _, cx| {
        assert!(app.project.documents[&file].dirty);
        let written = app.pending_writes(cx);
        let (written, error) = written.write();
        assert!(error.is_none());
        app.written_unsaved = written.unsaved.unwrap();
        app.written_revisions = written.revisions;
        // Nothing has moved since that snapshot: the next tick is a
        // revision comparison, not another read of every dirty buffer.
        assert!(app.pending_writes(cx).unsaved.is_none());
    });

    let session = session::Session::load(&session_file);
    assert_eq!(session.projects.len(), 1);
    assert_eq!(session.projects[0].tabs, vec![file.clone()]);
    assert_eq!(session.projects[0].active.as_ref(), Some(&file));
    assert_eq!(session::load_unsaved(&unsaved_file)[&file], "// edited\n");

    // A new edit bumps the revision, so the next tick has work again.
    view.update_in(cx, |app, window, cx| {
        let base = app.project.documents[&file]
            .editor
            .read(cx)
            .base_state()
            .clone();
        base.update(cx, |base, cx| {
            base.replace_all("// edited again\n", window, cx)
        });
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, cx| {
        let next = app.pending_writes(cx);
        assert!(next.unsaved.is_some());
        assert_eq!(
            next.revisions[&file], app.project.documents[&file].revision,
            "the snapshot is tagged with the revision it read"
        );
    });

    // Saving it would have emptied the unsaved file; quitting leaves it
    // behind either way, which is what the next launch reads.
    view.update_in(cx, |app, _, _| {
        app.finish_session();
        assert!(session_file.exists());
    });
    assert!(session::load_unsaved(&unsaved_file).is_empty());
}

#[gpui::test]
fn editor_copies_the_active_file_path(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("main.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |app, window, cx| {
        app.open_menu(
            MenuTarget::Editor { path: file.clone() },
            point(px(80.), px(80.)),
            window,
            cx,
        );
        assert_eq!(
            visible_menu_items(&app.menu.as_ref().unwrap().target)
                .into_iter()
                .map(|index| EDITOR_MENU[index].1)
                .collect::<Vec<_>>(),
            [
                "Cut",
                "Copy",
                "Paste",
                "Select All",
                "Go to Line…",
                "Reveal in Finder",
                "Open in Terminal",
                "Copy Path",
                "Copy Relative Path",
                "Show File Changes",
                "Blame",
            ]
        );
        app.run_menu_item(MenuItem::CopyRelativePath, window, cx);
    });
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("main.rs")
    );
    view.update_in(cx, |app, _, cx| app.copy_active_path(false, cx));
    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("the absolute path");
    assert_eq!(Path::new(&copied), file.as_path());
}

/// Blame: the strip says who last wrote the line the caret is on, and says
/// something else when the caret moves to a line another commit wrote.
#[gpui::test]
fn blame_follows_the_caret(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("main.rs");
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
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
    std::fs::write(&file, "let one = 1;\nlet two = 2;\n").unwrap();
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
    std::fs::write(&file, "let one = 1;\nlet two = 22;\n").unwrap();
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

    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |app, _, cx| {
        // Nothing is shown until it is asked for.
        assert!(app.blame_text(cx).is_none());
    });
    view.update_in(cx, |app, window, cx| app.toggle_blame(window, cx));
    // The blame is a git call, which happens behind the window.
    cx.run_until_parked();

    view.update_in(cx, |app, _, cx| {
        let base = app.project.documents[&file]
            .editor
            .read(cx)
            .base_state()
            .clone();
        let text = app.blame_text(cx).expect("a line to blame");
        assert!(text.contains("fixture"), "{text}");
        assert!(text.contains("Folio Test"), "{text}");

        // The second line was rewritten by someone else.
        base.update(cx, |base, cx| base.set_selected_range(17..17, cx));
        let text = app.blame_text(cx).expect("a line to blame");
        assert!(text.contains("second pass"), "{text}");
        assert!(text.contains("Ada Lovelace"), "{text}");
    });

    view.update_in(cx, |app, window, cx| {
        app.toggle_blame(window, cx);
        assert!(app.blame_text(cx).is_none());
    });
}

/// The shortcut strings the menus print are matched against real
/// keystrokes, and the capture-phase claim for the project search rests on
/// this one matching. The key dispatch itself cannot be exercised here, so
/// what is pinned is the predicate rather than the dispatch.
///
/// Why it cannot, since `simulate_keystrokes` is the obvious thing to try.
/// Dispatch resolves against the drawn frame, and nothing is drawn in a
/// test window: `add_empty_window` has no root view, so the tests that need
/// a tree build one by hand. Giving the window a root view with
/// `add_window_view` does make the harness draw — and then the first draw
/// with an input focused panics inside gpui-component, which asks the
/// window for its `NSView` to set a text content type and gets "Test
/// Windows are not backed by a real platform window". An editor is an
/// input, and opening a file focuses one, so there is no order that both
/// draws and keeps a focus. Both halves were checked against the editor.
#[test]
fn a_keystroke_matches_the_shortcut_it_prints() {
    let keystroke = |source: &str| Keystroke::parse(source).unwrap();
    assert!(matches_shortcut(
        &keystroke("cmd-shift-f"),
        "\u{2318}\u{21e7}F"
    ));
    // A different key, a missing modifier, an extra one.
    assert!(!matches_shortcut(
        &keystroke("cmd-shift-f"),
        "\u{2318}\u{21e7}H"
    ));
    assert!(!matches_shortcut(
        &keystroke("shift-f"),
        "\u{2318}\u{21e7}F"
    ));
    assert!(!matches_shortcut(&keystroke("cmd-f"), "\u{2318}\u{21e7}F"));
    assert!(matches_shortcut(
        &keystroke("cmd-shift-z"),
        "\u{2318}\u{21e7}Z"
    ));
    // The menu's own shortcuts are matched the same way.
    assert!(matches_shortcut(
        &keystroke("alt-cmd-t"),
        "\u{2325}\u{2318}T"
    ));
}

/// The About window builds. Its version comes from the manifest, so there
/// is nothing to assert about it that the compiler has not already.
#[gpui::test]
fn the_about_view_renders(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let about = cx.update(|_, cx| cx.new(|_| AboutView));
    about.update_in(cx, |view, window, cx| {
        // The harness never draws on its own.
        let _ = view.render(window, cx);
    });
}

/// Markdown opens rendered. Source is the same buffer, one toggle away.
#[gpui::test]
fn markdown_opens_rendered_and_switches_to_source(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let note = root.join("note.md");
    std::fs::write(&note, "# Hello\n\nA paragraph with **bold**.\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(note.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        let document = &app.project.documents[&note];
        assert!(document.preview);
        assert_eq!(
            document.editor.read(cx).value().as_str(),
            "# Hello\n\nA paragraph with **bold**.\n"
        );
        let _ = app.render(window, cx);
        app.set_markdown_preview(false, window, cx);
        assert!(!app.project.documents[&note].preview);
        let _ = app.render(window, cx);
        app.toggle_markdown_preview(window, cx);
        assert!(app.project.documents[&note].preview);
    });
    view.update_in(cx, |app, window, cx| {
        app.open_file(note.clone(), window, cx);
        app.goto = Some((note.clone(), Position::new(2, 0)));
        app.apply_goto(&note, window, cx);
        assert!(
            !app.project.documents[&note].preview,
            "a jump is a place in the source"
        );
    });
}

#[test]
fn font_choices_keep_bundled_fonts_first_and_custom_fonts_available() {
    let options = font_options(
        vec![
            "Menlo".into(),
            "Fira Code".into(),
            "Menlo".into(),
            ".Hidden".into(),
        ],
        Some("Custom Mono"),
        true,
    );
    assert_eq!(
        &options[..3],
        &["JetBrains Mono", "Fira Code", "Source Code Pro"]
    );
    assert_eq!(&options[3..], &["Custom Mono", "Menlo"]);
    assert_eq!(font_options(vec![], None, false)[0], "System default");
}

/// The settings form end to end: what a change writes to disk and how it
/// reaches the tree. `show_settings` itself is not covered — opening a
/// window needs a real platform window, which the test harness does not
/// have.
#[gpui::test]
fn settings_view_persists_and_drives_the_tree(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let src = root.join("src");
    for name in ["src", "notes"] {
        std::fs::create_dir(root.join(name)).unwrap();
    }
    std::fs::write(src.join("main.rs"), "// hi\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |app, window, cx| {
        app.open_file(src.join("main.rs"), window, cx)
    });
    cx.run_until_parked();
    let open_editor = view.update_in(cx, |app, _, cx| {
        app.project.documents[&src.join("main.rs")]
            .editor
            .read(cx)
            .base_state()
            .clone()
    });
    cx.update(|window, _| window.blur());

    // The settings form, wired to the app view the way the window wires it.
    let settings = view.update_in(cx, |app, window, cx| {
        let folio = cx.entity().downgrade();
        let settings = cx.new(|cx| SettingsView::new(folio, app.settings.clone(), window, cx));
        app.settings_view = Some(settings.clone());
        settings
    });

    // Every page builds, not just the one it opens on, with headings both
    // open and closed; the harness never draws on its own.
    settings.update_in(cx, |view, window, cx| {
        assert_eq!(view.page, Page::Type);
        assert!(view.expanded.iter().all(|open| *open));
        for collapsed in [false, true] {
            view.expanded = [collapsed; GROUPS.len()];
            for group in GROUPS {
                for page in group.pages {
                    view.page = *page;
                    let _ = view.render(window, cx);
                }
            }
        }
        view.expanded = [true; GROUPS.len()];
        view.page = Page::Interface;
    });

    // Stepping a value applies it and writes the file.
    settings.update_in(cx, |view, _, cx| {
        view.change(
            |settings| {
                settings.tab_size = 2;
                settings.hard_tabs = true;
            },
            cx,
        );
    });
    cx.run_until_parked();
    let stored = settings::load(&root.join("settings.json")).unwrap();
    assert_eq!((stored.tab_size, stored.hard_tabs), (2, true));

    // A value outside the range is pulled back rather than written.
    settings.update_in(cx, |view, _, cx| {
        view.change(|settings| settings.font_size = 99., cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert_eq!(app.settings.font_size, settings::FONT_SIZE.1);
    });

    // Confirming a dropdown applies the family and persists it. Exercise
    // the same event as mouse/keyboard confirmation, including defaults.
    for family in ["Fira Code", "Source Code Pro", "JetBrains Mono"] {
        settings.update_in(cx, |view, window, cx| {
            view.code_font_select.update(cx, |select, cx| {
                select.set_selected_value(&family.to_owned(), window, cx);
                cx.emit(SelectEvent::Confirm(Some(family.to_owned())));
            });
        });
        cx.run_until_parked();
        let stored = settings::load(&root.join("settings.json")).unwrap();
        assert_eq!(
            stored.code_font_family.as_deref(),
            (family != "JetBrains Mono").then_some(family)
        );
        view.update_in(cx, |app, _, cx| {
            assert_eq!(app.code_font().as_ref(), family);
            assert_eq!(cx.theme().mono_font_family.as_ref(), family);
        });
        // Draw the same open document after changing the dropdown, then
        // compare actual glyph geometry with the selected font's metrics.
        cx.draw(point(px(0.), px(0.)), size(px(800.), px(500.)), |_, _| {
            div().w(px(800.)).h(px(500.)).child(view.clone())
        });
        cx.update(|window, cx| {
            assert_eq!(
                open_editor
                    .read(cx)
                    .rendered_font()
                    .unwrap()
                    .family
                    .as_ref(),
                family
            );
            let actual = open_editor
                .read(cx)
                .range_to_bounds(&(0..5))
                .expect("the open code must have been painted")
                .size
                .width;
            let run = TextRun {
                len: 5,
                font: gpui::font(family),
                color: gpui::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let expected = window
                .text_system()
                .shape_line("// hi".into(), px(14.), &[run], None)
                .width;
            assert!(
                (f32::from(actual - expected)).abs() < 0.1,
                "{family}: painted {actual:?}, expected {expected:?}"
            );
        });
    }

    settings.update_in(cx, |view, window, cx| {
        view.font_select.update(cx, |select, cx| {
            select.set_selected_value(&"Source Code Pro".to_owned(), window, cx);
            cx.emit(SelectEvent::Confirm(Some("Source Code Pro".to_owned())));
        });
    });
    cx.run_until_parked();
    assert_eq!(
        settings::load(&root.join("settings.json"))
            .unwrap()
            .font_family
            .as_deref(),
        Some("Source Code Pro")
    );
    cx.draw(point(px(0.), px(0.)), size(px(800.), px(500.)), |_, _| {
        div().w(px(800.)).h(px(500.)).child(view.clone())
    });
    cx.update(|_, cx| {
        assert_eq!(
            open_editor
                .read(cx)
                .rendered_font()
                .unwrap()
                .family
                .as_ref(),
            "JetBrains Mono"
        );
    });
    settings.update_in(cx, |view, window, cx| {
        view.font_select.update(cx, |select, cx| {
            select.set_selected_value(&"System default".to_owned(), window, cx);
            cx.emit(SelectEvent::Confirm(Some("System default".to_owned())));
        });
        view.code_font_select.update(cx, |select, cx| {
            select.set_selected_value(&"Fira Code".to_owned(), window, cx);
            cx.emit(SelectEvent::Confirm(Some("Fira Code".to_owned())));
        });
    });
    cx.run_until_parked();
    // Settling an unrelated input must not overwrite the chosen font.
    settings.update_in(cx, |view, window, cx| {
        view.ignore_query
            .update(cx, |input, cx| input.set_value("src, notes", window, cx));
        view.commit_text(window, cx);
    });
    cx.run_until_parked();
    let stored = settings::load(&root.join("settings.json")).unwrap();
    assert_eq!(stored.code_font_family.as_deref(), Some("Fira Code"));
    assert_eq!(stored.font_family, None);
    assert_eq!(stored.ignored, vec!["src".to_string(), "notes".to_string()]);
    // Reopening the form restores the persisted selection.
    settings.update_in(cx, |view, window, cx| {
        let reopened = cx.new(|cx| SettingsView::new(view.folio.clone(), stored, window, cx));
        assert_eq!(
            reopened
                .read(cx)
                .code_font_select
                .read(cx)
                .selected_value()
                .map(String::as_str),
            Some("Fira Code")
        );
    });

    // Both folders are ignored now, so the tree and the index agree that
    // neither is there — the new rules reached the cached listings.
    view.update_in(cx, |app, _, _| {
        assert!(
            app.project
                .rows
                .iter()
                .all(|row| row.entry.name != "src" && row.entry.name != "notes"),
            "the new rules reached the cached listings"
        );
        assert!(app.project.files.iter().all(|path| !path.starts_with(&src)));
    });
}

/// Open the tree's context menu over a folder, the way a right-click does.
fn open_tree_menu(
    app: &mut Folio,
    folder: &Path,
    root: bool,
    window: &mut Window,
    cx: &mut Context<Folio>,
) {
    app.open_menu(
        MenuTarget::Tree {
            path: folder.to_path_buf(),
            root,
        },
        point(px(120.), px(200.)),
        window,
        cx,
    );
}

#[gpui::test]
fn tree_context_menu_creates_moves_and_duplicates_entries(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let src = root.join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "// child\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            // Never read or write the real settings file from a test.
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |app, window, cx| {
        assert!(
            app.project
                .rows
                .iter()
                .any(|row| row.entry.path == src && row.depth == 1),
            "the first folder under the project header is indented"
        );
        assert!(
            !app.menu_item_enabled(
                MenuItem::Paste,
                &MenuTarget::Tree {
                    path: root.clone(),
                    root: true
                },
                cx
            ),
            "paste stays disabled until something is on the clipboard"
        );
        open_tree_menu(app, &src, false, window, cx);
        assert!(app.menu.is_some());
        // The harness never draws, so build the element tree by hand to
        // exercise the menu's rendering.
        let _ = app.render(window, cx);
        app.move_menu_selection(true);
        assert_eq!(app.menu.as_ref().unwrap().selected, 1);
        let target = app.menu.as_ref().unwrap().target.clone();
        assert!(app.menu_item_enabled(TREE_MENU[1].0, &target, cx));

        app.run_menu_item(MenuItem::NewFile, window, cx);
        assert!(app.menu.is_none(), "choosing an item closes the menu");
        let edit = app.editing.as_ref().expect("the row is being named");
        assert_eq!(edit.kind, EntryKind::File);
        assert_eq!(edit.parent, src);
        // The field is spliced in as the folder's first child, one level
        // deeper than the folder's own row. The folder itself sits at
        // depth 1 because the project root is a separate header.
        assert_eq!(app.project.rows[edit.row - 1].entry.path, src);
        assert_eq!(app.project.rows[edit.row - 1].depth, 1);
        assert_eq!(app.project.rows[edit.row].depth, 2);
        // That row draws a text field instead of an entry; render it too.
        let _ = app.render(window, cx);
    });

    // Enter is what creates the entry, so nothing exists yet.
    assert!(!src.join("笔记.md").exists());
    view.update_in(cx, |app, window, cx| {
        let input = app.editing.as_ref().unwrap().input.clone();
        input.update(cx, |input, cx| input.set_value("笔记.md", window, cx));
        app.commit_edit(window, cx);
        assert!(app.editing.is_none());
    });
    cx.run_until_parked();
    let note = src.join("笔记.md");
    assert!(note.is_file(), "the named file must exist on disk");
    view.update_in(cx, |app, _, _| {
        assert!(app.project.rows.iter().any(|row| row.entry.path == note));
        assert_eq!(app.project.active(), Some(&note));
    });

    // Escape abandons the row without touching the filesystem.
    let before = std::fs::read_dir(&src).unwrap().count();
    view.update_in(cx, |app, window, cx| {
        app.begin_create(src.clone(), EntryKind::Directory, window, cx);
        assert!(app.editing.is_some());
        app.cancel_create(window, cx);
        assert!(app.editing.is_none());
    });
    assert_eq!(std::fs::read_dir(&src).unwrap().count(), before);

    // A name the filesystem cannot hold is reported, not written.
    view.update_in(cx, |app, window, cx| {
        app.begin_create(src.clone(), EntryKind::File, window, cx);
        let input = app.editing.as_ref().unwrap().input.clone();
        input.update(cx, |input, cx| input.set_value("a/b", window, cx));
        app.commit_edit(window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, _| {
        assert!(
            app.notice
                .as_ref()
                .is_some_and(|n| n.text.contains("path separator")),
            "an unusable name must surface as a message"
        );
    });

    // Cut then paste moves the entry and empties the clipboard.
    view.update_in(cx, |app, window, cx| {
        open_tree_menu(app, &note, false, window, cx);
        app.run_menu_item(MenuItem::Cut, window, cx);
        assert_eq!(app.clipboard.as_ref().map(|entry| entry.cut), Some(true));
        open_tree_menu(app, &root, false, window, cx);
        app.run_menu_item(MenuItem::Paste, window, cx);
        assert!(
            app.clipboard.is_some(),
            "a cut remains pending until it lands"
        );
    });
    cx.run_until_parked();
    let moved = root.join("笔记.md");
    assert!(moved.is_file() && !note.exists());
    view.update(cx, |app, _| assert!(app.clipboard.is_none()));

    // Copy leaves the source behind, so the name is taken in `src` again.
    view.update_in(cx, |app, window, cx| {
        open_tree_menu(app, &moved, false, window, cx);
        app.run_menu_item(MenuItem::Copy, window, cx);
        open_tree_menu(app, &src, false, window, cx);
        app.run_menu_item(MenuItem::Paste, window, cx);
        assert_eq!(app.clipboard.as_ref().map(|entry| entry.cut), Some(false));
    });
    cx.run_until_parked();
    assert!(src.join("笔记.md").is_file() && moved.is_file());

    view.update_in(cx, |app, window, cx| {
        open_tree_menu(app, &moved, false, window, cx);
        app.run_menu_item(MenuItem::Duplicate, window, cx);
    });
    cx.run_until_parked();
    assert!(root.join("笔记 copy.md").is_file());

    // Find in Folder narrows the project search to the folder it opened on.
    view.update_in(cx, |app, window, cx| {
        open_tree_menu(app, &src, false, window, cx);
        app.run_menu_item(MenuItem::FindInFolder, window, cx);
        assert_eq!(app.panel, Some(Panel::Search));
        assert_eq!(app.search.scope.as_deref(), Some(src.as_path()));
    });

    // Reopening the panel from the keyboard clears that scope again.
    view.update_in(cx, |app, window, cx| {
        app.show_search(false, window, cx);
        assert!(app.search.scope.is_none());
    });

    // The project root is the workspace's identity, so its menu drops the
    // entries that would rename or delete that folder. Removing it from
    // the project list is offered there, and only there.
    let labels = |target: &MenuTarget| {
        visible_menu_items(target)
            .into_iter()
            .map(|index| surface_of(target).menu().0[index].1)
            .collect::<Vec<_>>()
    };
    let tree_labels = |root: bool| {
        labels(&MenuTarget::Tree {
            path: PathBuf::new(),
            root,
        })
    };
    for dropped in ["Rename", "Move to Trash", "Delete Immediately"] {
        assert!(
            !tree_labels(true).contains(&dropped),
            "{dropped} on the root"
        );
        assert!(
            tree_labels(false).contains(&dropped),
            "{dropped} inside a folder"
        );
    }
    assert!(
        tree_labels(true).contains(&"Remove Project"),
        "the root can leave the project list"
    );
    assert!(
        !tree_labels(false).contains(&"Remove Project"),
        "a folder is not a project"
    );
    // The changes view offers the way out and nothing the tree has.
    let changes = labels(&MenuTarget::Changes {
        path: PathBuf::new(),
    });
    assert_eq!(changes, vec!["Hide File Changes"]);
    assert_eq!(
        labels(&MenuTarget::Split { pane: 0 }),
        vec!["Split Right", "Split Left", "Split Up", "Split Down"]
    );
    assert_eq!(
        labels(&MenuTarget::Editor {
            path: PathBuf::new()
        }),
        vec![
            "Cut",
            "Copy",
            "Paste",
            "Select All",
            "Go to Line…",
            "Reveal in Finder",
            "Open in Terminal",
            "Copy Path",
            "Copy Relative Path",
            "Show File Changes",
            "Blame",
        ]
    );
    assert!(!tree_labels(false).contains(&"Hide File Changes"));
    view.update_in(cx, |app, window, cx| {
        open_tree_menu(app, &root, true, window, cx);
        assert_eq!(
            app.menu.as_ref().unwrap().target,
            MenuTarget::Tree {
                path: root.clone(),
                root: true
            }
        );
        open_tree_menu(app, &src, false, window, cx);
        assert_eq!(
            app.menu.as_ref().unwrap().target,
            MenuTarget::Tree {
                path: src.clone(),
                root: false
            }
        );
        app.close_menu(cx);
    });

    // Rename moves the entry and takes the caches keyed off its path with
    // it, including an open buffer.
    let library = src.join("lib.rs");
    view.update_in(cx, |app, window, cx| {
        app.open_file(library.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        open_tree_menu(app, &library, false, window, cx);
        app.run_menu_item(MenuItem::Rename, window, cx);
        let edit = app.editing.as_ref().expect("the row is being renamed");
        assert_eq!(edit.renaming.as_deref(), Some(library.as_path()));
        // The field takes over the entry's own row instead of adding one.
        assert_eq!(app.project.rows[edit.row].entry.path, library);
        let input = edit.input.clone();
        input.update(cx, |input, cx| input.set_value("core.rs", window, cx));
        app.commit_edit(window, cx);
    });
    cx.run_until_parked();
    let core = src.join("core.rs");
    assert!(core.is_file() && !library.exists());
    // The open buffer moved with it under its new key, and the entries that
    // were not renamed stayed where they were.
    view.update_in(cx, |app, _, _| {
        assert!(app.project.documents.contains_key(&core));
        assert_eq!(app.project.active(), Some(&core));
        assert!(app.project.documents.contains_key(&moved));
        assert!(!app.project.documents.contains_key(&src.join("笔记.md")));
    });

    // Trashing is recoverable, so it only stops when it would drop edits
    // that are not on disk. Cancelling must leave the folder alone.
    view.update_in(cx, |app, window, cx| {
        // The test harness cannot drive the editor's change events, so put
        // the open buffer into the state a real edit would leave it in.
        app.project.documents.get_mut(&core).unwrap().dirty = true;
        assert_eq!(app.unsaved_under(&src), 1);
        open_tree_menu(app, &src, false, window, cx);
        app.run_menu_item(MenuItem::Trash, window, cx);
    });
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(src.is_dir() && core.is_file());

    // Delete always asks, because nothing about it can be undone.
    let doomed = root.join("doomed");
    std::fs::create_dir(&doomed).unwrap();
    std::fs::write(doomed.join("inner.txt"), "x").unwrap();
    view.update_in(cx, |app, window, cx| {
        open_tree_menu(app, &doomed, false, window, cx);
        app.run_menu_item(MenuItem::Delete, window, cx);
    });
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Delete");
    cx.run_until_parked();
    assert!(!doomed.exists(), "the whole subtree must go");
    view.update_in(cx, |app, _, _| {
        assert!(
            app.notice
                .as_ref()
                .is_some_and(|n| n.text.contains("Deleted")),
            "a successful delete reports back instead of erroring"
        );
    });

    // Removing a project takes it off the list and leaves the folder.
    view.update_in(cx, |app, window, cx| {
        app.project.documents.get_mut(&core).unwrap().dirty = false;
        open_tree_menu(app, &root, true, window, cx);
        app.run_menu_item(MenuItem::RemoveProject, window, cx);
        assert!(app.project.workspace.is_none());
        assert!(app.project_order.is_empty());
    });
    assert!(src.is_dir(), "remove must not delete the project folder");
    assert!(core.is_file(), "remove must not delete files inside it");
}

#[gpui::test]
fn external_edits_reload_clean_buffers_and_prompt_dirty_ones(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("main.rs");
    std::fs::write(&file, "fn first() {}\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();

    std::fs::write(&file, "fn second() {}\n").unwrap();
    view.update_in(cx, |app, window, cx| {
        app.apply_watch_paths(HashMap::from([(file.clone(), false)]), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, cx| {
        let document = &app.project.documents[&file];
        assert_eq!(document.editor.read(cx).value(), "fn second() {}\n");
        assert!(!document.dirty);
        assert!(app.project.disk_notice.is_empty());
    });

    view.update_in(cx, |app, window, cx| {
        let editor = app.project.documents[&file].editor.clone();
        editor.read(cx).base_state().clone().update(cx, |base, cx| {
            base.replace_all("fn local() {}\n", window, cx)
        });
        app.project.documents.get_mut(&file).unwrap().dirty = true;
    });
    std::fs::write(&file, "fn third() {}\n").unwrap();
    view.update_in(cx, |app, window, cx| {
        app.apply_watch_paths(HashMap::from([(file.clone(), false)]), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        let document = &app.project.documents[&file];
        assert_eq!(document.editor.read(cx).value(), "fn local() {}\n");
        assert_eq!(
            app.project.disk_notice.get(&file),
            Some(&DiskNotice::Changed)
        );
        app.project.disk_notice.remove(&file);
        app.reload_from_disk(file.clone(), window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, cx| {
        let document = &app.project.documents[&file];
        assert_eq!(document.editor.read(cx).value(), "fn third() {}\n");
        assert!(!document.dirty);
        assert!(app.project.disk_notice.is_empty());
    });
}

#[gpui::test]
fn dropping_a_tree_row_moves_the_file_and_its_buffer(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let src = root.join("src");
    std::fs::create_dir(&src).unwrap();
    let file = root.join("main.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.project.documents[&file]
            .editor
            .read(cx)
            .base_state()
            .clone()
            .update(cx, |base, cx| {
                base.replace_all("fn moved() {}\n", window, cx)
            });
        app.project.documents.get_mut(&file).unwrap().dirty = true;
        app.drop_tree_entry(file.clone(), src.clone(), window, cx);
    });
    cx.run_until_parked();
    let dest = src.join("main.rs");
    assert!(dest.is_file());
    assert!(!file.exists());
    view.update_in(cx, |app, _, cx| {
        let document = app
            .project
            .documents
            .get(&dest)
            .expect("the open buffer follows the file");
        assert_eq!(document.editor.read(cx).value(), "fn moved() {}\n");
        assert!(document.dirty);
        assert!(!app.project.documents.contains_key(&file));
    });
}

#[gpui::test]
fn composing_then_undo_or_escape_returns_to_the_buffer_before_pinyin(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file = root.join("main.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |app, window, cx| {
        let base = app.project.documents[&file]
            .editor
            .read(cx)
            .base_state()
            .clone();
        let value = |cx: &App| base.read(cx).value().to_string();
        // Caret sits before the closer, the way a comment would be typed.
        base.update(cx, |base, cx| {
            base.set_selected_range(11..11, cx);
            EntityInputHandler::replace_and_mark_text_in_range(
                base,
                None,
                "ni",
                Some(2..2),
                window,
                cx,
            );
            EntityInputHandler::replace_and_mark_text_in_range(
                base,
                None,
                "你",
                Some(1..1),
                window,
                cx,
            );
            EntityInputHandler::replace_text_in_range(base, None, "你", window, cx);
        });
        assert_eq!(value(cx), "fn main() {你}\n");
        base.update(cx, |base, cx| base.undo(&input::Undo, window, cx));
        assert_eq!(
            value(cx),
            "fn main() {}\n",
            "one undo must drop the whole composition"
        );

        base.update(cx, |base, cx| {
            base.set_selected_range(11..11, cx);
            EntityInputHandler::replace_and_mark_text_in_range(
                base,
                None,
                "hao",
                Some(3..3),
                window,
                cx,
            );
            base.escape(&input::Escape, window, cx);
        });
        assert_eq!(
            value(cx),
            "fn main() {}\n",
            "escape during a composition must not leave the pinyin"
        );

        app.panel = Some(Panel::Search);
        base.update(cx, |base, cx| {
            base.set_selected_range(11..11, cx);
            EntityInputHandler::replace_and_mark_text_in_range(
                base,
                None,
                "hao",
                Some(3..3),
                window,
                cx,
            );
        });
        assert!(
            app.is_composing(cx),
            "a marked run has to keep Escape for the editor"
        );
        assert_eq!(app.panel, Some(Panel::Search));
    });
}

#[gpui::test]
fn editor_snapshots_and_multicursor_changes(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let editor = cx.update(|window, cx| {
        cx.new(|cx| EditorState::new("text", window, cx).default_value("foo foo foo"))
    });
    editor.update_in(cx, |editor, window, cx| editor.prepare(window, cx));
    let before = editor.read_with(cx, |editor, _| editor.snapshot());
    let changes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = changes.clone();
    let subscription = editor.update(cx, |editor, cx| {
        cx.subscribe(editor.base_state(), move |_, _, event, _| {
            if matches!(event, InputEvent::Change) {
                count.fetch_add(1, Ordering::Relaxed);
            }
        })
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.base_state().clone().update(cx, |base, cx| {
            base.set_selected_range(0..3, cx);
            base.set_search_query("foo", false, cx);
            base.select_all_occurrences(cx);
            base.replace_in_every_selection("bar", window, cx);
            assert_eq!(base.search_session().matcher.len(), 0);
        });
    });
    cx.run_until_parked();
    assert_eq!(changes.load(Ordering::Relaxed), 1);
    assert_eq!(before, "foo foo foo");
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.snapshot(), "bar bar bar");
        assert_eq!(editor.value(), "bar bar bar");
        assert!(editor.is_modified("foo foo foo"));
        assert!(!editor.is_modified("bar bar bar"));
    });
    editor.update_in(cx, |editor, window, cx| {
        editor
            .base_state()
            .clone()
            .update(cx, |base, cx| base.undo(&input::Undo, window, cx));
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| assert_eq!(editor.value(), "foo foo foo"));
    editor.update_in(cx, |editor, window, cx| {
        editor.set_value("foo ".repeat(10_000), window, cx);
        editor.base_state().clone().update(cx, |base, cx| {
            base.set_selected_range(0..3, cx);
            base.select_all_occurrences(cx);
            assert_eq!(base.selected_ranges().len(), 1000);
        });
    });
    drop(subscription);
}

/// The in-file search overlay re-seeds its box (`set_value` + `select_all`)
/// whenever `search_session.query` changes. Keystrokes go through
/// `set_search_query` and must only update the matcher, or each character
/// would select the whole query and the next key would replace it.
#[gpui::test]
fn typing_in_search_does_not_reselect_the_query(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let editor = cx.update(|window, cx| {
        cx.new(|cx| EditorState::new("text", window, cx).default_value("hello hello"))
    });
    editor.update_in(cx, |editor, window, cx| editor.prepare(window, cx));
    editor.update_in(cx, |editor, _, cx| {
        editor.base_state().clone().update(cx, |base, cx| {
            base.set_selected_range(0..5, cx);
            base.open_search(false, cx);
            assert_eq!(base.search_session().query, "hello");
            assert_eq!(base.search_session().matcher.len(), 2);

            base.set_search_query("hell", false, cx);
            assert_eq!(
                base.search_session().query,
                "hello",
                "the open query is what the overlay uses to decide to select-all"
            );
            assert_eq!(base.search_session().matcher.len(), 2);

            base.set_search_query("x", false, cx);
            assert_eq!(base.search_session().query, "hello");
            assert!(base.search_session().matcher.is_empty());
        });
    });
}

#[gpui::test]
fn a_cross_project_move_keeps_tracking_edits(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let a = root.join("a");
    let b = root.join("b");
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();
    let file = a.join("x.rs");
    let dest = b.join("x.rs");
    std::fs::write(&file, "fn before() {}\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(a.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.open_file(file.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(b.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.drop_tree_entry(file.clone(), b.clone(), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        assert!(app.project.documents.contains_key(&dest));
        app.open_file(dest.clone(), window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.project.documents[&dest]
            .editor
            .read(cx)
            .base_state()
            .clone()
            .update(cx, |base, cx| {
                base.replace_all("fn after() {}\n", window, cx)
            });
    });
    cx.run_until_parked();
    view.update_in(cx, |app, _, cx| {
        let doc = &app.project.documents[&dest];
        assert_eq!(doc.editor.read(cx).value(), "fn after() {}\n");
        assert!(
            doc.dirty,
            "moving between projects must retain dirty-change subscription"
        );
    });
}

#[test]
fn a_failed_recovery_write_is_not_acknowledged() {
    let tmp = tempfile::tempdir().unwrap();
    let blocker = tmp.path().join("not-a-directory");
    std::fs::write(&blocker, "x").unwrap();
    let writes = Writes {
        session_file: blocker.join("session.json"),
        unsaved_file: blocker.join("unsaved.json"),
        session: None,
        unsaved: Some(UnsavedSnapshot::from([(
            tmp.path().join("x.rs"),
            "edits".into(),
        )])),
        revisions: BTreeMap::new(),
    };
    let (written, error) = writes.write();
    assert!(error.is_some());
    assert!(
        written.unsaved.is_none(),
        "a failed recovery write must not be returned as acknowledged"
    );
}

#[gpui::test]
fn right_terminal_dock_and_split_panes_keep_their_own_columns(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let src = root.join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("main.rs"), "// hi\n").unwrap();
    cx.update(gpui_component::init);
    let cx = cx.add_empty_window();
    let view = cx.update(|window, cx| {
        cx.new(|cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.recent_file = root.join("recent.json");
            app.settings_file = root.join("settings.json");
            app.window_file = root.join("window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        })
    });
    view.update_in(cx, |app, window, cx| {
        app.request(Next::Open(root.clone()), window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |app, window, cx| {
        app.terminal_open = true;
        app.terminal_right = true;
        app.ensure_terminal(cx);
        app.split_editor(window, cx);
    });
    cx.run_until_parked();
    // A focused input syncs its native content type, which needs a
    // real macOS window; the layout below only needs the tree.
    cx.update(|window, _| window.blur());
    cx.draw(point(px(0.), px(0.)), size(px(1600.), px(900.)), |_, _| {
        div().w(px(1600.)).h(px(900.)).child(view.clone())
    });
    let left = cx
        .debug_bounds("editor-tabs-0")
        .expect("pane 0's tab strip was drawn");
    let right = cx
        .debug_bounds("editor-tabs-1")
        .expect("pane 1's tab strip was drawn");
    let dock = cx
        .debug_bounds("terminal-dock")
        .expect("the terminal dock was drawn");
    eprintln!("FOLIO_DEBUG tabs0={left:?} tabs1={right:?} dock={dock:?} viewport=1600x900");
    assert!(
        f32::from(left.right()) <= f32::from(dock.origin.x) + 0.5,
        "pane 0's tab strip ({left:?}) must not run under the right dock ({dock:?})"
    );
    assert!(
        f32::from(right.right()) <= f32::from(dock.origin.x) + 0.5,
        "pane 1's tab strip ({right:?}) must not run under the right dock ({dock:?})"
    );
}
