use super::*;

impl Folio {
    pub(super) fn remove_recent(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.refresh_recent(RecentAction::Remove(path), cx);
    }

    pub(super) fn refresh_recent(&mut self, action: RecentAction, cx: &mut Context<Self>) {
        let config = self.recent_file.clone();
        // ponytail: one window's JSON operations run in order; use a file lock if multiple instances are supported.
        let previous = self.recent_task.take();
        let executor = cx.background_executor().clone();
        self.recent_task = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let result = executor
                .spawn(async move {
                    match action {
                        RecentAction::Load => recent::load(&config),
                        RecentAction::Open(path) => recent::record(&path, &config),
                        RecentAction::Remove(path) => recent::remove(&path, &config),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(items) => this.recent = items,
                    Err(e) => this.error(format!("Could not update recent projects: {e}"), cx),
                }
                cx.notify();
            });
        }));
    }

    /// Queue up what was open the last time, and start putting it back.
    ///
    /// The session is read here rather than in a task — one small file, the way
    /// the settings are read — and anything in it that is no longer on disk was
    /// dropped as it was read.
    pub(super) fn start_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = session::Session::load(&self.session_file);
        let unsaved = session::load_unsaved(&self.unsaved_file);

        for project in &session.projects {
            self.restore
                .push_back(Restore::Project(project.root.clone()));
            self.restore
                .extend(project.tabs.iter().cloned().map(Restore::File));
            if !project.right_tabs.is_empty() {
                self.restore.push_back(Restore::Split {
                    vertical: project.split_vertical,
                });
                self.restore
                    .extend(project.right_tabs.iter().cloned().map(Restore::File));
            }
            // Recovered buffers belong to this project, and are opened while it
            // is the one being shown. After both columns' tabs, so a right-pane
            // file is already where it belongs and `show_path` does not move it.
            for (path, text) in &unsaved {
                if path.starts_with(&project.root) {
                    self.restore
                        .push_back(Restore::Unsaved(path.clone(), text.clone()));
                }
            }
            if !project.reviewed.is_empty() {
                self.restore
                    .push_back(Restore::Reviewed(project.reviewed.clone()));
            }
            if !project.recent_files.is_empty() {
                self.restore
                    .push_back(Restore::RecentFiles(project.recent_files.clone()));
            }
            if let Some(active) = &project.right_active {
                self.restore.push_back(Restore::Activate(active.clone()));
            }
            if let Some(active) = &project.active {
                self.restore.push_back(Restore::Activate(active.clone()));
            }
            if !project.right_tabs.is_empty() {
                self.restore.push_back(Restore::FocusPane(0));
            }
        }

        if !self.restore.is_empty() {
            self.next_restore(window, cx);
        }
    }

    /// Work through the restore queue, one step at a time.
    ///
    /// Each step starts the next when it lands: opening a project and opening a
    /// file are both asynchronous, and either starting while the other is in
    /// flight would have the one in flight thrown away.
    pub(super) fn next_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(step) = self.restore.pop_front() else {
            match self.recovered {
                0 => {}
                1 => self.toast("Recovered an unsaved file", cx),
                count => self.toast(&format!("Recovered {count} unsaved files"), cx),
            }
            self.recovered = 0;
            return;
        };
        match step {
            Restore::Project(root) => self.open_project(root, window, cx),
            Restore::File(path) => self.open_file(path, window, cx),
            Restore::Activate(path) => {
                // Already open, so this is not an open at all: it is the focus
                // and the strip's idea of which file is being read, both of
                // which the last file opened would otherwise have taken.
                if let Some(document) = self.project.documents.get(&path) {
                    document.editor.read(cx).focus_handle(cx).focus(window, cx);
                    self.project.show_path(path.clone());
                    self.apply_goto(&path, window, cx);
                    self.update_title(window);
                    cx.notify();
                }
                self.next_restore(window, cx);
            }
            Restore::Unsaved(path, text) => {
                self.restoring = Some((path.clone(), text));
                self.open_file(path, window, cx);
            }
            Restore::Reviewed(paths) => {
                self.project.reviewed = paths.into_iter().collect();
                cx.notify();
                self.next_restore(window, cx);
            }
            Restore::RecentFiles(paths) => {
                self.project.recent_files = paths;
                cx.notify();
                self.next_restore(window, cx);
            }
            Restore::Split { vertical } => {
                if self.project.panes.len() < 2 {
                    self.project.panes.push(Pane::default());
                }
                self.project.split_axis = if vertical {
                    SplitAxis::Vertical
                } else {
                    SplitAxis::Horizontal
                };
                self.project.focused = 1;
                self.next_restore(window, cx);
            }
            Restore::FocusPane(index) => {
                self.focus_pane(index, window, cx);
                self.next_restore(window, cx);
            }
        }
    }

    /// The projects that are open, in the order they were opened, with the
    /// files each one has showing.
    pub(super) fn collect_session(&self) -> session::Session {
        let mut projects = Vec::new();
        for root in &self.project_order {
            let Some(project) = self.project_for(root) else {
                continue;
            };
            let Some(workspace) = project.workspace.as_ref() else {
                continue;
            };
            let left = project.panes.first();
            let right = project.panes.get(1);
            projects.push(session::SessionProject {
                root: workspace.root.clone(),
                tabs: left.map(|pane| pane.tabs.clone()).unwrap_or_default(),
                active: left.and_then(|pane| pane.active.clone()),
                right_tabs: right.map(|pane| pane.tabs.clone()).unwrap_or_default(),
                right_active: right.and_then(|pane| pane.active.clone()),
                split_vertical: project.split_axis == SplitAxis::Vertical,
                reviewed: {
                    let mut reviewed = project.reviewed.iter().cloned().collect::<Vec<_>>();
                    reviewed.sort();
                    reviewed
                },
                recent_files: project.recent_files.clone(),
            });
        }
        session::Session { projects }
    }

    /// The project with this root, whichever list it is in.
    pub(super) fn project_for(&self, root: &Path) -> Option<&Project> {
        std::iter::once(&self.project)
            .chain(self.parked.iter())
            .find(|project| {
                project
                    .workspace
                    .as_ref()
                    .is_some_and(|workspace| workspace.root == root)
            })
    }

    /// Every buffer with edits that are not on disk, across every project.
    /// The dirty buffers and their current revisions, without snapshotting any
    /// text. This is the cheap half: when every revision matches what was last
    /// written, there is nothing to do and no Rope has to be walked.
    pub(super) fn dirty_revisions(&self) -> Vec<(PathBuf, u64)> {
        std::iter::once(&self.project)
            .chain(self.parked.iter())
            .flat_map(|project| project.documents.iter())
            .filter(|(_, document)| document.dirty)
            .map(|(path, document)| (path.clone(), document.revision))
            .collect()
    }

    /// What has changed since the last write, if anything.
    pub(super) fn pending_writes(&self, cx: &App) -> Writes {
        let session = self.collect_session();
        let dirty = self.dirty_revisions();
        let changed = dirty.len() != self.written_revisions.len()
            || dirty
                .iter()
                .any(|(path, revision)| self.written_revisions.get(path) != Some(revision));
        let revisions = if changed {
            dirty.iter().cloned().collect()
        } else {
            self.written_revisions.clone()
        };
        let unsaved = changed.then(|| {
            std::iter::once(&self.project)
                .chain(self.parked.iter())
                .flat_map(|project| project.documents.iter())
                .filter(|(_, document)| document.dirty)
                .map(|(path, document)| {
                    // Unchanged buffers reuse the Rope already in the
                    // snapshot; only the ones that moved are read again.
                    let text = match self.written_revisions.get(path) {
                        Some(revision) if *revision == document.revision => self
                            .written_unsaved
                            .get(path)
                            .cloned()
                            .unwrap_or_else(|| document.editor.read(cx).snapshot()),
                        _ => document.editor.read(cx).snapshot(),
                    };
                    (path.clone(), text)
                })
                .collect()
        });
        Writes {
            session_file: self.session_file.clone(),
            unsaved_file: self.unsaved_file.clone(),
            session: (session != self.written_session).then_some(session),
            unsaved,
            revisions,
        }
    }

    /// Write down what is open and what is unsaved, while the app is running.
    ///
    /// On a timer rather than from every place that changes something: what
    /// this is for is the case where there is no clean exit to write at.
    pub(super) fn watch_unsaved(&mut self, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(UNSAVED_EVERY).await;
                let Ok(writes) = this.update(cx, |this, cx| this.pending_writes(cx)) else {
                    // The window is gone, and with it anything worth writing.
                    break;
                };
                if writes.is_empty() {
                    continue;
                }
                let (written, error) = executor.spawn(async move { writes.write() }).await;
                // Remembered only once the write has landed, so a failure is
                // retried on the next tick rather than assumed done.
                let _ = this.update(cx, |this, cx| {
                    if error != this.recovery_error {
                        if let Some(error) = &error {
                            this.error(error.clone(), cx);
                        }
                        this.recovery_error = error;
                    }
                    if let Some(session) = written.session {
                        this.written_session = session;
                    }
                    if let Some(unsaved) = written.unsaved {
                        this.written_unsaved = unsaved;
                        this.written_revisions = written.revisions;
                    }
                });
            }
        })
        .detach();
    }

    pub(super) fn sync_disk_watch(&mut self) {
        self.disk_watch
            .sync(&self.project_order, &self.settings.ignored);
    }

    /// Drain kernel events and apply them to the tree, git dots, and open
    /// buffers. Started once with the session so a test window that never
    /// calls `start_session` does not watch the machine.
    pub(super) fn watch_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_disk_watch();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(watch::DEBOUNCE).await;
                let Ok(paths) = this.update(cx, |this, _| this.disk_watch.take()) else {
                    break;
                };
                if paths.is_empty() {
                    continue;
                }
                let _ = this.update_in(cx, |this, window, cx| {
                    this.apply_watch_paths(paths, window, cx);
                });
            }
        })
        .detach();
    }

    pub(super) fn quiet_write(&mut self, path: &Path) {
        self.quiet_writes.insert(
            path.to_path_buf(),
            Instant::now() + watch::QUIET_AFTER_WRITE,
        );
    }

    pub(super) fn apply_watch_paths(
        &mut self,
        paths: HashMap<PathBuf, bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = Instant::now();
        self.quiet_writes.retain(|_, until| now < *until);
        let ignored = self.settings.ignored.clone();
        let mut work = Vec::new();
        for project in std::iter::once(&self.project).chain(self.parked.iter()) {
            let Some(workspace) = &project.workspace else {
                continue;
            };
            let relevant: Vec<_> = paths
                .iter()
                .filter(|(path, _)| path.starts_with(&workspace.root))
                .collect();
            if relevant.is_empty() {
                continue;
            }
            // A `.gitignore` edit changes which paths belong in the index, and
            // its own event is a data write, not a create/remove. Treat it as
            // a full structural refresh.
            let rules_changed = relevant
                .iter()
                .any(|(path, _)| path.file_name().is_some_and(|name| name == ".gitignore"));
            let mut full = paths.get(&workspace.root) == Some(&true) || rules_changed;
            let cached = project.directories.keys().cloned().collect::<HashSet<_>>();
            let open = project.documents.keys().cloned().collect::<HashSet<_>>();
            let mut batch = watch::batch(
                relevant.iter().map(|(path, _)| (*path).clone()),
                &[],
                &self.quiet_writes,
                now,
                &cached,
                &open,
            );
            let structure = rules_changed
                || relevant.iter().any(|(path, changed)| {
                    **changed
                        && !watch::is_quiet(path, &self.quiet_writes, now)
                        && !watch::is_git_metadata(path)
                        && path
                            .strip_prefix(&workspace.root)
                            .is_ok_and(|relative| !watch::under_ignored(relative, &ignored))
                });
            let mut created = Vec::new();
            let mut removed = Vec::new();
            if structure && !full {
                for (path, _) in &relevant {
                    if watch::is_quiet(path, &self.quiet_writes, now)
                        || watch::is_git_metadata(path)
                    {
                        continue;
                    }
                    let Ok(relative) = path.strip_prefix(&workspace.root) else {
                        continue;
                    };
                    if watch::under_ignored(relative, &ignored) {
                        continue;
                    }
                    match path.symlink_metadata() {
                        Ok(meta) if meta.is_file() => created.push((*path).clone()),
                        Ok(meta) if meta.is_dir() => {
                            // A directory appeared or moved. A full walk is
                            // both cheap at this granularity and the only
                            // answer that clears a renamed-away old tree.
                            full = true;
                            break;
                        }
                        // Symlinks are not tree entries.
                        Ok(_) => {}
                        Err(_) => removed.push((*path).clone()),
                    }
                }
            }
            if !structure || full {
                created.clear();
                removed.clear();
            }
            if !structure {
                batch.directories.clear();
            }
            if full {
                batch.directories = cached.into_iter().collect();
                batch.files = open.into_iter().collect();
                batch.git = true;
            }
            work.push((
                project.id,
                workspace.root.clone(),
                batch,
                structure,
                full,
                created,
                removed,
            ));
        }
        let mut files = HashSet::new();
        for (project_id, root, batch, structure, full, created, removed) in work {
            for dir in batch.directories {
                self.reload_directory(dir, cx);
            }
            if structure {
                if full || (created.is_empty() && removed.is_empty()) {
                    self.reindex_project(project_id, cx);
                } else {
                    self.apply_index_delta(project_id, root, created, removed, cx);
                }
            }
            if batch.git {
                self.refresh_git_project(project_id, cx);
            }
            files.extend(batch.files);
        }
        if files.is_empty() {
            cx.notify();
            return;
        }
        let task = cx.background_executor().spawn(async move {
            files
                .into_iter()
                .map(|path| {
                    let result = buffer::read(&path);
                    (path, result)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn_in(window, async move |this, cx| {
            let results = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.apply_disk_reads(results, window, cx);
            });
        })
        .detach();
    }

    /// Apply a small file-level create/remove to the quick-open index instead
    /// of walking the whole project. Removal is just a prefix filter; a new
    /// file is checked against `.gitignore` in the background before it joins.
    ///
    /// A directory create or a rename still asks for a full reindex: that is
    /// the only answer that is correct when the old side of a rename may not
    /// have produced an event. `indexing` keeps this from racing the full walk:
    /// a request that arrives meanwhile is folded into `index_again`.
    pub(super) fn apply_index_delta(
        &mut self,
        project_id: u64,
        root: PathBuf,
        created: Vec<PathBuf>,
        removed: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.project_mut(project_id) else {
            return;
        };
        if project.indexing {
            project.index_again = true;
            return;
        }
        project.indexing = true;
        if !removed.is_empty() {
            // A removed directory arrives as one path; a removed file as
            // itself. Walking the file's ancestors makes both O(depth) per
            // entry instead of comparing every file with every removed path.
            let gone = removed.iter().cloned().collect::<HashSet<_>>();
            let mut files = std::mem::take(&mut project.files);
            files.retain(|file| !file.ancestors().any(|component| gone.contains(component)));
            project.set_files(files);
        }
        if created.is_empty() {
            project.indexing = false;
            if self.project.id == project_id {
                self.filter(cx);
            }
            cx.notify();
            return;
        }

        let ignored = self.settings.ignored.clone();
        let task = cx.background_executor().spawn(async move {
            let rules = tree::IgnoreRules::new(&root, &ignored);
            let mut added = Vec::new();
            for path in created {
                if path.symlink_metadata().is_ok_and(|meta| meta.is_file())
                    && !rules.is_ignored(&path, false)
                {
                    added.push(path);
                }
            }
            added
        });
        cx.spawn(async move |this, cx| {
            let added = task.await;
            let _ = this.update(cx, |this, cx| {
                let again = {
                    let Some(project) = this.project_mut(project_id) else {
                        return;
                    };
                    project.indexing = false;
                    std::mem::take(&mut project.index_again)
                };
                if again {
                    this.reindex_project(project_id, cx);
                    return;
                }
                if !added.is_empty()
                    && let Some(project) = this.project_mut(project_id)
                {
                    let mut files = std::mem::take(&mut project.files);
                    files.extend(added);
                    files.sort();
                    files.dedup();
                    project.set_files(files);
                }
                if this.project.id == project_id {
                    this.filter(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn apply_disk_reads(
        &mut self,
        results: Vec<(PathBuf, io::Result<String>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut reloaded = 0;
        let mut removed = Vec::new();
        for (path, result) in results {
            match result {
                Ok(text) => {
                    if self.apply_disk_text(&path, text, false, window, cx) {
                        reloaded += 1;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    if self.mark_disk_removed(&path) {
                        removed.push(name(&path));
                    }
                }
                Err(_) => {}
            }
        }
        if reloaded == 1 {
            self.toast("Reloaded from disk", cx);
        } else if reloaded > 1 {
            self.toast(&format!("Reloaded {reloaded} files from disk"), cx);
        }
        for name in removed {
            self.warn(&format!("{name} was removed on disk"), cx);
        }
        if self.blame.is_some() {
            self.read_blame(window, cx);
        }
        self.update_title(window);
        cx.notify();
    }

    /// Apply a disk snapshot to an open buffer. Forced reloads come from the
    /// banner and overwrite local edits; the watch path only overwrites when
    /// the buffer is clean. Returns whether a clean buffer was replaced.
    pub(super) fn apply_disk_text(
        &mut self,
        path: &Path,
        text: String,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(project_id) = self.project_id_for_document(path) else {
            return false;
        };
        let Some(document) = self
            .project_mut(project_id)
            .and_then(|project| project.documents.get(path))
        else {
            return false;
        };
        let editor = document.editor.clone();
        let dirty = document.dirty;
        let saved = document.saved.to_string();
        if text == saved {
            if let Some(project) = self.project_mut(project_id) {
                project.disk_notice.remove(path);
            }
            return false;
        }
        if dirty && !force {
            if let Some(project) = self.project_mut(project_id) {
                project
                    .disk_notice
                    .insert(path.to_path_buf(), DiskNotice::Changed);
            }
            return false;
        }
        if let Some(document) = self
            .project_mut(project_id)
            .and_then(|project| project.documents.get_mut(path))
        {
            document.saved = text.clone().into();
        }
        editor.update(cx, |editor, cx| editor.set_value(text, window, cx));
        if let Some(document) = self
            .project_mut(project_id)
            .and_then(|project| project.documents.get_mut(path))
        {
            document.revision = document.revision.wrapping_add(1);
        }
        self.refresh_brackets(&editor, cx);
        self.refresh_runnables(&editor, cx);
        if let Some(project) = self.project_mut(project_id) {
            if let Some(document) = project.documents.get_mut(path) {
                document.dirty = document.editor.read(cx).is_modified(&document.saved);
            }
            project.disk_notice.remove(path);
        }
        true
    }

    pub(super) fn mark_disk_removed(&mut self, path: &Path) -> bool {
        let Some(project_id) = self.project_id_for_document(path) else {
            return false;
        };
        let Some(project) = self.project_mut(project_id) else {
            return false;
        };
        let Some(document) = project.documents.get(path) else {
            return false;
        };
        if document.dirty {
            project
                .disk_notice
                .insert(path.to_path_buf(), DiskNotice::Removed);
            false
        } else {
            true
        }
    }

    pub(super) fn project_id_for_document(&self, path: &Path) -> Option<u64> {
        std::iter::once(&self.project)
            .chain(self.parked.iter())
            .find(|project| project.documents.contains_key(path))
            .map(|project| project.id)
    }

    pub(super) fn reload_from_disk(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let task = cx
            .background_executor()
            .spawn(async move { (path.clone(), buffer::read(&path)) });
        cx.spawn_in(window, async move |this, cx| {
            let (path, result) = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(text) => {
                        this.apply_disk_text(&path, text, true, window, cx);
                        this.toast("Reloaded from disk", cx);
                    }
                    Err(error) => this.error(error.to_string(), cx),
                }
                this.update_title(window);
                cx.notify();
            });
        })
        .detach();
    }

    /// The last write, on the way out: the session as it stands, and no
    /// unsaved file at all — whatever was not saved was either saved or
    /// deliberately let go of at the prompt.
    pub(super) fn finish_session(&mut self) {
        let _ = session::Session::save(&self.collect_session(), &self.session_file);
        let _ = session::clear_unsaved(&self.unsaved_file);
    }

    pub(super) fn switch_project(
        &mut self,
        root: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.saving || self.prompting || self.project_loading {
            return;
        }
        if self
            .project
            .workspace
            .as_ref()
            .is_some_and(|w| w.root == root)
        {
            return;
        }
        let Some(index) = self
            .parked
            .iter()
            .position(|p| p.workspace.as_ref().is_some_and(|w| w.root == root))
        else {
            return;
        };
        self.reset();
        let project = self.parked.remove(index);
        let previous = std::mem::replace(&mut self.project, project);
        if previous.workspace.is_some() {
            self.parked.push(previous);
        }
        self.rebuild_rows();
        self.update_title(window);
        // The dock follows the project it is open on.
        if self.terminal_open {
            self.ensure_terminal(cx);
        }
        if let Some(doc) = self
            .project
            .active()
            .and_then(|p| self.project.documents.get(p))
        {
            doc.editor.focus_handle(cx).focus(window, cx);
        } else {
            self.tree_focus.focus(window, cx);
        }
        self.refresh_git(cx);
        cx.notify();
    }

    /// Make an empty folder and open it. Fails rather than reuse a name that
    /// is already on disk — that is Open, not New.
    pub(super) fn create_empty_project(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(parent) = path.parent().map(Path::to_path_buf) else {
            self.error("Choose a folder to put the project in", cx);
            return;
        };
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            self.error("Name cannot be empty", cx);
            return;
        };
        let task = cx
            .background_executor()
            .spawn(async move { fs_op::create_dir(&parent, &name) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(path) => this.open_project(path, window, cx),
                Err(e) => this.error(e.to_string(), cx),
            });
        })
        .detach();
    }

    pub(super) fn show_clone_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = None;
        self.clone_open = true;
        self.clone_url.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn close_clone_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clone_open = false;
        self.tree_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn confirm_clone_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.clone_open || self.prompting || self.project_loading || self.cloning {
            return;
        }
        match git::parse_github(&self.clone_url.read(cx).value()) {
            Ok(repo) => {
                self.clone_open = false;
                self.pick_clone_destination(repo, window, cx);
            }
            Err(e) => self.error(e, cx),
        }
    }

    pub(super) fn pick_clone_destination(
        &mut self,
        repo: git::GithubRepo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A folder picker: the repo lands in `chosen/name`. The name comes
        // from the URL, not a second prompt.
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Clone Project".into()),
        });
        self.prompting = true;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = picker.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.prompting = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(parent) = paths.into_iter().next() {
                            this.start_clone(repo.url, parent.join(&repo.name), window, cx);
                        }
                    }
                    Ok(Err(e)) => this.error(e.to_string(), cx),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Clone into `dest` on a background thread, then open it. `url` is
    /// whatever `git clone` accepts — GitHub after the prompt, a local
    /// path in tests.
    pub(super) fn start_clone(
        &mut self,
        url: String,
        dest: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_loading || self.saving || self.prompting || self.cloning {
            return;
        }
        self.cloning = true;
        self.project_loading = true;
        self.status("Cloning", "Copying the repository onto this computer.", cx);
        let task = cx
            .background_executor()
            .spawn(async move { git::clone(&url, &dest) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.cloning = false;
                this.project_loading = false;
                match result {
                    Ok(path) => {
                        this.clear_notice();
                        this.open_project(path, window, cx);
                    }
                    Err(e) => this.error(e.to_string(), cx),
                }
            });
        })
        .detach();
    }

    pub(super) fn open_project(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_loading || self.saving || self.prompting {
            return;
        }
        self.open_request += 1;
        self.project_loading = true;
        self.loading = true;
        self.clear_notice();
        cx.notify();
        let generation = self.generation;
        let ignored = self.settings.ignored.clone();
        let started = Instant::now();
        let task = cx.background_executor().spawn(async move {
            let workspace = Workspace::open(&path)?;
            let children = tree::children_in(&workspace.root, &workspace.root, &ignored)?;
            Ok::<_, std::io::Error>((workspace, children))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                this.project_loading = false;
                cx.notify();
                match result {
                    Ok((workspace, children)) => {
                        if perf_on() {
                            eprintln!(
                                "folio_open_tree_ms={:.1} children={} rss_mb={:.1}",
                                perf_ms(started),
                                children.len(),
                                rss_mb().unwrap_or(f64::NAN)
                            );
                        }
                        if this
                            .project
                            .workspace
                            .as_ref()
                            .is_some_and(|w| w.root == workspace.root)
                        {
                            return;
                        }
                        if this.parked.iter().any(|p| {
                            p.workspace
                                .as_ref()
                                .is_some_and(|w| w.root == workspace.root)
                        }) {
                            this.switch_project(&workspace.root, window, cx);
                            return;
                        }
                        this.reset();
                        let previous = std::mem::replace(
                            &mut this.project,
                            Project {
                                id: this.generation,
                                ..Default::default()
                            },
                        );
                        if previous.workspace.is_some() {
                            this.parked.push(previous);
                        }
                        this.project_order.push(workspace.root.clone());
                        this.project
                            .directories
                            .insert(workspace.root.clone(), children);
                        this.project.expanded.insert(workspace.root.clone());
                        window.set_window_title(&name(&workspace.root));
                        this.project.workspace = Some(workspace);
                        this.rebuild_rows();
                        this.refresh_project(cx);
                        if this.terminal_open {
                            this.ensure_terminal(cx);
                        }
                        this.tree_focus.focus(window, cx);
                        this.sync_disk_watch();
                    }
                    Err(e) => this.error(format!("Could not open the project: {e}"), cx),
                }
                cx.notify();
                // Nothing left to put back is the common case: this is the same
                // path a project the user opened takes.
                this.next_restore(window, cx);
            });
        })
        .detach();
    }

    pub(super) fn refresh_project(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        self.refresh_recent(RecentAction::Open(root), cx);
        self.reindex(cx);
        self.refresh_git(cx);
    }

    /// Rebuild the flat index that quick-open and project search read.
    pub(super) fn reindex(&mut self, cx: &mut Context<Self>) {
        self.reindex_project(self.project.id, cx);
    }

    pub(super) fn reindex_project(&mut self, project_id: u64, cx: &mut Context<Self>) {
        let Some(root) = self.project_ref(project_id).and_then(|project| {
            project
                .workspace
                .as_ref()
                .map(|workspace| workspace.root.clone())
        }) else {
            return;
        };
        if let Some(project) = self.project_mut(project_id) {
            if project.indexing {
                project.index_again = true;
                return;
            }
            project.indexing = true;
        }
        let ignored = self.settings.ignored.clone();
        let started = Instant::now();
        let task = cx
            .background_executor()
            .spawn(async move { tree::index(&root, &ignored) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(project) = this.project_mut(project_id) else {
                    return;
                };
                project.indexing = false;
                if std::mem::take(&mut project.index_again) {
                    this.reindex_project(project_id, cx);
                    return;
                }
                match result {
                    Ok(files) => {
                        if perf_on() {
                            eprintln!(
                                "folio_index_ms={:.1} files={} rss_mb={:.1}",
                                perf_ms(started),
                                files.len(),
                                rss_mb().unwrap_or(f64::NAN)
                            );
                        }
                        project.set_files(files);
                        if this.project.id == project_id {
                            this.filter(cx);
                        }
                    }
                    Err(e) => this.error(e.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn refresh_git(&mut self, cx: &mut Context<Self>) {
        self.refresh_git_project(self.project.id, cx);
    }

    pub(super) fn refresh_git_project(&mut self, project_id: u64, cx: &mut Context<Self>) {
        let Some(root) = self.project_ref(project_id).and_then(|project| {
            project
                .workspace
                .as_ref()
                .map(|workspace| workspace.root.clone())
        }) else {
            return;
        };
        if let Some(project) = self.project_mut(project_id) {
            if project.git_running {
                project.git_again = true;
                return;
            }
            project.git_running = true;
        }
        let task = cx
            .background_executor()
            .spawn(async move { git::status(&root) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(project) = this.project_mut(project_id) else {
                    return;
                };
                project.git_running = false;
                if std::mem::take(&mut project.git_again) {
                    this.refresh_git_project(project_id, cx);
                    return;
                }
                match result {
                    Ok(status) => project.git_status = status,
                    Err(e) => this.error(format!("Could not read git status: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn rebuild_rows(&mut self) {
        self.tree_hover_row = None;
        pub(super) fn flatten(
            dir: &Path,
            depth: usize,
            cache: &HashMap<PathBuf, Vec<Entry>>,
            expanded: &HashSet<PathBuf>,
            rows: &mut Vec<Row>,
        ) {
            if let Some(entries) = cache.get(dir) {
                for entry in entries {
                    rows.push(Row {
                        entry: entry.clone(),
                        depth,
                    });
                    if expanded.contains(&entry.path) {
                        flatten(&entry.path, depth + 1, cache, expanded, rows);
                    }
                }
            }
        }
        self.project.rows.clear();
        if let Some(workspace) = &self.project.workspace
            && self.project.expanded.contains(&workspace.root)
        {
            flatten(
                &workspace.root,
                1,
                &self.project.directories,
                &self.project.expanded,
                &mut self.project.rows,
            );
        }
        self.project.selected_row = self
            .project
            .selected_row
            .min(self.project.rows.len().saturating_sub(1));

        // The row being named is not on disk under its new name yet, so it is
        // spliced in after the flattening pass: renaming takes over the entry's
        // own row, creating adds one in front of the folder's children.
        let edit = self.editing.as_ref().map(|edit| {
            (
                edit.parent.clone(),
                edit.kind,
                edit.renaming.clone(),
                edit.renaming.as_ref().and_then(|path| {
                    self.project
                        .rows
                        .iter()
                        .position(|row| &row.entry.path == path)
                }),
            )
        });
        if let Some((parent, kind, renaming, taken)) = edit {
            // Collapsing the folder took the renamed entry's row away; there is
            // nothing left to type into.
            if renaming.is_some() && taken.is_none() {
                self.editing = None;
                return;
            }
            let (index, depth) = match taken {
                Some(index) => (index, self.project.rows[index].depth),
                None => {
                    let parent_row = self
                        .project
                        .rows
                        .iter()
                        .position(|row| row.entry.path == parent);
                    match (parent_row, &self.project.workspace) {
                        (Some(index), _) => (index + 1, self.project.rows[index].depth + 1),
                        (None, Some(workspace)) if parent == workspace.root => (0, 1),
                        (None, _) => (self.project.rows.len(), 1),
                    }
                }
            };
            // A placeholder path that can never collide with a real entry: NUL
            // is illegal in a path on every platform we build for.
            let entry = Entry {
                path: renaming.unwrap_or_else(|| parent.join("\0")),
                name: String::new(),
                kind,
            };
            let row = Row { entry, depth };
            if taken.is_some() {
                self.project.rows[index] = row;
            } else {
                self.project.rows.insert(index, row);
            }
            if let Some(edit) = self.editing.as_mut() {
                edit.row = index;
            }
        }
    }

    pub(super) fn toggle_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.project.expanded.remove(&path) {
            if self.project.directories.contains_key(&path) {
                self.project.expanded.insert(path);
            } else {
                // Keep expansion intent while the read is pending; a late result only fills the cache.
                self.project.expanded.insert(path.clone());
                self.project.directories.insert(path.clone(), vec![]);
                let project_id = self.project.id;
                let dir = path.clone();
                let root = self
                    .project
                    .workspace
                    .as_ref()
                    .map(|workspace| workspace.root.clone())
                    .unwrap_or_else(|| dir.clone());
                let ignored = self.settings.ignored.clone();
                let task = cx
                    .background_executor()
                    .spawn(async move { tree::children_in(&root, &dir, &ignored) });
                cx.spawn(async move |this, cx| {
                    let result = task.await;
                    let _ = this.update(cx, |this, cx| {
                        let Some(project) = this.project_mut(project_id) else {
                            return;
                        };
                        match result {
                            Ok(children) => {
                                project.directories.insert(path.clone(), children);
                            }
                            Err(e) => {
                                project.directories.remove(&path);
                                project.expanded.remove(&path);

                                this.error(e.to_string(), cx);
                            }
                        }
                        if this.project.id == project_id {
                            this.rebuild_rows();
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        self.rebuild_rows();
        cx.notify();
    }

    /// Re-read one folder into the tree cache. Unlike `toggle_directory` this
    /// forces a fresh read, which is what a filesystem change needs.
    pub(super) fn reload_directory(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let Some((project_id, root)) = std::iter::once(&self.project)
            .chain(self.parked.iter())
            .find(|project| project.directories.contains_key(&dir))
            .and_then(|project| {
                project
                    .workspace
                    .as_ref()
                    .map(|workspace| (project.id, workspace.root.clone()))
            })
        else {
            return;
        };
        let read = dir.clone();
        let ignored = self.settings.ignored.clone();
        let task = cx
            .background_executor()
            .spawn(async move { tree::children_in(&root, &read, &ignored) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let error = match result {
                    Ok(children) => {
                        if let Some(project) = this.project_mut(project_id) {
                            project.directories.insert(dir.clone(), children);
                        }
                        None
                    }
                    Err(e) => Some(e.to_string()),
                };
                if this.project.id == project_id {
                    this.rebuild_rows();
                }
                if let Some(error) = error {
                    this.error(error, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Refresh everything a filesystem change invalidates: the folder that
    /// changed, the quick-open index, and the git dots.
    pub(super) fn rescan(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.reload_directory(dir, cx);
        self.reindex(cx);
        self.refresh_git(cx);
    }

    /// Run a launcher — Reveal, Open, Terminal — off the UI thread.
    pub(super) fn spawn_launch<F>(&mut self, work: F, cx: &mut Context<Self>)
    where
        F: FnOnce() -> io::Result<()> + Send + 'static,
    {
        let task = cx.background_executor().spawn(async move { work() });
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                let _ = this.update(cx, |this, cx| this.error(error.to_string(), cx));
            }
        })
        .detach();
    }

    /// Run a change to the filesystem off the UI thread. On success the folder
    /// that changed is re-read; on failure the tree is left alone. A newly
    /// created file is opened once it exists on disk.
    pub(super) fn spawn_change<F>(
        &mut self,
        dir: PathBuf,
        open_created: bool,
        work: F,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce() -> io::Result<PathBuf> + Send + 'static,
    {
        let task = cx.background_executor().spawn(async move { work() });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(created) => {
                        this.quiet_write(&created);
                        this.rescan(dir, cx);
                        if open_created {
                            this.open_file(created, window, cx);
                        }
                    }
                    Err(error) => this.error(error.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Rename off the UI thread. `spawn_change`'s shape is not enough here:
    /// the caches that key off the old path have to move with the entry.
    pub(super) fn spawn_rename(
        &mut self,
        from: PathBuf,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = from.clone();
        let task = cx
            .background_executor()
            .spawn(async move { fs_op::rename(&source, &name) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(renamed) => {
                        this.quiet_write(&renamed);
                        this.remap_paths(&from, &renamed);
                        this.rebuild_rows();
                        let dir = renamed
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|| renamed.clone());
                        this.rescan(dir, cx);
                        this.update_title(window);
                    }
                    Err(error) => this.error(error.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// How many open buffers under `path` hold edits that are not on disk.
    pub(super) fn unsaved_under(&self, path: &Path) -> usize {
        self.project
            .documents
            .iter()
            .filter(|(key, document)| key.starts_with(path) && document.dirty)
            .count()
    }

    /// Deleting is irreversible, so it always asks first. Trashing is
    /// recoverable and only asks when it would drop unsaved edits.
    pub(super) fn confirm_removal(
        &mut self,
        path: PathBuf,
        trashed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.saving || self.prompting || self.project_loading {
            return;
        }
        let name = name(&path);
        let unsaved = self.unsaved_under(&path);
        let detail = match (trashed, unsaved) {
            (true, unsaved) => {
                let bin = if cfg!(target_os = "windows") {
                    "Recycle Bin"
                } else {
                    "Trash"
                };
                format!(
                    "{name} and its contents will move to the {bin}. {unsaved} files have unsaved changes."
                )
            }
            (false, 0) => format!(
                "{name} and its contents will be deleted permanently. This cannot be undone."
            ),
            (false, unsaved) => format!(
                "{name} and its contents will be deleted permanently. This cannot be undone. {unsaved} files have unsaved changes."
            ),
        };
        let confirm = if !trashed {
            "Delete"
        } else if cfg!(target_os = "windows") {
            "Move to Recycle Bin"
        } else {
            "Move to Trash"
        };
        self.prompting = true;
        cx.notify();
        let answer = window.prompt(
            PromptLevel::Warning,
            confirm,
            Some(&detail),
            &[confirm, "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await.unwrap_or(1);
            let _ = this.update_in(cx, |this, window, cx| {
                this.prompting = false;
                if answer == 0 {
                    this.spawn_removal(path, trashed, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Remove an entry off the UI thread, then drop everything that pointed at
    /// it. A buffer for a file that no longer exists cannot stay open.
    pub(super) fn spawn_removal(
        &mut self,
        path: PathBuf,
        trashed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = path.clone();
        let task = cx.background_executor().spawn(async move {
            if trashed {
                fs_op::trash(&target)
            } else {
                fs_op::delete(&target)
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(()) => {
                        let dir = path
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|| path.clone());
                        this.forget_paths(&path);
                        this.rescan(dir, cx);
                        this.update_title(window);
                        let verb = if trashed { "Moved to Trash" } else { "Deleted" };
                        this.toast(&format!("{verb} {}", name(&path)), cx);
                    }
                    Err(error) => this.error(error.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Forget everything that lived under `path`, which is no longer on disk.
    pub(super) fn forget_paths(&mut self, path: &Path) {
        self.project
            .documents
            .retain(|key, _| !key.starts_with(path));
        for pane in &mut self.project.panes {
            pane.tabs.retain(|tab| !tab.starts_with(path));
            if pane
                .active
                .as_ref()
                .is_some_and(|key| key.starts_with(path))
            {
                pane.active = None;
            }
        }
        self.prune_empty_panes();
        if self
            .project
            .image
            .as_ref()
            .is_some_and(|(key, _)| key.starts_with(path))
        {
            self.project.image = None;
        }
        self.project
            .recent_files
            .retain(|key| !key.starts_with(path));
        self.project.expanded.retain(|key| !key.starts_with(path));
        self.project
            .directories
            .retain(|key, _| !key.starts_with(path));
    }

    /// Note where the settings window was and persist it alongside the main
    /// window's geometry.
    pub(super) fn remember_settings_bounds(&mut self, bounds: Bounds<Pixels>) {
        self.settings_bounds = Some(bounds_values(bounds));
        self.save_window_state();
    }

    /// Persist both windows' geometry. Written whenever either window goes
    /// away, so the app never depends on quitting being the last thing to
    /// happen.
    pub(super) fn save_window_state(&self) {
        WindowState {
            main: Some(bounds_values(self.main_bounds)),
            settings: self.settings_bounds,
        }
        .save(&self.window_file);
    }

    /// Re-key the state that pointed at `from`, now that it lives at `to`. A
    /// renamed folder takes its whole subtree with it.
    pub(super) fn remap_paths(&mut self, from: &Path, to: &Path) {
        remap_project_paths(&mut self.project, from, to);
        for project in &mut self.parked {
            remap_project_paths(project, from, to);
        }
        self.settle_documents();
    }

    /// After a move, a buffer may now live under another project's root.
    pub(super) fn settle_documents(&mut self) {
        let mut orphans = Vec::new();
        for project in std::iter::once(&mut self.project).chain(self.parked.iter_mut()) {
            let Some(root) = project
                .workspace
                .as_ref()
                .map(|workspace| workspace.root.clone())
            else {
                continue;
            };
            let documents = std::mem::take(&mut project.documents);
            for (path, document) in documents {
                if path.starts_with(&root) {
                    project.documents.insert(path, document);
                } else {
                    for pane in &mut project.panes {
                        pane.tabs.retain(|tab| tab != &path);
                        if pane.active.as_ref() == Some(&path) {
                            pane.active = None;
                        }
                    }
                    project.pinned.remove(&path);
                    project.read_only.remove(&path);
                    project.disk_notice.remove(&path);
                    orphans.push((path, document));
                }
            }
        }
        for (path, mut document) in orphans {
            // A new root can mean a different Cargo.toml above the file.
            document.can_run = None;
            let home = std::iter::once(&mut self.project)
                .chain(self.parked.iter_mut())
                .find(|project| {
                    project
                        .workspace
                        .as_ref()
                        .is_some_and(|workspace| path.starts_with(&workspace.root))
                });
            if let Some(project) = home {
                if project.pane_index_of(&path).is_none() {
                    project.pane_mut().tabs.push(path.clone());
                }
                project.documents.insert(path, document);
            }
        }
    }

    pub(super) fn drop_tree_entry(
        &mut self,
        source: PathBuf,
        onto: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if source == onto {
            return;
        }
        let into = if self.is_tree_directory(&onto) {
            onto
        } else {
            match onto.parent() {
                Some(parent) => parent.to_path_buf(),
                None => return,
            }
        };
        if source.parent() == Some(into.as_path()) {
            return;
        }
        if source.is_dir() && into.starts_with(&source) {
            self.error("Cannot move a folder into itself", cx);
            return;
        }
        self.move_entry(source, into, window, cx);
    }

    pub(super) fn move_entry(
        &mut self,
        source: PathBuf,
        into: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let from_parent = source.parent().map(Path::to_path_buf);
        let dest_dir = into.clone();
        let moving = source.clone();
        let task = cx
            .background_executor()
            .spawn(async move { fs_op::paste(&moving, &into, true) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(dest) => {
                        if this
                            .clipboard
                            .as_ref()
                            .is_some_and(|entry| entry.cut && entry.path == source)
                        {
                            this.clipboard = None;
                        }
                        this.quiet_write(&dest);
                        this.remap_paths(&source, &dest);
                        this.rebuild_rows();
                        if let Some(parent) = from_parent {
                            this.reload_directory(parent, cx);
                        }
                        this.rescan(dest_dir, cx);
                        this.update_title(window);
                        this.toast(&format!("Moved {}", name(&dest)), cx);
                    }
                    Err(error) => this.error(error.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
