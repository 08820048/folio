use super::*;

impl Folio {
    /// Keep an open changes view pointed at whatever file is active now, so a
    /// review can walk the tree instead of reopening the view per file.
    pub(super) fn follow_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.diff.is_some() {
            self.load_diff(window, cx);
            // The list is how you walk files; keep the keyboard there.
            if self.sidebar && self.sidebar_kind == SidebarKind::Changes {
                self.tree_focus.focus(window, cx);
            }
        }
    }

    /// Open the folders above a file in the tree and put the cursor on it, so
    /// the panel shows where the file lives.
    /// `⇧⌘D`: show the active file as its changes against HEAD, or go back to
    /// reading it.
    pub(super) fn toggle_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.diff.take().is_some() {
            self.focus_editor(window, cx);
            cx.notify();
            return;
        }
        // The tree keeps the keyboard: the diff is read-only, and picking the
        // next file moves the view on to its changes.
        self.tree_focus.focus(window, cx);
        self.load_diff(window, cx);
    }

    /// Read the active file's changes. Called when the view is turned on and
    /// again whenever the file it is showing changes, so it follows a review
    /// rather than having to be reopened per file.
    pub(super) fn load_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(path), Some(workspace)) = (
            self.project.active().cloned(),
            self.project.workspace.clone(),
        ) else {
            self.project.diff = None;
            cx.notify();
            return;
        };
        let find_branch = self.diff_against == DiffAgainst::Branch && self.diff_branch.is_none();
        let base = self.diff_base().to_string();
        let project_id = self.project.id;
        self.project.diff_request += 1;
        let request = self.project.diff_request;
        self.project.diff = Some(DiffView {
            path: path.clone(),
            loading: true,
            request,
            ..DiffView::default()
        });
        cx.notify();
        let task = cx.background_executor().spawn(async move {
            let branch = find_branch
                .then(|| git::review_branch(&workspace.root))
                .flatten();
            let base = branch.as_deref().unwrap_or(&base);
            let diff = diff::for_file_against(&workspace.root, &path, base)?;
            let new_text = buffer::read(&path).ok().map(Arc::<str>::from);
            let old_text = git::show_file(&workspace.root, base, &path)?.map(Arc::<str>::from);
            Ok::<_, io::Error>((diff, old_text, new_text, branch))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, _, cx| {
                let Some(project) = this.project_mut(project_id) else {
                    return;
                };
                let Some(view) = project.diff.as_mut() else {
                    return;
                };
                // A slower answer for a file the user has already left is not
                // the answer to show.
                if view.request != request {
                    return;
                }
                view.loading = false;
                match result {
                    Ok((diff, old_text, new_text, branch)) => {
                        view.split = diff::split_rows(&diff.lines);
                        view.lines = diff.lines;
                        view.untracked = diff.untracked;
                        view.old_text = old_text;
                        view.new_text = new_text;
                        view.error = None;
                        view.hunk = 0;
                        if this.project.id == project_id
                            && let Some(branch) = branch
                        {
                            this.diff_branch = Some(branch);
                        }
                    }
                    Err(error) => view.error = Some(error.to_string()),
                }
                this.recolor_diff(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Paint the open diff again after the highlight theme changes.
    pub(super) fn recolor_diff(&mut self, cx: &mut Context<Self>) {
        let theme = cx.theme().highlight_theme.clone();
        for project in std::iter::once(&mut self.project).chain(self.parked.iter_mut()) {
            let Some(view) = project.diff.as_mut().filter(|view| !view.loading) else {
                continue;
            };
            let project_id = project.id;
            let request = view.request;
            view.color_request += 1;
            let color_request = view.color_request;
            let path = view.path.clone();
            let old = view.old_text.clone();
            let new = view.new_text.clone();
            let theme = theme.clone();
            let task = cx.background_executor().spawn(async move {
                let old = old
                    .as_deref()
                    .map(|text| syntax::line_styles(&path, text, &theme))
                    .unwrap_or_default();
                let new = new
                    .as_deref()
                    .map(|text| syntax::line_styles(&path, text, &theme))
                    .unwrap_or_default();
                (old, new)
            });
            cx.spawn(async move |this, cx| {
                let (old, new) = task.await;
                let _ = this.update(cx, |this, cx| {
                    let Some(view) = this
                        .project_mut(project_id)
                        .and_then(|project| project.diff.as_mut())
                    else {
                        return;
                    };
                    if view.request != request || view.color_request != color_request {
                        return;
                    }
                    view.old_styles = old;
                    view.new_styles = new;
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Current project's dirty / untracked files, as absolute paths.
    pub(super) fn changed_files(&self) -> Vec<(PathBuf, char)> {
        let Some(root) = self.project.workspace.as_ref().map(|w| &w.root) else {
            return Vec::new();
        };
        git::changed_paths(&self.project.git_status)
            .into_iter()
            .map(|(relative, kind)| (root.join(relative), kind))
            .collect()
    }

    /// Mark a file as read in the changes view, or take the mark back. This is
    /// view state: nothing on disk is touched and it lives in the session.
    pub(super) fn toggle_reviewed(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !self.project.reviewed.remove(path) {
            self.project.reviewed.insert(path.to_path_buf());
        }
        cx.notify();
    }

    pub(super) fn open_changed_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self
            .changed_files()
            .iter()
            .position(|(entry, _)| entry == &path)
        {
            self.changes_selected = index;
        }
        // `open_file` is often async. Plant the view first so `follow_diff`
        // loads this file's changes when the buffer lands; calling
        // `toggle_diff` now would see no active file and turn the view off.
        if self.project.diff.is_none() {
            self.project.diff = Some(DiffView {
                loading: true,
                ..DiffView::default()
            });
        }
        self.open_file(path, window, cx);
    }

    pub(super) fn move_change_file(
        &mut self,
        next: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let files = self.changed_files();
        if files.is_empty() {
            return;
        }
        let last = files.len() - 1;
        self.changes_selected = if next {
            (self.changes_selected + 1).min(last)
        } else {
            self.changes_selected.saturating_sub(1)
        };
        let path = files[self.changes_selected].0.clone();
        self.open_changed_file(path, window, cx);
    }

    pub(super) fn move_hunk(&mut self, next: bool, cx: &mut Context<Self>) {
        let Some(view) = self.project.diff.as_mut() else {
            return;
        };
        let hunks = if self.diff_layout == DiffLayout::Split {
            view.split
                .iter()
                .enumerate()
                .filter(|(_, row)| row.hunk.is_some())
                .map(|(index, _)| index)
                .collect::<Vec<_>>()
        } else {
            diff::hunk_indices(&view.lines)
        };
        if hunks.is_empty() {
            return;
        }
        if next {
            view.hunk = (view.hunk + 1).min(hunks.len() - 1);
        } else {
            view.hunk = view.hunk.saturating_sub(1);
        }
        view.scroll
            .scroll_to_item(hunks[view.hunk], ScrollStrategy::Top);
        cx.notify();
    }

    pub(super) fn set_diff_layout(&mut self, layout: DiffLayout, cx: &mut Context<Self>) {
        self.diff_layout = layout;
        cx.notify();
    }

    pub(super) fn set_diff_against(
        &mut self,
        against: DiffAgainst,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.diff_against = against;
        if against == DiffAgainst::Branch {
            self.diff_branch = None;
        }
        if self.project.diff.is_some() {
            self.load_diff(window, cx);
        }
        cx.notify();
    }

    pub(super) fn diff_base(&self) -> &str {
        match self.diff_against {
            DiffAgainst::Head => "HEAD",
            DiffAgainst::Branch => self.diff_branch.as_deref().unwrap_or("HEAD"),
        }
    }
}
