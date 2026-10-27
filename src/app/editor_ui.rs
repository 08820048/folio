use super::*;

impl Folio {
    /// Enter and Escape for the field, shared by naming and renaming.
    pub(super) fn subscribe_edit(
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(
            input,
            window,
            move |this: &mut Self, input, event: &InputEvent, window, cx| {
                if this
                    .editing
                    .as_ref()
                    .is_none_or(|edit| &edit.input != input)
                {
                    return;
                }
                match event {
                    InputEvent::PressEnter { .. } => this.commit_edit(window, cx),
                    // A row that was never named should not linger once the
                    // pointer moves on.
                    InputEvent::Blur => this.cancel_create(window, cx),
                    _ => {}
                }
            },
        )
    }

    /// Keep `Document::dirty` in step with the buffer it belongs to. The
    /// document is found by editor rather than by path, so renaming an open
    /// file does not have to tear the subscription down and build it again —
    /// which would be a trap, because a `Subscription` unsubscribes whatever
    /// currently sits at its key when it is dropped.
    pub(super) fn subscribe_document(
        editor: &Entity<EditorState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let editor = editor.clone();
        cx.subscribe_in(
            &editor,
            window,
            move |this, editor, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    if let Some(doc) = std::iter::once(&mut this.project)
                        .chain(this.parked.iter_mut())
                        .flat_map(|project| project.documents.values_mut())
                        .find(|doc| &doc.editor == editor)
                    {
                        doc.dirty = editor.read(cx).is_modified(&doc.saved);
                        doc.revision = doc.revision.wrapping_add(1);
                    }
                    this.refresh_brackets(editor, cx);
                    this.refresh_runnables(editor, cx);
                    this.update_title(window);
                    cx.notify();
                }
            },
        )
    }

    pub(super) fn attach_brackets(
        editor: &Entity<EditorState>,
        cx: &mut Context<Self>,
    ) -> BracketPaint {
        let rainbow = editor.update(cx, |state, cx| {
            state.create_decorations_collection(Vec::new(), cx)
        });
        BracketPaint {
            rainbow,
            last_rainbow: Vec::new(),
        }
    }

    pub(super) fn refresh_brackets(
        &mut self,
        editor: &Entity<EditorState>,
        cx: &mut Context<Self>,
    ) {
        let dark = cx.theme().is_dark();
        let Some(doc) = std::iter::once(&mut self.project)
            .chain(self.parked.iter_mut())
            .flat_map(|project| project.documents.values_mut())
            .find(|doc| &doc.editor == editor)
        else {
            return;
        };
        if doc.large {
            editor.update(cx, |state, cx| state.set_bracket_pairs(Vec::new(), cx));
            return;
        }
        let pairs = rope_pairs(&editor.read(cx).snapshot());
        let rainbow: Vec<TextDecoration> = pairs
            .iter()
            .flat_map(|pair| {
                let style = HighlightStyle {
                    color: Some(bracket_color(pair.depth, dark)),
                    ..Default::default()
                };
                [
                    TextDecoration::new(pair.open..pair.open + 1, style),
                    TextDecoration::new(pair.close..pair.close + 1, style),
                ]
            })
            .collect();
        let offsets: Vec<(usize, usize)> =
            pairs.iter().map(|pair| (pair.open, pair.close)).collect();
        if rainbow != doc.brackets.last_rainbow {
            doc.brackets.last_rainbow.clone_from(&rainbow);
            doc.brackets.rainbow.set(rainbow, cx);
        }
        editor.update(cx, |state, cx| state.set_bracket_pairs(offsets, cx));
    }

    pub(super) fn can_run_active(&self, _: &App) -> bool {
        self.project
            .active()
            .and_then(|path| self.project.documents.get(path))
            .is_some_and(|doc| doc.can_run == Some(true))
    }

    pub(super) fn set_document_can_run(&mut self, editor: &Entity<EditorState>, can_run: bool) {
        if let Some(doc) = std::iter::once(&mut self.project)
            .chain(self.parked.iter_mut())
            .flat_map(|project| project.documents.values_mut())
            .find(|doc| &doc.editor == editor)
        {
            doc.can_run = Some(can_run);
        }
    }

    pub(super) fn refresh_runnables(
        &mut self,
        editor: &Entity<EditorState>,
        cx: &mut Context<Self>,
    ) {
        let Some((path, large, cached, root)) = std::iter::once(&self.project)
            .chain(self.parked.iter())
            .flat_map(|project| {
                let root = project.workspace.as_ref().map(|w| w.root.clone());
                project
                    .documents
                    .iter()
                    .filter(|(_, doc)| &doc.editor == editor)
                    .map(move |(path, doc)| (path.clone(), doc.large, doc.can_run, root.clone()))
            })
            .next()
        else {
            return;
        };
        if large || root.is_none() {
            self.set_document_can_run(editor, false);
            editor.update(cx, |state, cx| {
                state.set_runnables(Vec::new(), None, None, cx)
            });
            return;
        }
        let root = root.unwrap();
        let can_run = cached.unwrap_or_else(|| run::can_run(&root, &path));
        if cached.is_none() {
            self.set_document_can_run(editor, can_run);
        }
        if !can_run {
            editor.update(cx, |state, cx| {
                state.set_runnables(Vec::new(), None, None, cx)
            });
            return;
        }
        let text = editor.read(cx).snapshot().to_string();
        let mut lines: Vec<usize> = run::find(&path, &text)
            .iter()
            .map(|runnable| runnable.line)
            .collect();
        lines.sort_unstable();
        lines.dedup();
        let folio = cx.entity().downgrade();
        let on_click = Rc::new({
            let folio = folio.clone();
            move |line: usize, window: &mut Window, cx: &mut App| {
                let _ = folio.update(cx, |this, cx| this.run_or_stop(Some(line), window, cx));
            }
        });
        let icon = Rc::new(move |cx: &App| {
            let busy = folio
                .upgrade()
                .is_some_and(|this| this.read(cx).run_is_busy(cx));
            if busy {
                Icon::new(IconName::Pause)
                    .xsmall()
                    .text_color(run_stop_color(cx))
                    .into_any_element()
            } else {
                Icon::new(IconName::Play)
                    .xsmall()
                    .text_color(cx.theme().success)
                    .into_any_element()
            }
        });
        editor.update(cx, |state, cx| {
            state.set_runnables(lines, Some(on_click), Some(icon), cx)
        });
    }

    pub(super) fn caret_line(&self, cx: &App) -> Option<usize> {
        let path = self.project.active()?;
        let doc = self.project.documents.get(path)?;
        let offset = doc
            .editor
            .read(cx)
            .base_state()
            .read(cx)
            .selected_ranges()
            .last()?
            .end;
        Some(doc.editor.read(cx).snapshot().offset_to_point(offset).row)
    }

    pub(super) fn run_is_busy(&self, cx: &App) -> bool {
        self.run_terminal(cx)
            .is_some_and(|view| view.read(cx).is_busy())
    }

    pub(super) fn run_terminal_index(&self, cx: &App) -> Option<usize> {
        let root = self.project.workspace.as_ref()?.root.clone();
        self.terminals
            .get(&root)?
            .views
            .iter()
            .position(|view| view.read(cx).is_run())
    }

    pub(super) fn run_terminal(&self, cx: &App) -> Option<&Entity<TerminalView>> {
        let index = self.run_terminal_index(cx)?;
        let root = self.project.workspace.as_ref()?.root.clone();
        self.terminals.get(&root)?.views.get(index)
    }

    pub(super) fn run_or_stop(
        &mut self,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.run_is_busy(cx) {
            self.stop_run(window, cx);
            return;
        }
        self.run_code(line, window, cx);
    }

    /// Stop the Run tab's command, escalating with each press. Nothing to
    /// stop leaves the drawer as it was.
    pub(super) fn stop_run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.run_terminal_index(cx) else {
            return;
        };
        let Some(view) = self.run_terminal(cx).cloned() else {
            return;
        };
        if !view.read(cx).is_busy() {
            return;
        }
        self.terminal_open = true;
        if let Some(tabs) = self
            .project
            .workspace
            .as_ref()
            .map(|w| w.root.clone())
            .and_then(|root| self.terminals.get_mut(&root))
        {
            tabs.active = index;
        }
        let step = view.update(cx, |view, cx| {
            view.focus_handle().focus(window, cx);
            view.stop()
        });
        match step {
            StopStep::Interrupt => {}
            StopStep::Terminate => self.warn("Still running — sent SIGTERM", cx),
            StopStep::Kill => self.warn("Still running — sent SIGKILL", cx),
        }
        cx.notify();
    }

    pub(super) fn focus_run_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal_open = true;
        if let Some(index) = self.run_terminal_index(cx)
            && let Some(tabs) = self
                .project
                .workspace
                .as_ref()
                .map(|w| w.root.clone())
                .and_then(|root| self.terminals.get_mut(&root))
        {
            tabs.active = index;
        }
        if let Some(view) = self.run_terminal(cx).cloned() {
            view.update(cx, |view, cx| view.focus_handle().focus(window, cx));
        }
        cx.notify();
    }

    pub(super) fn run_code(
        &mut self,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let line = line.or_else(|| self.caret_line(cx));
        let Some(path) = self.project.active().cloned() else {
            self.warn("Open a file to run", cx);
            return;
        };
        if self
            .project
            .documents
            .get(&path)
            .is_some_and(|doc| doc.dirty)
        {
            self.run_after_save = Some(line);
            self.save_documents(None, window, cx);
            return;
        }
        self.start_run(line, window, cx);
    }

    pub(super) fn rerun(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.run_is_busy(cx) {
            self.focus_run_terminal(window, cx);
            self.warn("Already running — stop it first", cx);
            return;
        }
        let last = self
            .project
            .workspace
            .as_ref()
            .and_then(|workspace| self.terminals.get(&workspace.root))
            .and_then(|tabs| tabs.last_run.clone());
        if let Some(command) = last {
            self.start_run_command(command, window, cx);
        } else {
            self.run_code(None, window, cx);
        }
    }

    pub(super) fn start_run(
        &mut self,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        let Some(path) = self.project.active().cloned() else {
            self.warn("Open a file to run", cx);
            return;
        };
        let Some(doc) = self.project.documents.get(&path) else {
            return;
        };
        let text = doc.editor.read(cx).snapshot().to_string();
        let Some(command) = run::resolve(&root, &path, &text, line) else {
            self.warn("Nothing to run in this file", cx);
            return;
        };
        self.start_run_command(command, window, cx);
    }

    /// Start `command` in the project's Run tab, making one if there is
    /// none. A tab whose command is still running is left alone.
    pub(super) fn start_run_command(
        &mut self,
        command: run::Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        self.terminal_open = true;
        if self.run_is_busy(cx) {
            self.focus_run_terminal(window, cx);
            self.warn("Already running — stop it first", cx);
            return;
        }
        if let Some(index) = self.run_terminal_index(cx) {
            let Some(view) = self.run_terminal(cx).cloned() else {
                return;
            };
            view.update(cx, |view, cx| {
                view.respawn_command(&command.cwd, &command.shell, cx);
                view.focus_handle().focus(window, cx);
            });
            if let Some(tabs) = self.terminals.get_mut(&root) {
                tabs.last_run = Some(command);
                tabs.active = index;
            }
            cx.notify();
            return;
        }
        let view = cx.new(|cx| TerminalView::new_run(&command.cwd, &command.shell, 80, 24, cx));
        let watch = Self::watch_terminal(&view, cx);
        view.update(cx, |view, cx| view.focus_handle().focus(window, cx));
        let tabs = self.terminals.entry(root).or_insert(TerminalTabs {
            views: vec![],
            active: 0,
            last_run: None,
            _watches: vec![],
        });
        tabs.last_run = Some(command);
        tabs.views.push(view);
        tabs._watches.push(watch);
        tabs.active = tabs.views.len() - 1;
        cx.notify();
    }

    pub(super) fn refresh_all_brackets(&mut self, cx: &mut Context<Self>) {
        let editors: Vec<_> = std::iter::once(&self.project)
            .chain(self.parked.iter())
            .flat_map(|project| project.documents.values().map(|doc| doc.editor.clone()))
            .collect();
        for editor in editors {
            self.refresh_brackets(&editor, cx);
        }
    }

    pub(super) fn cancel_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.take().is_none() {
            return;
        }
        self.rebuild_rows();
        self.tree_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.editing else {
            return;
        };
        let name = edit.input.read(cx).value().to_string();
        let (parent, kind, renaming) = (edit.parent.clone(), edit.kind, edit.renaming.clone());
        self.editing = None;
        self.rebuild_rows();
        self.tree_focus.focus(window, cx);
        match renaming {
            Some(source) => self.spawn_rename(source, name, window, cx),
            None => {
                let dir = parent.clone();
                let create = move || match kind {
                    EntryKind::Directory => fs_op::create_dir(&dir, &name),
                    EntryKind::File => fs_op::create_file(&dir, &name),
                };
                self.spawn_change(parent, kind == EntryKind::File, create, window, cx);
            }
        }
    }

    /// The font code is drawn in, falling back to the one in the binary.
    pub(super) fn code_font(&self) -> SharedString {
        self.settings
            .code_font_family
            .clone()
            .unwrap_or_else(|| "JetBrains Mono".to_string())
            .into()
    }

    /// Code face plus the CJK stack. The editor is its own view, so this has
    /// to be set on the input state rather than inherited from the window.
    pub(super) fn code_typeface(&self) -> Font {
        Font {
            family: self.code_font(),
            fallbacks: Some(FontFallbacks::from_fonts(
                CJK_FALLBACKS.iter().map(|name| name.to_string()).collect(),
            )),
            ..Default::default()
        }
    }

    pub(super) fn reveal_in_tree(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        if !path.starts_with(&root) {
            return;
        }
        // Every folder between the root and the file has to be open for its
        // row to exist at all.
        let mut folders: Vec<PathBuf> = path
            .parent()
            .into_iter()
            .flat_map(|parent| parent.ancestors())
            .take_while(|dir| dir.starts_with(&root))
            .map(Path::to_path_buf)
            .collect();
        folders.reverse();
        for folder in &folders {
            self.project.expanded.insert(folder.clone());
        }
        let project_id = self.project.id;
        let root = self
            .project
            .workspace
            .as_ref()
            .map(|workspace| workspace.root.clone());
        let ignored = self.settings.ignored.clone();
        // Every folder on the way down is re-read rather than only the ones
        // the tree has never opened: a folder added since its parent was last
        // read is not in that parent's listing, so the row would not exist even
        // though the folder itself would read fine.
        let task = cx.background_executor().spawn(async move {
            let mut read = Vec::new();
            for folder in folders {
                let listed = match &root {
                    Some(root) => tree::children_in(root, &folder, &ignored),
                    None => tree::children(&folder, &ignored),
                };
                if let Ok(children) = listed {
                    read.push((folder, children));
                }
            }
            read
        });
        cx.spawn_in(window, async move |this, cx| {
            let read = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(project) = this.project_mut(project_id) {
                    for (folder, children) in read {
                        project.directories.insert(folder, children);
                    }
                }
                if this.project.id == project_id {
                    this.select_row_for(&path, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Put the tree's cursor on a row and bring it into view.
    pub(super) fn select_row_for(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.rebuild_rows();
        if let Some(index) = self
            .project
            .rows
            .iter()
            .position(|row| row.entry.path == path)
        {
            self.project.selected_row = index;
            self.project
                .tree_scroll
                .scroll_to_item(index, ScrollStrategy::Center);
        }
        self.tree_focus.focus(window, cx);
        cx.notify();
    }

    /// Put the caret back on the code, or on the tree when nothing is open.
    pub(super) fn focus_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .project
            .active()
            .and_then(|path| self.project.documents.get(path))
        {
            doc.editor.focus_handle(cx).focus(window, cx);
        } else {
            self.tree_focus.focus(window, cx);
        }
    }

    /// True while an IME composition is marked in the editor, a search
    /// field, or the terminal. Escape has to cancel that first, not
    /// close a panel or abandon a name.
    pub(super) fn is_composing(&self, cx: &App) -> bool {
        if let Some(editor) = self.active_editor()
            && editor.read(cx).base_state().read(cx).is_composing()
        {
            return true;
        }
        if self
            .active_terminal()
            .is_some_and(|terminal| terminal.read(cx).composing())
        {
            return true;
        }
        self.query.read(cx).base_state().read(cx).is_composing()
            || self
                .search_query
                .read(cx)
                .base_state()
                .read(cx)
                .is_composing()
            || self
                .replace_query
                .read(cx)
                .base_state()
                .read(cx)
                .is_composing()
            || self.clone_url.read(cx).base_state().read(cx).is_composing()
    }

    pub(super) fn editor_is_editable(&self, cx: &App) -> bool {
        let Some(editor) = self.active_editor() else {
            return false;
        };
        editor
            .read(cx)
            .base_state()
            .read(cx)
            .context_menu_capabilities()
            .is_editable()
    }

    pub(super) fn editor_has_selection(&self, cx: &App) -> bool {
        let Some(editor) = self.active_editor() else {
            return false;
        };
        editor
            .read(cx)
            .base_state()
            .read(cx)
            .context_menu_capabilities()
            .has_selection()
    }

    /// The buffer being edited, if one is. An image preview has none.
    pub(super) fn active_editor(&self) -> Option<Entity<EditorState>> {
        let path = self.project.active()?;
        self.project
            .documents
            .get(path)
            .map(|document| document.editor.clone())
    }

    /// Run a command against the buffer being edited.
    ///
    /// The multi-cursor commands are the editor's own — it holds the
    /// selections, and the edit they make has to go through the same path a
    /// keystroke does — so this is only the reaching and the redrawing.
    pub(super) fn on_active_buffer(
        &mut self,
        cx: &mut Context<Self>,
        command: impl FnOnce(&mut InputBaseState, &mut Context<InputBaseState>),
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        base.update(cx, |base, cx| command(base, cx));
        cx.notify();
    }

    /// What an opening bracket does: a pair around the selection, or an empty
    /// pair with the caret between the two.
    ///
    /// Both characters go in at once, so an opener is one edit and one step on
    /// the undo stack rather than two.
    pub(super) fn open_pair(
        &mut self,
        open: &'static str,
        close: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        if self.type_at_every_cursor(&base, open, window, cx) {
            return;
        }
        let value = base.read(cx).value().to_string();
        let selection = base.read(cx).selected_range();
        let Some(selected) = value.get(selection.clone()) else {
            return;
        };
        // Wrapping a selection is always what was meant. An empty pair is not:
        // in front of a word the closer would land where the rest of the word
        // has to go, so the opener is typed on its own.
        if selected.is_empty() && !allows_autoclose(value[selection.start..].chars().next()) {
            base.update(cx, |base, cx| base.replace(open, window, cx));
            cx.notify();
            return;
        }
        // Wrapping leaves the caret after the pair; an empty pair leaves it
        // between the two.
        let caret = if selected.is_empty() {
            selection.start + open.len()
        } else {
            selection.start + open.len() + selected.len() + close.len()
        };
        let text = format!("{open}{selected}{close}");
        base.update(cx, |base, cx| {
            base.replace(text, window, cx);
            base.set_selected_range(caret..caret, cx);
        });
        cx.notify();
    }

    /// A quote closes itself, so it is its own pair.
    pub(super) fn pair_quote(
        &mut self,
        quote: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        if self.type_at_every_cursor(&base, quote, window, cx) {
            return;
        }
        let value = base.read(cx).value().to_string();
        let selection = base.read(cx).selected_range();
        let follows = || {
            value
                .get(selection.start..)
                .is_some_and(|rest| rest.starts_with(quote))
        };
        if selection.is_empty() && follows() {
            self.step_over(&base, selection.start + quote.len(), cx);
            return;
        }
        // A quote after a word is not opening a quote — it is an apostrophe,
        // or a lifetime. Zed asks the language; a word character is what that
        // amounts to for the languages here.
        let after_word =
            selection.is_empty() && is_word_char(value[..selection.start].chars().next_back());
        if after_word {
            base.update(cx, |base, cx| base.replace(quote, window, cx));
            cx.notify();
            return;
        }
        self.open_pair(quote, quote, window, cx);
    }

    /// A closing bracket typed where one already is steps over it, and
    /// anywhere else is typed as itself.
    pub(super) fn skip_closer(
        &mut self,
        close: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        if self.type_at_every_cursor(&base, close, window, cx) {
            return;
        }
        let value = base.read(cx).value().to_string();
        let selection = base.read(cx).selected_range();
        let follows = || {
            value
                .get(selection.start..)
                .is_some_and(|rest| rest.starts_with(close))
        };
        if selection.is_empty() && follows() {
            self.step_over(&base, selection.start + close.len(), cx);
            return;
        }
        base.update(cx, |base, cx| base.replace(close, window, cx));
        cx.notify();
    }

    /// Move the caret past a bracket that is already there.
    pub(super) fn step_over(
        &self,
        base: &Entity<InputBaseState>,
        caret: usize,
        cx: &mut Context<Self>,
    ) {
        base.update(cx, |base, cx| base.set_selected_range(caret..caret, cx));
        cx.notify();
    }

    /// Type one character at every cursor, when there is more than one.
    ///
    /// A pair is a decision about a single selection: with several there is
    /// nothing to wrap and nothing to step over, so the character goes in at
    /// each cursor like any other. Answering here rather than letting the
    /// single-selection paths below run matters for a second reason — those go
    /// through `replace`, which leaves one selection behind and would take the
    /// other cursors with it.
    ///
    /// Answers whether it typed, so the caller can stop.
    pub(super) fn type_at_every_cursor(
        &self,
        base: &Entity<InputBaseState>,
        text: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if base.read(cx).selected_ranges().len() <= 1 {
            return false;
        }
        base.update(cx, |base, cx| {
            base.replace_in_every_selection(text, window, cx)
        });
        cx.notify();
        true
    }

    /// `⌘/`: comment out every line the selection touches, or bring them back.
    ///
    /// The editor offers no hook for this and no way to replace an arbitrary
    /// range, so it is done by selecting the lines and replacing the selection
    /// — which is also what puts it on the undo stack as one step.
    pub(super) fn toggle_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.project.active().cloned() else {
            return;
        };
        let Some(marker) = buffer::line_comment(buffer::language(&path)) else {
            // Plenty of languages only have a block form; saying so beats
            // inserting a marker the file cannot use.
            self.warn("This language has only block comments", cx);
            return;
        };
        // An image has no buffer to comment.
        let Some(editor) = self
            .project
            .documents
            .get(&path)
            .map(|document| document.editor.clone())
        else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        let text = base.read(cx).value().to_string();
        // Every cursor's lines, from the first to the last: commenting is one
        // edit over a block, not one per caret, so the block is what the span
        // of the set covers. A single cursor gives the same range it always
        // did.
        let selections = base.read(cx).selected_ranges();
        let (Some(first), Some(last)) = (selections.first(), selections.last()) else {
            return;
        };
        let lines = buffer::line_range(&text, first.start..last.end);
        let Some(block) = text.get(lines.clone()) else {
            return;
        };
        let toggled = buffer::toggle_comments(block, marker);
        let reselect = lines.start..lines.start + toggled.len();
        base.update(cx, |base, cx| {
            base.set_selected_range(lines, cx);
            base.replace(toggled, window, cx);
            // Left selected, so pressing again undoes what this one did.
            base.set_selected_range(reselect, cx);
        });
        cx.notify();
    }

    pub(super) fn toggle_block_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.project.active().cloned() else {
            return;
        };
        let Some((open, close)) = buffer::block_comment(buffer::language(&path)) else {
            self.warn("This language has no block comments", cx);
            return;
        };
        self.replace_active_lines(window, cx, |block| {
            buffer::toggle_block_comments(block, open, close)
        });
    }

    pub(super) fn duplicate_lines(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_active_text(window, cx, |text, selection| {
            Some(buffer::duplicate_lines(text, selection))
        });
    }

    pub(super) fn move_lines(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_active_text(window, cx, |text, selection| {
            buffer::move_lines(text, selection, down)
        });
    }

    pub(super) fn replace_active_lines(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&str) -> String,
    ) {
        let Some(path) = self.project.active().cloned() else {
            return;
        };
        let Some(editor) = self
            .project
            .documents
            .get(&path)
            .map(|document| document.editor.clone())
        else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        let text = base.read(cx).value().to_string();
        let selections = base.read(cx).selected_ranges();
        let (Some(first), Some(last)) = (selections.first(), selections.last()) else {
            return;
        };
        let lines = buffer::line_range(&text, first.start..last.end);
        let Some(block) = text.get(lines.clone()) else {
            return;
        };
        let next = change(block);
        let reselect = lines.start..lines.start + next.len();
        base.update(cx, |base, cx| {
            base.set_selected_range(lines, cx);
            base.replace(next, window, cx);
            base.set_selected_range(reselect, cx);
        });
        cx.notify();
    }

    pub(super) fn replace_active_text(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&str, std::ops::Range<usize>) -> Option<(String, std::ops::Range<usize>)>,
    ) {
        let Some(path) = self.project.active().cloned() else {
            return;
        };
        let Some(editor) = self
            .project
            .documents
            .get(&path)
            .map(|document| document.editor.clone())
        else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        let text = base.read(cx).value().to_string();
        let selections = base.read(cx).selected_ranges();
        let (Some(first), Some(last)) = (selections.first(), selections.last()) else {
            return;
        };
        let Some((next, reselect)) = change(&text, first.start..last.end) else {
            return;
        };
        base.update(cx, |base, cx| {
            base.set_selected_range(0..text.len(), cx);
            base.replace(next, window, cx);
            base.set_selected_range(reselect, cx);
        });
        cx.notify();
    }

    /// `⌥⌘B`: show what the caret's line was last written by, or stop showing
    /// it.
    pub(super) fn toggle_blame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.blame.take().is_some() {
            cx.notify();
            return;
        }
        self.read_blame(window, cx);
    }

    /// Read the blame of the file being read, against what its buffer holds.
    ///
    /// The buffer rather than the file, because blame is looked up by line and
    /// the lines are the buffer's: a file with an edit in it would otherwise
    /// have everything below that edit attributed to the wrong commit.
    pub(super) fn read_blame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.project.active().cloned() else {
            return;
        };
        let Some(document) = self.project.documents.get(&path) else {
            // An image preview has no lines to blame.
            return;
        };
        let text = document.editor.read(cx).value().to_string();
        self.blame_request += 1;
        let request = self.blame_request;
        let blamed = path.clone();
        let task = cx
            .background_executor()
            .spawn(async move { blame::run(&blamed, &text) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, _, cx| {
                // The user may have moved on to another file while this was
                // being read, in which case its own read is the one that counts.
                if this.blame_request != request {
                    return;
                }
                match result {
                    Ok(blame) => this.blame = Some(BlameView { path, blame }),
                    Err(e) => {
                        this.blame = None;
                        this.error(format!("Could not read blame: {e}"), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// What the blame strip says for the line the caret is on, if it is on and
    /// there is anything to say.
    pub(super) fn blame_text(&self, cx: &App) -> Option<String> {
        let view = self.blame.as_ref()?;
        let path = self.project.active()?;
        if view.path != *path {
            return None;
        }
        let document = self.project.documents.get(path)?;
        let line = document
            .editor
            .read(cx)
            .base_state()
            .read(cx)
            .cursor_position()
            .line as usize;
        let blamed = view.blame.line(line)?;
        if blamed.uncommitted() {
            return Some("Not committed yet".to_string());
        }
        Some(format!(
            "{} · {}, {} — {}",
            blamed.short(),
            blamed.author,
            relative_time(blamed.time),
            blamed.summary
        ))
    }
}
