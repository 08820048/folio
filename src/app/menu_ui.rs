use super::*;

impl Folio {
    /// Open the context menu over `target`, flipped when it would overhang the
    /// window's bottom or right edge.
    pub(super) fn open_menu(
        &mut self,
        target: MenuTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let size = window.viewport_size();
        let (x, y) = (f32::from(position.x), f32::from(position.y));
        let height = menu_height(&target);
        let x = if x + MENU_WIDTH + 8. > f32::from(size.width) {
            (x - MENU_WIDTH).max(8.)
        } else {
            x
        };
        let y = if y + height + 8. > f32::from(size.height) {
            (y - height).max(8.)
        } else {
            y
        };
        self.menu = Some(ContextMenu {
            selected: visible_menu_items(&target).first().copied().unwrap_or(0),
            target,
            position: point(px(x), px(y)),
        });
        cx.notify();
    }

    pub(super) fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// Whether a tab's buffer has edits that are not on disk.
    pub(super) fn is_dirty(&self, path: &Path) -> bool {
        self.project
            .documents
            .get(path)
            .is_some_and(|doc| doc.dirty)
    }

    pub(super) fn is_pinned(&self, path: &Path) -> bool {
        self.project.pinned.contains(path)
    }

    pub(super) fn is_read_only(&self, path: &Path) -> bool {
        self.project.read_only.contains(path)
    }

    /// The tabs an entry would close, which is nothing for anything else. The
    /// bulk entries leave pinned tabs alone — that is what pinning is for.
    pub(super) fn tabs_to_close(&self, item: MenuItem, path: &Path) -> Vec<PathBuf> {
        let tabs = self
            .project
            .pane_index_of(path)
            .and_then(|index| self.project.panes.get(index))
            .map(|pane| pane.tabs.as_slice())
            .unwrap_or(&[]);
        let Some(index) = tabs.iter().position(|tab| tab == path) else {
            return Vec::new();
        };
        let free = |tab: &PathBuf| !self.is_pinned(tab);
        match item {
            MenuItem::CloseTab => vec![path.to_path_buf()],
            MenuItem::CloseOthers => tabs
                .iter()
                .filter(|tab| tab.as_path() != path && free(tab))
                .cloned()
                .collect(),
            MenuItem::CloseLeft => tabs[..index]
                .iter()
                .filter(|tab| free(tab))
                .cloned()
                .collect(),
            MenuItem::CloseRight => tabs[index + 1..]
                .iter()
                .filter(|tab| free(tab))
                .cloned()
                .collect(),
            MenuItem::CloseClean => tabs
                .iter()
                .filter(|tab| !self.is_dirty(tab) && free(tab))
                .cloned()
                .collect(),
            MenuItem::CloseAll => tabs.iter().filter(|tab| free(tab)).cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// Whether an entry can be taken here. Everything is available unless it
    /// would do nothing at all.
    pub(super) fn menu_item_enabled(&self, item: MenuItem, target: &MenuTarget, cx: &App) -> bool {
        let path = target.path();
        match (target, item) {
            (MenuTarget::Editor { .. }, MenuItem::Cut) => {
                self.editor_is_editable(cx) && self.editor_has_selection(cx)
            }
            (MenuTarget::Editor { .. }, MenuItem::Copy) => self.editor_has_selection(cx),
            (MenuTarget::Editor { .. }, MenuItem::Paste) => {
                self.editor_is_editable(cx) && cx.read_from_clipboard().is_some()
            }
            (MenuTarget::Editor { .. }, MenuItem::CopyRelativePath) => {
                self.project.workspace.is_some()
            }
            (_, MenuItem::Paste) => self.clipboard.is_some(),
            (
                _,
                MenuItem::CloseOthers
                | MenuItem::CloseLeft
                | MenuItem::CloseRight
                | MenuItem::CloseAll
                | MenuItem::CloseClean,
            ) => !self.tabs_to_close(item, path).is_empty(),
            (_, MenuItem::CopyRelativePath | MenuItem::RevealInTree) => {
                self.project.workspace.is_some()
            }
            _ => true,
        }
    }

    /// The label an entry shows here. Most are what the table says; the two
    /// that flip are the reason this is a call and not a lookup.
    pub(super) fn menu_label(
        &self,
        item: MenuItem,
        path: &Path,
        label: &'static str,
    ) -> SharedString {
        match item {
            MenuItem::ToggleReadOnly if self.is_read_only(path) => "Make Tab Writable".into(),
            MenuItem::PinTab if self.is_pinned(path) => "Unpin Tab".into(),
            MenuItem::Blame if self.blame.is_some() => "Hide Blame".into(),
            MenuItem::Reveal if cfg!(target_os = "windows") => "Reveal in File Explorer".into(),
            MenuItem::Trash if cfg!(target_os = "windows") => "Move to Recycle Bin".into(),
            _ => label.into(),
        }
    }

    /// Land `dragged` at `to` in `to_pane`. Same pane reorders the strip;
    /// another pane takes the tab across, and an emptied pane is pruned.
    ///
    /// Nothing here reads the pointer. The tab or strip the drop landed on is
    /// the one thing a drop reliably reports, and a rule built on anything
    /// finer — which half of a tab the pointer is over — depends on move
    /// events arriving for every tab on the way, which is exactly what could
    /// not be relied on.
    ///
    /// Pinned tabs stay at the front of whichever pane they land in.
    pub(super) fn move_tab_to(
        &mut self,
        dragged: &Path,
        to_pane: usize,
        to: usize,
        cx: &mut Context<Self>,
    ) {
        self.tab_drop = None;
        if to_pane >= self.project.panes.len() {
            return;
        }
        let Some(from_pane) = self.project.pane_index_of(dragged) else {
            return;
        };
        let Some(from) = self.project.panes[from_pane]
            .tabs
            .iter()
            .position(|tab| tab == dragged)
        else {
            return;
        };
        let tab = self.project.panes[from_pane].tabs.remove(from);
        if from_pane != to_pane {
            let source = &mut self.project.panes[from_pane];
            if source.active.as_ref() == Some(&tab) {
                source.active = source
                    .tabs
                    .get(from)
                    .or_else(|| source.tabs.last())
                    .cloned();
            }
        }
        let dest = &mut self.project.panes[to_pane];
        if dest.tabs.iter().any(|open| open == &tab) {
            dest.active = Some(tab);
            self.project.focused = to_pane;
            self.prune_empty_panes();
            cx.notify();
            return;
        }
        let pinned = dest
            .tabs
            .iter()
            .filter(|open| self.project.pinned.contains(*open))
            .count();
        let to = to.min(dest.tabs.len());
        let to = if self.project.pinned.contains(&tab) {
            to.min(pinned)
        } else {
            to.max(pinned)
        };
        dest.tabs.insert(to, tab.clone());
        if from_pane != to_pane {
            dest.active = Some(tab);
            self.project.focused = to_pane;
        }
        self.prune_empty_panes();
        cx.notify();
    }

    /// Pinned tabs sit at the front. `sort_by_key` is stable, so each group
    /// keeps the order it already had.
    pub(super) fn reorder_tabs(&mut self) {
        let pinned = self.project.pinned.clone();
        for pane in &mut self.project.panes {
            pane.tabs.sort_by_key(|tab| !pinned.contains(tab));
        }
    }

    /// Put a string on the system clipboard and say so, because the clipboard
    /// gives no sign of its own.
    pub(super) fn copy_to_clipboard(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.toast("Copied", cx);
    }

    /// Reveal the file being edited, the same path the tab menu acts on.
    pub(super) fn reveal_active_file(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.project.active().cloned() else {
            return;
        };
        self.spawn_launch(move || fs_op::reveal(&path), cx);
    }

    /// Open a terminal at the folder holding the file being edited.
    pub(super) fn open_terminal_for_active_file(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.project.active().cloned() else {
            return;
        };
        let dir = path.parent().map(Path::to_path_buf).unwrap_or(path);
        self.spawn_launch(move || fs_op::open_terminal(&dir), cx);
    }

    /// Copy the path of the file being edited, absolute or relative to the
    /// project root.
    pub(super) fn copy_active_path(&mut self, relative: bool, cx: &mut Context<Self>) {
        let Some(path) = self.project.active() else {
            return;
        };
        let text = if relative {
            self.relative_path(path)
        } else {
            path.to_string_lossy().into_owned()
        };
        self.copy_to_clipboard(text, cx);
    }

    /// Move the keyboard highlight to the next or previous visible entry.
    pub(super) fn move_menu_selection(&mut self, down: bool) {
        let Some(menu) = self.menu.as_ref() else {
            return;
        };
        let visible = visible_menu_items(&menu.target);
        let current = visible.iter().position(|&index| index == menu.selected);
        let next = match (current, down) {
            (Some(current), true) => (current + 1).min(visible.len().saturating_sub(1)),
            (Some(current), false) => current.saturating_sub(1),
            (None, true) => 0,
            (None, false) => visible.len().saturating_sub(1),
        };
        let Some(&index) = visible.get(next) else {
            return;
        };
        if let Some(menu) = self.menu.as_mut() {
            menu.selected = index;
        }
    }

    pub(super) fn run_menu_item(
        &mut self,
        item: MenuItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        if !self.menu_item_enabled(item, &menu.target, cx) {
            cx.notify();
            return;
        }
        match menu.target {
            MenuTarget::Tree { path, .. } => self.run_tree_item(item, path, window, cx),
            MenuTarget::Changes { .. } => self.run_changes_item(item, window, cx),
            MenuTarget::Tab { path, .. } => self.run_tab_item(item, path, window, cx),
            MenuTarget::Editor { path } => self.run_editor_item(item, path, window, cx),
            MenuTarget::Split { pane } => self.run_split_item(item, pane, window, cx),
        }
    }

    pub(super) fn run_split_item(
        &mut self,
        item: MenuItem,
        pane: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if pane < self.project.panes.len() {
            self.project.focused = pane;
        }
        let direction = match item {
            MenuItem::SplitRight => SplitDirection::Right,
            MenuItem::SplitLeft => SplitDirection::Left,
            MenuItem::SplitUp => SplitDirection::Up,
            MenuItem::SplitDown => SplitDirection::Down,
            _ => return,
        };
        self.split_in(direction, window, cx);
    }

    /// The entries a tab offers.
    pub(super) fn run_tab_item(
        &mut self,
        item: MenuItem,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match item {
            MenuItem::CloseTab
            | MenuItem::CloseOthers
            | MenuItem::CloseLeft
            | MenuItem::CloseRight
            | MenuItem::CloseClean
            | MenuItem::CloseAll => {
                let closing = self.tabs_to_close(item, &path);
                if !closing.is_empty() {
                    self.request(Next::CloseTabs(closing), window, cx);
                }
            }
            MenuItem::ToggleReadOnly => {
                if !self.project.read_only.remove(&path) {
                    self.project.read_only.insert(path);
                }
                cx.notify();
            }
            MenuItem::PinTab => {
                if !self.project.pinned.remove(&path) {
                    self.project.pinned.insert(path);
                }
                self.reorder_tabs();
                cx.notify();
            }
            MenuItem::CopyPath => {
                let text = path.to_string_lossy().into_owned();
                self.copy_to_clipboard(text, cx);
            }
            MenuItem::CopyRelativePath => {
                let text = self.relative_path(&path);
                self.copy_to_clipboard(text, cx);
            }
            MenuItem::RevealInTree => self.reveal_in_tree(path, window, cx),
            MenuItem::Reveal => {
                let target = path.clone();
                self.spawn_launch(move || fs_op::reveal(&target), cx);
            }
            MenuItem::OpenTerminal => {
                // A tab is a file; a terminal is useful at the folder holding
                // it rather than at the file.
                let dir = path.parent().map(Path::to_path_buf).unwrap_or(path);
                self.spawn_launch(move || fs_op::open_terminal(&dir), cx);
            }
            // Nothing else is offered on that surface.
            _ => cx.notify(),
        }
    }

    /// The entries the editor offers. Text edits go through the buffer's own
    /// actions so they share undo with typing; the rest is the file under the
    /// caret.
    pub(super) fn run_editor_item(
        &mut self,
        item: MenuItem,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match item {
            MenuItem::Cut => {
                self.focus_editor(window, cx);
                window.dispatch_action(Box::new(input::Cut), cx);
            }
            MenuItem::Copy => {
                self.focus_editor(window, cx);
                window.dispatch_action(Box::new(input::Copy), cx);
            }
            MenuItem::Paste => {
                self.focus_editor(window, cx);
                window.dispatch_action(Box::new(input::Paste), cx);
            }
            MenuItem::SelectAll => {
                self.focus_editor(window, cx);
                window.dispatch_action(Box::new(input::SelectAll), cx);
            }
            MenuItem::GoToLine => self.show_quick_open(true, window, cx),
            MenuItem::Reveal => {
                self.spawn_launch(move || fs_op::reveal(&path), cx);
            }
            MenuItem::OpenTerminal => {
                let dir = path.parent().map(Path::to_path_buf).unwrap_or(path);
                self.spawn_launch(move || fs_op::open_terminal(&dir), cx);
            }
            MenuItem::CopyPath => {
                self.copy_to_clipboard(path.to_string_lossy().into_owned(), cx);
            }
            MenuItem::CopyRelativePath => {
                let text = self.relative_path(&path);
                self.copy_to_clipboard(text, cx);
            }
            MenuItem::ShowFileChanges => self.toggle_diff(window, cx),
            MenuItem::Blame => self.toggle_blame(window, cx),
            _ => cx.notify(),
        }
    }

    /// The entries a folder in the tree offers.
    pub(super) fn run_tree_item(
        &mut self,
        item: MenuItem,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match item {
            MenuItem::NewFile => self.begin_create(path, EntryKind::File, window, cx),
            MenuItem::NewFolder => self.begin_create(path, EntryKind::Directory, window, cx),
            MenuItem::FindInFolder => self.show_search_in(path, window, cx),
            MenuItem::Rename => self.begin_rename(path, window, cx),
            MenuItem::Cut | MenuItem::Copy => {
                let cut = item == MenuItem::Cut;
                self.clipboard = Some(FileClipboard {
                    path: path.clone(),
                    cut,
                });
                let verb = if cut { "Cut" } else { "Copied" };
                self.toast(&format!("{verb} {}", name(&path)), cx);
            }
            MenuItem::Duplicate => {
                let source = path.clone();
                self.spawn_change(path, false, move || fs_op::duplicate(&source), window, cx);
            }
            MenuItem::Paste => {
                let Some(clipboard) = self.clipboard.clone() else {
                    return;
                };
                if clipboard.cut {
                    self.move_entry(clipboard.path, path, window, cx);
                } else {
                    let into = path.clone();
                    self.spawn_change(
                        path,
                        false,
                        move || fs_op::paste(&clipboard.path, &into, false),
                        window,
                        cx,
                    );
                }
            }
            // Trashing is recoverable, so it only stops to warn about edits
            // that are not on disk yet. Deleting never is.
            MenuItem::Trash if self.unsaved_under(&path) > 0 => {
                self.confirm_removal(path, true, window, cx)
            }
            MenuItem::Trash => self.spawn_removal(path, true, window, cx),
            MenuItem::Delete => self.confirm_removal(path, false, window, cx),
            MenuItem::RemoveProject => {
                if self
                    .project
                    .workspace
                    .as_ref()
                    .is_some_and(|workspace| workspace.root != path)
                {
                    self.switch_project(&path, window, cx);
                }
                self.request(Next::Close, window, cx);
            }
            MenuItem::Reveal => {
                let target = path.clone();
                self.spawn_launch(move || fs_op::reveal(&target), cx);
            }
            MenuItem::OpenDefault => {
                let target = path.clone();
                self.spawn_launch(move || fs_op::open_default(&target), cx);
            }
            MenuItem::OpenTerminal => {
                let target = path.clone();
                self.spawn_launch(move || fs_op::open_terminal(&target), cx);
            }
            // The remaining entries belong to other surfaces.
            _ => cx.notify(),
        }
    }

    /// The entries a file's changes offer. Leaving is the only one so far.
    pub(super) fn run_changes_item(
        &mut self,
        item: MenuItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match item {
            MenuItem::HideChanges => self.toggle_diff(window, cx),
            // Nothing else is offered on that surface.
            _ => cx.notify(),
        }
    }

    /// Splice a text field into the tree for a new entry under `parent`. The
    /// entry is created on Enter; Escape creates nothing.
    pub(super) fn begin_create(
        &mut self,
        parent: PathBuf,
        kind: EntryKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project.workspace.is_none() || self.project_loading || self.prompting {
            return;
        }
        self.editing = None;
        // The field needs a row to sit in, so open the folder it will go into.
        // A folder that was never opened has no children cached yet, which is
        // exactly the case `toggle_directory` also reads on demand.
        if !self.project.expanded.contains(&parent) {
            self.toggle_directory(parent.clone(), cx);
        }
        let placeholder = match kind {
            EntryKind::Directory => "Folder name",
            EntryKind::File => "File name",
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = Self::subscribe_edit(&input, window, cx);
        self.editing = Some(InlineEdit {
            parent,
            kind,
            renaming: None,
            row: 0,
            input: input.clone(),
            _subscription: subscription,
        });
        self.rebuild_rows();
        if let Some(edit) = &self.editing {
            self.project
                .tree_scroll
                .scroll_to_item(edit.row, ScrollStrategy::Nearest);
        }
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// Turn a tree row into a text field holding the entry's current name. The
    /// row already exists, so unlike creating there is nothing to splice in.
    pub(super) fn begin_rename(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_loading || self.prompting || self.editing.is_some() {
            return;
        }
        let Some(row) = self.project.rows.iter().find(|row| row.entry.path == path) else {
            return;
        };
        let (kind, name) = (row.entry.kind, row.entry.name.clone());
        let Some(parent) = path.parent().map(Path::to_path_buf) else {
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx));
        // Preselect the whole name, so typing replaces it the way Finder does.
        input.update(cx, |input, cx| {
            input.set_value(name, window, cx);
            input.select_all(window, cx);
        });
        let subscription = Self::subscribe_edit(&input, window, cx);
        self.editing = Some(InlineEdit {
            parent,
            kind,
            renaming: Some(path),
            row: 0,
            input: input.clone(),
            _subscription: subscription,
        });
        self.rebuild_rows();
        if let Some(edit) = &self.editing {
            self.project
                .tree_scroll
                .scroll_to_item(edit.row, ScrollStrategy::Nearest);
        }
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }
}
