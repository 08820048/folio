use super::*;

impl Render for Folio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.logged_ready {
            self.logged_ready = true;
            if perf_on()
                && let Some(start) = LAUNCH.get()
            {
                eprintln!(
                    "folio_first_frame_ms={:.1} rss_mb={:.1}",
                    perf_ms(*start),
                    rss_mb().unwrap_or(f64::NAN)
                );
                if std::env::var_os("FOLIO_PERF_QUIT").is_some() {
                    // A short wait lets session restore log open-tree / open-file.
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(Duration::from_secs(2)).await;
                        let _ = this.update(cx, |_, cx| cx.quit());
                    })
                    .detach();
                }
            }
        }
        if self.applied_wrap != self.settings.soft_wrap {
            self.sync_soft_wrap(window, cx);
        }
        div()
            .id("folio")
            .key_context("Folio")
            // GPUI's own fallback stack names no CJK family, so without this a
            // Chinese glyph is drawn in whatever the platform picks — usually a
            // proportional face, which breaks the character grid.
            .font(Font {
                family: cx.theme().font_family.clone(),
                fallbacks: Some(FontFallbacks::from_fonts(
                    CJK_FALLBACKS.iter().map(|name| name.to_string()).collect(),
                )),
                ..Default::default()
            })
            .when(self.project.workspace.is_none(), |el| {
                el.track_focus(&self.tree_focus)
            })
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(if self.project.workspace.is_some() {
                workspace_chrome(cx)
            } else {
                cx.theme().background
            })
            .text_color(cx.theme().foreground)
            .text_size(ui(13.))
            .on_action(cx.listener(|this, _: &OpenProject, window, cx| {
                this.request(Next::Picker, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NewProject, window, cx| {
                this.request(Next::NewProject, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CloneFromGithub, window, cx| {
                this.request(Next::CloneFromGithub, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseProject, window, cx| {
                this.request(Next::Close, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &Quit, window, cx| this.request(Next::Quit, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Save, window, cx| this.save_documents(None, window, cx)),
            )
            .on_action(cx.listener(|this, _: &SaveAll, window, cx| {
                this.save_documents(Some(Next::SaveAll), window, cx)
            }))
            .on_action(cx.listener(|this, _: &ReopenClosedTab, window, cx| {
                this.reopen_closed_tab(window, cx)
            }))
            .on_action(cx.listener(|this, _: &NextTab, window, cx| {
                let pane = this.project.focused;
                this.step_tab(pane, 1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PrevTab, window, cx| {
                let pane = this.project.focused;
                this.step_tab(pane, -1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_code(1., false, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_code(-1., false, cx)))
            .on_action(cx.listener(|this, _: &ZoomReset, _, cx| this.zoom_code(0., true, cx)))
            .on_action(cx.listener(|this, _: &QuickOpen, window, cx| {
                this.show_quick_open(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ProjectSearch, window, cx| {
                this.show_search(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ProjectReplace, window, cx| {
                this.show_search(true, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &GoToLine, window, cx| {
                    this.show_quick_open(true, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.toggle_file_tree(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleActivityBar, _, cx| {
                this.toggle_activity_bar(cx);
            }))
            .on_action(
                cx.listener(|this, _: &ToggleTerminal, window, cx| {
                    this.toggle_terminal(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| this.show_settings(cx)))
            .on_action(cx.listener(|this, _: &ToggleDiff, window, cx| this.toggle_diff(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleChanges, _, cx| this.toggle_changes_list(cx)))
            .on_action(
                cx.listener(|this, _: &SplitEditor, window, cx| this.split_editor(window, cx)),
            )
            .on_action(cx.listener(|this, _: &SplitEditorLeft, window, cx| {
                this.split_in(SplitDirection::Left, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SplitEditorUp, window, cx| {
                this.split_in(SplitDirection::Up, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SplitEditorDown, window, cx| {
                this.split_in(SplitDirection::Down, window, cx)
            }))
            .on_action(cx.listener(|this, _: &JoinEditor, window, cx| this.join_editor(window, cx)))
            .on_action(
                cx.listener(|this, _: &FocusLeftPane, window, cx| this.focus_pane(0, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FocusRightPane, window, cx| this.focus_pane(1, window, cx)),
            )
            .on_action(cx.listener(|this, _: &NextHunk, _, cx| this.move_hunk(true, cx)))
            .on_action(cx.listener(|this, _: &PrevHunk, _, cx| this.move_hunk(false, cx)))
            .on_action(cx.listener(|this, _: &NextChangeFile, window, cx| {
                this.move_change_file(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PrevChangeFile, window, cx| {
                this.move_change_file(false, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ToggleBlame, window, cx| this.toggle_blame(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleMarkdownPreview, window, cx| {
                this.toggle_markdown_preview(window, cx)
            }))
            .on_action(cx.listener(|this, _: &RevealInFinder, _, cx| this.reveal_active_file(cx)))
            .on_action(
                cx.listener(|this, _: &OpenInTerminal, _, cx| {
                    this.open_terminal_for_active_file(cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &CopyFilePath, _, cx| this.copy_active_path(false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &CopyRelativeFilePath, _, cx| {
                    this.copy_active_path(true, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &OpenAbout, _, cx| this.show_about(cx)))
            .on_action(cx.listener(|this, _: &CheckForUpdates, window, cx| {
                this.check_for_updates(window, cx)
            }))
            .on_action(cx.listener(|this, _: &Run, window, cx| this.run_or_stop(None, window, cx)))
            .on_action(cx.listener(|this, _: &Rerun, window, cx| this.rerun(window, cx)))
            .on_action(cx.listener(|this, _: &StopRun, window, cx| this.stop_run(window, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleComment, window, cx| this.toggle_comment(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleBlockComment, window, cx| {
                this.toggle_block_comment(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &DuplicateLines, window, cx| {
                    this.duplicate_lines(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &MoveLinesUp, window, cx| this.move_lines(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &MoveLinesDown, window, cx| {
                    this.move_lines(true, window, cx)
                }),
            )
            // The editor's own commands, bound only where the code editor has
            // the keyboard.
            .on_action(cx.listener(|this, _: &Fold, _, cx| {
                this.on_active_buffer(cx, |base, cx| {
                    base.fold_at_cursor(cx);
                })
            }))
            .on_action(cx.listener(|this, _: &Unfold, _, cx| {
                this.on_active_buffer(cx, |base, cx| {
                    base.unfold_at_cursor(cx);
                })
            }))
            .on_action(cx.listener(|this, _: &SelectNextOccurrence, _, cx| {
                this.on_active_buffer(cx, |base, cx| base.select_next_occurrence(cx))
            }))
            .on_action(cx.listener(|this, _: &SelectAllOccurrences, _, cx| {
                this.on_active_buffer(cx, |base, cx| base.select_all_occurrences(cx))
            }))
            .on_action(cx.listener(|this, _: &AddCursorAbove, _, cx| {
                this.on_active_buffer(cx, |base, cx| base.add_cursor_above(cx))
            }))
            .on_action(cx.listener(|this, _: &AddCursorBelow, _, cx| {
                this.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx))
            }))
            .on_action(cx.listener(|this, _: &SelectColumnUp, _, cx| {
                this.on_active_buffer(cx, |base, cx| base.extend_box(-1, cx))
            }))
            .on_action(cx.listener(|this, _: &SelectColumnDown, _, cx| {
                this.on_active_buffer(cx, |base, cx| base.extend_box(1, cx))
            }))
            // These are bound to the code editor's own key context, so the
            // search box and the settings fields keep typing brackets the
            // ordinary way.
            .on_action(
                cx.listener(|this, _: &PairParen, window, cx| this.open_pair("(", ")", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PairBracket, window, cx| {
                    this.open_pair("[", "]", window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &PairBrace, window, cx| this.open_pair("{", "}", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PairQuote, window, cx| this.pair_quote("\"", window, cx)),
            )
            // An apostrophe is safe to pair only because a quote after a word
            // is not treated as an opening quote.
            .on_action(
                cx.listener(|this, _: &PairApostrophe, window, cx| {
                    this.pair_quote("'", window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &PairBacktick, window, cx| this.pair_quote("`", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SkipParen, window, cx| this.skip_closer(")", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SkipBracket, window, cx| this.skip_closer("]", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SkipBrace, window, cx| this.skip_closer("}", window, cx)),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                this.resizing &= event.dragging();
                this.resizing_terminal &= event.dragging();
                this.resizing_pane &= event.dragging();
                if this.resizing {
                    this.sidebar_width =
                        (f32::from(event.position.x) - this.workspace_left_inset()).clamp(
                            160.,
                            (f32::from(window.viewport_size().width) * 0.4).max(160.),
                        );
                    cx.notify();
                }
                if this.resizing_terminal {
                    let viewport = window.viewport_size();
                    if this.terminal_right {
                        // The dock's left edge drags its width against the
                        // card's right edge, the way the drawer's top edge
                        // drags its height against the card's bottom.
                        this.terminal_width = (f32::from(viewport.width)
                            - WORKSPACE_INSET
                            - f32::from(event.position.x))
                        .clamp(200., f32::from(viewport.width) * 0.6);
                    } else {
                        this.terminal_height = (f32::from(viewport.height)
                            - f32::from(event.position.y))
                        .clamp(120., f32::from(viewport.height) * 0.7);
                    }
                    cx.notify();
                }
                if this.resizing_pane {
                    if this.project.split_axis == SplitAxis::Vertical {
                        // Title bar plus the card's top inset; the terminal
                        // drawer, when open below, eats the bottom of this
                        // column. A right-hand dock does not.
                        let top = 34. + WORKSPACE_INSET;
                        let bottom = f32::from(window.viewport_size().height)
                            - if this.terminal_open && !this.terminal_right {
                                this.terminal_height
                            } else {
                                0.
                            }
                            - WORKSPACE_INSET;
                        let height = (bottom - top).max(1.);
                        this.pane_ratio =
                            ((f32::from(event.position.y) - top) / height).clamp(0.25, 0.75);
                    } else {
                        let left = this.workspace_left_inset()
                            + if this.sidebar {
                                this.sidebar_width + 5.
                            } else {
                                0.
                            };
                        // The right-hand terminal column eats this
                        // column's width the way the drawer eats the
                        // vertical axis's height.
                        let right_dock = if this.terminal_open && this.terminal_right {
                            this.terminal_width + 4.
                        } else {
                            0.
                        };
                        let width =
                            (f32::from(window.viewport_size().width) - left - right_dock).max(1.);
                        this.pane_ratio =
                            ((f32::from(event.position.x) - left) / width).clamp(0.25, 0.75);
                    }
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.resizing = false;
                    this.resizing_terminal = false;
                    this.resizing_pane = false;
                    // A drag that ended anywhere, dropped or not, takes its
                    // caret with it.
                    if this.tab_drop.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                if let Some(path) = paths.0.first() {
                    this.request(Next::Open(path.clone()), window, cx);
                }
            }))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // The menu sits on top of everything, so it gets first refusal
                // on every key while it is open.
                if this.menu.is_some() {
                    match event.keystroke.key.as_str() {
                        "escape" => this.close_menu(cx),
                        "up" => this.move_menu_selection(false),
                        "down" => this.move_menu_selection(true),
                        "enter" => {
                            let chosen = this.menu.as_ref().and_then(|menu| {
                                surface_of(&menu.target)
                                    .menu()
                                    .0
                                    .get(menu.selected)
                                    .map(|(item, _, _)| *item)
                            });
                            if let Some(item) = chosen {
                                this.run_menu_item(item, window, cx);
                            }
                        }
                        _ => {
                            let Some(menu) = this.menu.as_ref() else {
                                return;
                            };
                            let (entries, _) = surface_of(&menu.target).menu();
                            let Some(item) = visible_menu_items(&menu.target)
                                .into_iter()
                                .filter_map(|index| entries.get(index))
                                .find(|(_, _, shortcut)| {
                                    matches_shortcut(&event.keystroke, shortcut)
                                })
                                .map(|(item, _, _)| *item)
                            else {
                                // Anything else keeps its usual meaning.
                                return;
                            };
                            this.run_menu_item(item, window, cx);
                        }
                    }
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                // Zed: ⌘K then an arrow splits that way. The prefix is not a
                // GPUI binding, so it is armed here and spent on the next key.
                if this.split_chord {
                    this.split_chord = false;
                    let direction = match event.keystroke.key.as_str() {
                        "left" => Some(SplitDirection::Left),
                        "right" => Some(SplitDirection::Right),
                        "up" => Some(SplitDirection::Up),
                        "down" => Some(SplitDirection::Down),
                        _ => None,
                    };
                    if let Some(direction) = direction {
                        this.split_in(direction, window, cx);
                        cx.stop_propagation();
                        cx.notify();
                        return;
                    }
                    if event.keystroke.key == "escape" {
                        cx.stop_propagation();
                        return;
                    }
                }
                if this.project.workspace.is_some()
                    && this.panel.is_none()
                    && this.editing.is_none()
                    && !this.is_composing(cx)
                    && !this
                        .active_terminal()
                        .is_some_and(|view| view.read(cx).focus_handle().is_focused(window))
                    && matches_shortcut(&event.keystroke, "⌘K")
                {
                    this.split_chord = true;
                    cx.stop_propagation();
                    return;
                }
                // The editor binds ⌘⇧F to its own in-file replace, and a
                // binding on the focused element beats one further out — so
                // while the code has focus, which is most of the time, the
                // project search never saw it. The capture phase runs before
                // bindings are resolved at all, so it is claimed here.
                if matches_shortcut(&event.keystroke, "\u{2318}\u{21e7}F") {
                    this.show_search(false, window, cx);
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                // Escape has to be caught here: the input owns the key context
                // while a row is being named. A composition is the exception —
                // the marked run has to leave the buffer first.
                if event.keystroke.key == "escape" && this.is_composing(cx) {
                    return;
                }
                if this.clone_open && event.keystroke.key == "escape" {
                    this.close_clone_prompt(window, cx);
                    cx.stop_propagation();
                    return;
                }
                if this.editing.is_some() {
                    if event.keystroke.key == "escape" {
                        this.cancel_create(window, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                    return;
                }
                let Some(panel) = this.panel else {
                    // Nothing else wants it: Escape leaves the changes view,
                    // which is otherwise only reachable by its shortcut.
                    if this.project.diff.is_some() && event.keystroke.key == "escape" {
                        this.toggle_diff(window, cx);
                        cx.stop_propagation();
                    }
                    return;
                };
                match event.keystroke.key.as_str() {
                    "escape" => this.close_panel(window, cx),
                    "down" | "up" => {
                        let down = event.keystroke.key == "down";
                        match panel {
                            Panel::Search => this.move_search_selection(down),
                            Panel::Files => {
                                let last = this.matches.len().saturating_sub(1);
                                this.match_selected = if down {
                                    (this.match_selected + 1).min(last)
                                } else {
                                    this.match_selected.saturating_sub(1)
                                };
                                this.quick_scroll
                                    .scroll_to_item(this.match_selected, ScrollStrategy::Nearest);
                            }
                        }
                    }
                    "left" | "right" => {
                        if !this.panel_arrow(event.keystroke.key.as_str(), window, cx) {
                            return;
                        }
                    }
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .child(self.render_titlebar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(if self.project.workspace.is_some() {
                        div()
                            .size_full()
                            .flex()
                            .when(self.activity_bar, |el| {
                                el.child(self.render_activity_bar(cx))
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .pt(px(WORKSPACE_INSET))
                                    .pr(px(WORKSPACE_INSET))
                                    .pb(px(WORKSPACE_INSET))
                                    .pl(px(if self.activity_bar {
                                        WORKSPACE_RAIL_GAP
                                    } else {
                                        WORKSPACE_INSET
                                    }))
                                    .child(self.render_workspace(window, cx)),
                            )
                            .into_any_element()
                    } else {
                        self.render_launcher(cx)
                    }),
            )
            .when(self.clone_open, |el| el.child(self.render_clone_prompt(cx)))
            .when_some(self.panel, |el, panel| {
                el.child(self.render_panel(panel, cx))
            })
            .when(self.menu.is_some(), |el| {
                el.child(self.render_menu_backdrop(cx))
            })
            .when_some(self.menu.as_ref(), |el, menu| {
                el.child(self.render_menu(menu, cx))
            })
            .when_some(self.notice.clone(), |el, notice| {
                el.child(self.render_notice(&notice, cx))
            })
    }
}
