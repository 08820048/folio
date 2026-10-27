use super::*;

impl Folio {
    /// Flip one of the `Aa` / `ab` / `.*` toggles and rescan.
    pub(super) fn toggle_search_option(
        &mut self,
        toggle: impl Fn(&mut search::Options),
        cx: &mut Context<Self>,
    ) {
        toggle(&mut self.search.options);
        self.start_search(cx);
        cx.notify();
    }

    /// Compile the current query and scan the project for it.
    ///
    /// The scan runs on the background executor after a short debounce.
    /// `search_request` identifies the newest scan and lets an older one give up
    /// instead of racing it for the CPU.
    pub(super) fn start_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search_query.read(cx).value().to_string();
        self.search.query = query.clone();
        self.search.selected = 0;
        self.search.error = None;
        self.search.truncated = false;
        self.search.stopped = false;
        self.search.results.clear();
        self.search.indices.clear();
        self.search.rows.clear();
        self.search.scroll.scroll_to_item(0, ScrollStrategy::Top);
        let request = self.search_request.fetch_add(1, Ordering::Relaxed) + 1;
        if query.trim().is_empty() {
            self.search.running = false;
            cx.notify();
            return;
        }
        let matcher = match search::Matcher::new(&query, self.search.options) {
            Ok(matcher) => matcher,
            Err(error) => {
                self.search.running = false;
                self.search.error = Some(error);
                cx.notify();
                return;
            }
        };
        self.search.running = true;
        cx.notify();
        let cancel = self.search_request.clone();
        let cancel_inner = cancel.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            if cancel.load(Ordering::Relaxed) != request {
                return;
            }
            // Snapshot the file list and the open buffers only once the query
            // has settled, so typing does not copy them on every keystroke.
            let prepared = this
                .update(cx, |this, cx| {
                    if this.search_request.load(Ordering::Relaxed) != request {
                        return None;
                    }
                    let mut files = this.project.files.clone();
                    if let Some(scope) = &this.search.scope {
                        files.retain(|path| path.starts_with(scope));
                    }
                    let root = this
                        .project
                        .workspace
                        .as_ref()
                        .map(|workspace| workspace.root.clone());
                    let filter = this.search_filter.read(cx).value();
                    if !filter.trim().is_empty() {
                        files.retain(|path| search_filter_matches(path, root.as_deref(), &filter));
                    }
                    let open = this
                        .project
                        .documents
                        .iter()
                        .map(|(path, document)| {
                            (path.clone(), document.editor.read(cx).value().to_string())
                        })
                        .collect::<HashMap<PathBuf, String>>();
                    Some((files, open))
                })
                .ok()
                .flatten();
            let Some((files, open)) = prepared else {
                return;
            };
            // The scan reports each file as a worker finishes it. A channel
            // carries those batches to this task, which is the only place the
            // results list is touched; the background task only searches.
            let (sender, receiver) = mpsc::channel::<(usize, search::FileHits)>();
            let sender = Arc::new(Mutex::new(sender));
            let done = Arc::new(AtomicBool::new(false));
            let truncated = Arc::new(AtomicBool::new(false));
            let worker_cancel = cancel_inner.clone();
            let worker_done = done.clone();
            let worker_truncated = truncated.clone();
            let worker_sender = sender.clone();
            cx.background_executor()
                .spawn(async move {
                    let hit_limit = search::search_files_stream(
                        &files,
                        &open,
                        &matcher,
                        || worker_cancel.load(Ordering::Relaxed) == request,
                        |index, file| {
                            if let Ok(sender) = worker_sender.lock() {
                                // A closed receiver means the panel is gone;
                                // the worker will see the cancellation flag.
                                let _ = sender.send((index, file));
                            }
                        },
                    );
                    worker_truncated.store(hit_limit, Ordering::Relaxed);
                    worker_done.store(true, Ordering::Release);
                })
                .detach();

            loop {
                while let Ok((index, file)) = receiver.try_recv() {
                    let _ = this.update(cx, |this, cx| {
                        if this.search_request.load(Ordering::Relaxed) == request {
                            this.apply_search_batch(index, file);
                            cx.notify();
                        }
                    });
                }
                if done.load(Ordering::Acquire) {
                    // Sends happen before `done`, but drain once more so a
                    // batch that landed between the two is not lost.
                    while let Ok((index, file)) = receiver.try_recv() {
                        let _ = this.update(cx, |this, cx| {
                            if this.search_request.load(Ordering::Relaxed) == request {
                                this.apply_search_batch(index, file);
                                cx.notify();
                            }
                        });
                    }
                    let _ = this.update(cx, |this, cx| {
                        if this.search_request.load(Ordering::Relaxed) != request {
                            return;
                        }
                        this.search.running = false;
                        this.search.truncated |= truncated.load(Ordering::Relaxed);
                        if this.search.rows.get(this.search.selected).is_none() {
                            this.search.selected = this
                                .search
                                .rows
                                .iter()
                                .position(|row| matches!(row, SearchRow::Hit { .. }))
                                .unwrap_or(0);
                        }
                        cx.notify();
                    });
                    return;
                }
                if cancel.load(Ordering::Relaxed) != request {
                    return;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(60))
                    .await;
            }
        })
        .detach();
    }

    pub(super) fn open_selected_hit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(SearchRow::Hit { file, hit }) =
            self.search.rows.get(self.search.selected).copied()
        else {
            return;
        };
        let Some(file) = self.search.results.get(file) else {
            return;
        };
        let (path, hit) = (file.path.clone(), file.hits[hit].clone());
        self.goto = Some((path.clone(), Position::new(hit.line, hit.column)));
        self.open_file(path, window, cx);
    }

    /// Move the highlight to the next or previous match, skipping file headers.
    pub(super) fn move_search_selection(&mut self, down: bool) {
        let hits = self
            .search
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, SearchRow::Hit { .. }))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let current = hits.iter().position(|&index| index == self.search.selected);
        let next = match (current, down) {
            (Some(current), true) => (current + 1).min(hits.len() - 1),
            (Some(current), false) => current.saturating_sub(1),
            (None, _) => 0,
        };
        let Some(&selected) = hits.get(next) else {
            return;
        };
        self.search.selected = selected;
        self.search
            .scroll
            .scroll_to_item(selected, ScrollStrategy::Nearest);
    }

    pub(super) fn total_hits(&self) -> usize {
        self.search.results.iter().map(|file| file.hits.len()).sum()
    }

    /// One file from the stream. Its index keeps the list in the same order a
    /// completed scan would have produced, even though workers finish out of
    /// order. The caps are enforced here too so a partial list cannot exceed
    /// what a finished one would show.
    pub(super) fn apply_search_batch(&mut self, index: usize, mut file: search::FileHits) {
        if self.search.indices.contains(&index) {
            return;
        }
        if self.search.results.len() >= search::FILE_LIMIT {
            self.search.truncated = true;
            return;
        }
        let room = search::MATCH_LIMIT.saturating_sub(self.total_hits());
        if room == 0 {
            self.search.truncated = true;
            return;
        }
        if file.hits.len() > room {
            self.search.truncated = true;
            file.hits.truncate(room);
        }
        if file.hits.is_empty() {
            return;
        }
        let position = self.search.indices.partition_point(|&seen| seen < index);
        self.search.indices.insert(position, index);
        self.search.results.insert(position, file);
        self.search.rows = search_rows(&self.search.results);
        if self.search.rows.get(self.search.selected).is_none() {
            self.search.selected = self
                .search
                .rows
                .iter()
                .position(|row| matches!(row, SearchRow::Hit { .. }))
                .unwrap_or(0);
        }
    }

    /// Stop the running scan but keep what it has found so far. A new query
    /// starts a fresh scan; closing the panel clears the list.
    pub(super) fn stop_search(&mut self, cx: &mut Context<Self>) {
        if !self.search.running {
            return;
        }
        self.search_request.fetch_add(1, Ordering::Relaxed);
        self.search.running = false;
        self.search.stopped = true;
        cx.notify();
    }

    /// Confirm, then replace every match the results list points at.
    pub(super) fn replace_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving
            || self.prompting
            || self.project_loading
            || self.search.running
            || self.search.error.is_some()
            || self.search.results.is_empty()
        {
            return;
        }
        let files = self.search.results.len();
        let hits = self.total_hits();
        self.prompting = true;
        cx.notify();
        let answer = window.prompt(
            PromptLevel::Warning,
            "Replace All",
            Some(&format!(
                "Replace {hits} occurrences across {files} files. Files that are not open are written to disk immediately; this cannot be undone."
            )),
            &["Replace All", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await.unwrap_or(1);
            let _ = this.update_in(cx, |this, window, cx| {
                this.prompting = false;
                if answer == 0 {
                    this.apply_replace(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn apply_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let replacement = self.replace_query.read(cx).value().to_string();
        let Ok(matcher) = search::Matcher::new(&self.search.query, self.search.options) else {
            return;
        };
        let targets = self
            .search
            .results
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();

        // Open buffers are edited in memory: the change stays undoable and
        // reaches the disk through the usual ⌘S flow.
        let mut edited = 0;
        let mut open_files = 0;
        for path in &targets {
            let Some(document) = self.project.documents.get(path) else {
                continue;
            };
            let text = document.editor.read(cx).value().to_string();
            let (updated, count) = matcher.replace(&text, &replacement);
            if count == 0 {
                continue;
            }
            let editor = document.editor.clone();
            editor.update(cx, |state, cx| {
                state
                    .base_state()
                    .clone()
                    .update(cx, |base, cx| base.replace_all(updated, window, cx));
            });
            if let Some(document) = self.project.documents.get_mut(path) {
                document.revision = document.revision.wrapping_add(1);
            }
            edited += count;
            open_files += 1;
        }
        for path in &targets {
            if let Some(document) = self.project.documents.get_mut(path) {
                document.dirty = document.editor.read(cx).is_modified(&document.saved);
            }
        }

        // Closed files are rewritten on disk, atomically and only while they
        // still hold what the search saw.
        let disk = targets
            .iter()
            .filter(|path| !self.project.documents.contains_key(*path))
            .cloned()
            .collect::<Vec<_>>();
        let workspace = self.project.workspace.clone();
        if !disk.is_empty() {
            self.saving = true;
            cx.notify();
        }
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut replaced = 0;
                    let mut files = 0;
                    let mut errors = Vec::new();
                    for path in disk {
                        let outcome = workspace
                            .as_ref()
                            .ok_or_else(|| io::Error::other("Project is closed"))
                            .and_then(|workspace| workspace.resolve(&path))
                            .and_then(|path| {
                                let original = buffer::read(&path)?;
                                let (updated, count) = matcher.replace(&original, &replacement);
                                if count == 0 {
                                    return Ok(0);
                                }
                                buffer::save(&path, &updated, &original)?;
                                Ok(count)
                            });
                        match outcome {
                            Ok(count) => {
                                replaced += count;
                                files += 1;
                            }
                            Err(error) => errors.push(format!("{}：{error}", name(&path))),
                        }
                    }
                    (files, replaced, errors)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                let (files, replaced, errors) = result;
                let replaced = replaced + edited;
                let files = files + open_files;
                if errors.is_empty() {
                    this.update_title(window);
                    this.refresh_git(cx);
                    this.toast(
                        &if open_files == 0 {
                            format!("Replaced {replaced} in {files} files")
                        } else {
                            format!(
                                "Replaced {replaced} in {files} files ({open_files} open files still to save)"
                            )
                        },
                        cx,
                    );
                } else {
                    this.error(errors.join("\n"), cx);
                }
                this.start_search(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn accept_match(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value();
        if let Some(line) = query.trim().strip_prefix(':') {
            if let Ok(line) = line.parse::<u32>()
                && line > 0
                && let Some(doc) = self
                    .project
                    .active()
                    .and_then(|p| self.project.documents.get(p))
            {
                doc.editor
                    .read(cx)
                    .base_state()
                    .clone()
                    .update(cx, |base, cx| {
                        base.set_cursor_position(Position::new(line - 1, 0), window, cx)
                    });
                self.panel = None;
                cx.notify();
                return;
            }
            self.error("Enter a line number, for example :123", cx);
        } else if let Some(path) = self.matches.get(self.match_selected).cloned() {
            self.open_file(path, window, cx);
        }
    }

    /// The `⌘P` / `⇧⌘F` overlay. Both lookups share one panel so the mode switch
    /// is a click away instead of a separate dialog.
    pub(super) fn render_panel(&self, panel: Panel, cx: &mut Context<Self>) -> AnyElement {
        let width = if panel == Panel::Search { 720. } else { 520. };
        div()
            .id("panel")
            .absolute()
            .top(px(48.))
            .left(relative(0.5))
            .ml(px(-width / 2.))
            .w(px(width))
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .shadow_lg()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.render_panel_header(panel, cx))
            .when(panel == Panel::Files, |el| {
                el.child(self.render_file_finder(cx))
            })
            .when(panel == Panel::Search, |el| {
                el.child(self.render_project_search(cx))
            })
            .into_any_element()
    }

    /// Mode tabs, the `Aa` / `ab` / `.*` filters, and the replace toggle.
    pub(super) fn render_panel_header(&self, panel: Panel, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(self.panel_tab("panel-tab-files", "Files", Panel::Files, cx))
            .child(self.panel_tab("panel-tab-search", "Contents", Panel::Search, cx))
            .child(div().flex_1())
            .when(panel == Panel::Search, |el| {
                el.child(self.option_toggle(
                    "search-case",
                    "Aa",
                    "Match case",
                    self.search.options.case_sensitive,
                    |options| options.case_sensitive = !options.case_sensitive,
                    cx,
                ))
                .child(self.option_toggle(
                    "search-word",
                    "ab",
                    "Match whole word",
                    self.search.options.whole_word,
                    |options| options.whole_word = !options.whole_word,
                    cx,
                ))
                .child(self.option_toggle(
                    "search-regex",
                    ".*",
                    "Regular expression; $1 in the replacement refers to a capture group",
                    self.search.options.regex,
                    |options| options.regex = !options.regex,
                    cx,
                ))
                .child(Self::icon_button(
                    "search-replace-toggle",
                    IconName::Replace,
                    "Replace",
                    |this, window, cx| {
                        this.search.show_replace = !this.search.show_replace;
                        if this.search.show_replace {
                            this.replace_query
                                .update(cx, |query, cx| query.focus(window, cx));
                        } else {
                            this.search_query
                                .update(cx, |query, cx| query.focus(window, cx));
                        }
                        cx.notify();
                    },
                    cx,
                ))
            })
            .child(Self::icon_button(
                "panel-close",
                IconName::Close,
                "Close",
                |this, window, cx| this.close_panel(window, cx),
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn panel_tab(
        &self,
        id: &'static str,
        label: &'static str,
        panel: Panel,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.panel == Some(panel);
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .focusable()
            .tab_index(0)
            .h(px(22.))
            .px_2()
            .flex()
            .items_center()
            .rounded_sm()
            .cursor_pointer()
            .text_size(ui(11.))
            .text_color(if active {
                cx.theme().accent_foreground
            } else {
                cx.theme().muted_foreground
            })
            .hover(|el| el.text_color(cx.theme().foreground))
            .focus_visible(|el| el.text_color(cx.theme().accent_foreground))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| this.select_panel(panel, window, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.select_panel(panel, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(label)
            .into_any_element()
    }

    /// A text toggle. An "on" state is shown with colour alone, matching the
    /// icon rule in AGENTS.md: no background box, no border, hover or not.
    pub(super) fn option_toggle(
        &self,
        id: &'static str,
        label: &'static str,
        tooltip: &'static str,
        active: bool,
        toggle: fn(&mut search::Options),
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(if active {
                format!("{tooltip} (on)")
            } else {
                tooltip.to_string()
            })
            .focusable()
            .tab_index(0)
            .h(px(22.))
            .min_w(px(24.))
            .px_1()
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .cursor_pointer()
            .font_family(self.code_font())
            .text_size(ui(11.))
            .text_color(if active {
                cx.theme().accent_foreground
            } else {
                cx.theme().muted_foreground
            })
            .hover(|el| el.text_color(cx.theme().foreground))
            .focus_visible(|el| el.text_color(cx.theme().accent_foreground))
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(tooltip).build(window, cx)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_search_option(toggle, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.toggle_search_option(toggle, cx);
                    cx.stop_propagation();
                }
            }))
            .child(label)
            .into_any_element()
    }

    /// `⌘P`: fuzzy file names, or `:123` to jump to a line.
    pub(super) fn render_file_finder(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.query.read(cx).value();
        let jumping = query.starts_with(':');
        div()
            .flex()
            .flex_col()
            .gap_2()
            // `focus_bordered(false)` drops the focus ring: without it gpui-component
            // repaints the border in `theme().ring` and draws a second ring outside
            // the box, which reads as a grey halo on a panel this small.
            .child(Input::new(&self.query).focus_bordered(false))
            .child(
                div()
                    .px_2()
                    .text_size(ui(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.project.indexing {
                        "Indexing files…"
                    } else if jumping {
                        "Enter to go to the line · Esc to close"
                    } else if query.trim().is_empty() {
                        "Recent files first · type to filter · ↑ ↓ · Enter"
                    } else {
                        "↑ ↓ to choose · Enter to open · Esc to close"
                    }),
            )
            .when(!jumping, |el| {
                el.child(
                    uniform_list(
                        "quick-results",
                        self.matches.len(),
                        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|i| {
                                    let path = this.matches[i].clone();
                                    let label = this.relative_path(&path);
                                    div()
                                        .id(("match", i))
                                        .h(px(30.))
                                        .px_2()
                                        .flex()
                                        .items_center()
                                        .text_size(ui(12.))
                                        .rounded_sm()
                                        .when(i == this.match_selected, |el| {
                                            el.bg(cx.theme().list_active)
                                        })
                                        .hover(|el| el.bg(cx.theme().list_hover))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.open_file(path.clone(), window, cx)
                                        }))
                                        .child(div().truncate().child(label))
                                })
                                .collect()
                        }),
                    )
                    .track_scroll(&self.quick_scroll)
                    .h(px((self.matches.len().min(10) * 30) as f32)),
                )
            })
            .into_any_element()
    }

    /// `⇧⌘F`: project-wide content search with optional replace.
    pub(super) fn render_project_search(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(Input::new(&self.search_query).focus_bordered(false))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.search_filter).focus_bordered(false)),
                    )
                    .when(
                        !self.search_filter.read(cx).value().trim().is_empty()
                            || self.search.scope.is_some(),
                        |el| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(ui(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("space separated · ! excludes"),
                            )
                        },
                    ),
            )
            .when_some(self.search.scope.clone(), |el, scope| {
                let relative = self.relative_path(&scope);
                let label = if relative.is_empty() {
                    name(&scope)
                } else {
                    relative
                };
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .text_size(ui(11.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("In {label}"))
                        .child(Self::icon_button(
                            "search-scope-clear",
                            IconName::Close,
                            "Search the whole project",
                            |this, _, cx| {
                                this.search.scope = None;
                                this.start_search(cx);
                                cx.notify();
                            },
                            cx,
                        )),
                )
            })
            .when(self.search.show_replace, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&self.replace_query).focus_bordered(false)),
                        )
                        .child(
                            Button::new("replace-all")
                                .text()
                                .label("Replace All")
                                .xsmall()
                                .disabled(
                                    self.search.results.is_empty()
                                        || self.search.error.is_some()
                                        || self.search.running
                                        || self.saving,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.replace_project(window, cx)
                                })),
                        ),
                )
            })
            .child(self.render_search_status(cx))
            .when(!self.search.rows.is_empty(), |el| {
                el.child(
                    uniform_list(
                        "project-search-results",
                        self.search.rows.len(),
                        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            range.map(|i| this.render_search_row(i, cx)).collect()
                        }),
                    )
                    .track_scroll(&self.search.scroll)
                    .h(px((self.search.rows.len().min(12) * 26) as f32)),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_search_status(&self, cx: &Context<Self>) -> AnyElement {
        let error = self.search.error.is_some();
        let text = if let Some(error) = &self.search.error {
            error.clone()
        } else if self.project.indexing && self.search.running {
            "Indexing files…".into()
        } else if self.search.running {
            if self.search.results.is_empty() {
                "Searching…".into()
            } else {
                format!(
                    "Searching… {} matches · {} files so far",
                    self.total_hits(),
                    self.search.results.len()
                )
            }
        } else if self.search.stopped {
            format!(
                "Stopped · {} matches · {} files",
                self.total_hits(),
                self.search.results.len()
            )
        } else if self.search.query.trim().is_empty() {
            "Type to search the project".into()
        } else if self.search.results.is_empty() {
            "No results".into()
        } else {
            format!(
                "{} matches · {} files{}",
                self.total_hits(),
                self.search.results.len(),
                if self.search.truncated {
                    " · results truncated"
                } else {
                    ""
                }
            )
        };
        div()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .text_size(ui(11.))
            .text_color(if error {
                cx.theme().warning
            } else {
                cx.theme().muted_foreground
            })
            .child(div().flex_1().min_w_0().child(text))
            .when(self.search.running, |el| {
                el.child(
                    div()
                        .id("search-stop")
                        .role(Role::Button)
                        .aria_label("Stop search")
                        .cursor_pointer()
                        .text_color(cx.theme().accent_foreground)
                        .hover(|el| el.text_color(cx.theme().foreground))
                        .on_click(cx.listener(|this, _, _, cx| this.stop_search(cx)))
                        .child("Stop"),
                )
            })
            .when(
                !self.search.running && !error && !self.search.rows.is_empty(),
                |el| el.child("↑ ↓ to choose · Enter to open · Esc to close"),
            )
            .into_any_element()
    }

    pub(super) fn render_search_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let selected = index == self.search.selected;
        let base = div()
            .id(("search-row", index))
            .w_full()
            .h(px(26.))
            .flex()
            .items_center()
            .gap_2()
            .text_size(ui(12.))
            .cursor_default()
            .when(selected, |el| el.bg(cx.theme().list_active))
            .hover(|el| el.bg(cx.theme().list_hover));
        match self.search.rows[index] {
            SearchRow::File(file) => {
                let entry = &self.search.results[file];
                let label = self.relative_path(&entry.path);
                let count = entry.hits.len();
                let path = entry.path.clone();
                let position = entry
                    .hits
                    .first()
                    .map(|hit| Position::new(hit.line, hit.column));
                base.px_2()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(position) = position {
                            this.goto = Some((path.clone(), position));
                        }
                        this.open_file(path.clone(), window, cx);
                    }))
                    .child(
                        Icon::new(IconName::File)
                            .xsmall()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(label))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{count}")),
                    )
                    .into_any_element()
            }
            SearchRow::Hit { file, hit } => {
                let found = &self.search.results[file].hits[hit];
                let preview = found.preview.clone();
                let before = preview[..found.start].to_string();
                let matched = preview[found.start..found.end].to_string();
                let after = preview[found.end..].to_string();
                let line = found.line + 1;
                let path = self.search.results[file].path.clone();
                let position = Position::new(found.line, found.column);
                base.pl(px(18.))
                    .pr_2()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.goto = Some((path.clone(), position));
                        this.open_file(path.clone(), window, cx);
                    }))
                    .child(
                        div()
                            .w(px(44.))
                            .flex_shrink_0()
                            .text_right()
                            .font_family(self.code_font())
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{line}")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .font_family(self.code_font())
                            .child(div().flex_shrink_0().whitespace_nowrap().child(before))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .whitespace_nowrap()
                                    .text_color(cx.theme().accent_foreground)
                                    .child(matched),
                            )
                            .child(div().flex_shrink_0().whitespace_nowrap().child(after)),
                    )
                    .into_any_element()
            }
        }
    }
}
