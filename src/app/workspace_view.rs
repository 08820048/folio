use super::*;

impl Folio {
    pub(super) fn toggle_file_tree(&mut self, cx: &mut Context<Self>) {
        if self.sidebar && self.sidebar_kind == SidebarKind::Changes {
            self.sidebar_kind = SidebarKind::Tree;
            cx.notify();
            return;
        }
        self.settings.sidebar = !self.sidebar;
        self.apply_settings(cx);
    }

    /// The rail's compare icon: the dirty-file list, or hide it if it is up.
    pub(super) fn toggle_changes_list(&mut self, cx: &mut Context<Self>) {
        if self.sidebar && self.sidebar_kind == SidebarKind::Changes {
            self.settings.sidebar = false;
            self.apply_settings(cx);
            return;
        }
        self.sidebar_kind = SidebarKind::Changes;
        if !self.sidebar {
            self.settings.sidebar = true;
            self.apply_settings(cx);
        }
        self.refresh_git(cx);
        cx.notify();
    }

    /// The side panel's switch: show the tree or the change list, opening
    /// the panel if it was hidden, and closing a project search that had
    /// the "Search" segment.
    pub(super) fn show_side(
        &mut self,
        kind: SidebarKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.panel == Some(Panel::Search) {
            self.close_panel(window, cx);
        }
        self.sidebar_kind = kind;
        if !self.sidebar {
            self.settings.sidebar = true;
            self.apply_settings(cx);
        }
        if kind == SidebarKind::Changes {
            self.refresh_git(cx);
        }
        cx.notify();
    }

    /// Focus mode on or off. The terminal keeps running while hidden.
    pub(super) fn set_focus(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus == on {
            return;
        }
        self.focus = on;
        if on {
            self.close_panel(window, cx);
        }
        cx.notify();
    }

    /// The heading over the code in focus mode: the file in large type and
    /// a line about it.
    fn render_focus_heading(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let Some(path) = self.project.panes.get(index).and_then(|p| p.active.clone()) else {
            return div().into_any_element();
        };
        let rel = self
            .project
            .workspace
            .as_ref()
            .and_then(|w| path.strip_prefix(&w.root).ok())
            .and_then(|rel| rel.parent())
            .map(|dir| dir.to_string_lossy().into_owned())
            .filter(|dir| !dir.is_empty());
        let changed = self
            .project
            .workspace
            .as_ref()
            .and_then(|w| path.strip_prefix(&w.root).ok())
            .and_then(|rel| self.project.git_status.get(rel.to_string_lossy().as_ref()))
            .copied();
        let language = buffer::language(&path);
        div()
            .w_full()
            .max_w(px(FOCUS_COLUMN))
            .mx_auto()
            .pt(px(20.))
            .pb(px(14.))
            .pl(px(64.))
            .flex()
            .items_baseline()
            .gap(px(14.))
            .child(
                div()
                    .text_size(ui(22.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(name(&path)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .text_size(ui(12.))
                    .text_color(cx.theme().muted_foreground)
                    .when_some(rel, |el, rel| el.child(rel).child("·"))
                    .child(status_bar::language_label(language))
                    .when_some(changed, |el, status| {
                        el.child("·").child(
                            div()
                                .text_color(if status == 'U' {
                                    git_added_color(cx)
                                } else {
                                    git_modified_color(cx)
                                })
                                .child(if status == 'U' { "new file" } else { "changed" }),
                        )
                    })
                    .when_some(self.project.git_branch.clone(), |el, branch| {
                        el.child("·").child(
                            div()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(branch),
                        )
                    }),
            )
            .into_any_element()
    }

    /// How far the side panel starts from the window's left edge.
    pub(super) fn workspace_left_inset(&self) -> f32 {
        WORKSPACE_INSET
    }

    pub(super) fn render_titlebar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        // gpui-component's title bar row will not shrink below its content,
        // so the content is given the row's width outright; then the tabs
        // scroll and the command bar narrows instead of pushing Run out.
        let row = (f32::from(window.viewport_size().width)
            - if cfg!(target_os = "macos") && !window.is_fullscreen() {
                80.
            } else {
                12.
            })
        .max(200.);
        let bar = TitleBar::new()
            .h(px(TITLE_BAR_HEIGHT))
            .bg(if self.focus {
                cx.theme().background
            } else {
                workspace_chrome(cx)
            })
            .border_color(gpui::transparent_black())
            .on_close_window(cx.listener(|this, _, window, cx| this.close_window(window, cx)));
        if self.project.workspace.is_none() {
            return bar.into_any_element();
        }
        if self.focus {
            return bar
                .child(div().w(px(row)).h_full().child(self.render_focus_title(cx)))
                .into_any_element();
        }
        let doc = self
            .project
            .active()
            .and_then(|p| self.project.documents.get(p));
        let single = self.project.panes.len() == 1;
        bar.child(
            div()
                .h_full()
                .w(px(row))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_2()
                .pr(px(10.))
                .child(Self::glass_button(
                    "sidebar-toggle",
                    GlassIcon::Layout,
                    "Toggle Side Panel",
                    self.sidebar,
                    |this, _, cx| this.toggle_file_tree(cx),
                    cx,
                ))
                .when(single, |el| el.child(self.render_tabs(0, true, cx)))
                .when(!single, |el| el.child(div().flex_1()))
                .when(self.loading, |el| {
                    el.child(
                        div()
                            .flex_shrink_0()
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child("Loading…"),
                    )
                })
                .when_some(self.available_update.as_ref(), |el, update| {
                    el.child(self.render_update_badge(update, cx))
                })
                .child(self.render_command_bar(cx))
                .child(self.render_focus_button(cx))
                // Markdown opens rendered; the eye flips it to source.
                .when_some(
                    doc.filter(|_| {
                        self.project
                            .active()
                            .is_some_and(|p| buffer::is_markdown(p))
                    })
                    .filter(|doc| !doc.large),
                    |el, doc| {
                        el.child(Self::glass_button(
                            "markdown-preview",
                            GlassIcon::Preview,
                            "Markdown Preview",
                            doc.preview,
                            |this, window, cx| this.toggle_markdown_preview(window, cx),
                            cx,
                        ))
                    },
                )
                .when(self.run_is_busy(cx), |el| {
                    el.child(self.render_run_button(true, cx))
                })
                .when(!self.run_is_busy(cx) && self.can_run_active(cx), |el| {
                    el.child(self.render_run_button(false, cx))
                }),
        )
        .into_any_element()
    }

    /// The command bar in the title bar: where you are (project, branch,
    /// how many files changed) and the way to anywhere else (⌘P).
    fn render_command_bar(&self, cx: &Context<Self>) -> AnyElement {
        let project = self
            .project
            .workspace
            .as_ref()
            .map(|w| name(&w.root))
            .unwrap_or_default();
        let changed = self.project.git_status.len();
        let dark = cx.theme().is_dark();
        div()
            .id("command-bar")
            .role(Role::Button)
            .aria_label("Quick Open")
            .focusable()
            .tab_index(0)
            .w(px(320.))
            .min_w(px(180.))
            .flex_shrink(1.)
            .h(px(30.))
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(10.))
            .pr(px(6.))
            .rounded(px(8.))
            .bg(if dark {
                gpui::white().opacity(0.06)
            } else {
                gpui::black().opacity(0.05)
            })
            .hover(|el| {
                el.bg(if dark {
                    gpui::white().opacity(0.09)
                } else {
                    gpui::black().opacity(0.07)
                })
            })
            .focus_visible(|el| el.border_1().border_color(cx.theme().ring))
            .cursor_pointer()
            .text_size(ui(12.))
            .text_color(cx.theme().muted_foreground)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| this.show_quick_open(false, window, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.show_quick_open(false, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(Self::glass(GlassIcon::QuickOpen, 16., glass_tone(true, cx)))
            .child(
                div()
                    .flex_shrink_0()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().foreground)
                    .child(project),
            )
            .when_some(self.project.git_branch.clone(), |el, branch| {
                el.child("›")
                    .child(Self::glass(GlassIcon::Branch, 12., glass_tone(false, cx)))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(ui(11.))
                            .child(branch),
                    )
            })
            .when(changed > 0, |el| {
                el.child(
                    div()
                        .flex_shrink_0()
                        .text_size(ui(11.))
                        .text_color(git_modified_color(cx))
                        .child(format!("●{changed}")),
                )
            })
            .child(div().flex_1())
            .child(Self::kbd_label(
                if cfg!(target_os = "macos") {
                    "⌘P"
                } else {
                    "Ctrl+P"
                },
                cx,
            ))
            .into_any_element()
    }

    /// "Focus" in the title bar: four corners and the word.
    fn render_focus_button(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .id("focus-toggle")
            .role(Role::Button)
            .aria_label("Focus Mode")
            .focusable()
            .tab_index(0)
            .flex_shrink_0()
            .h(px(28.))
            .px_2()
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .text_size(ui(12.))
            .text_color(cx.theme().muted_foreground)
            .hover(|el| el.text_color(cx.theme().foreground))
            .focus_visible(|el| el.text_color(cx.theme().accent_foreground))
            .tooltip(|window, cx| {
                gpui_component::tooltip::Tooltip::new("Focus Mode")
                    .key_binding(gpui_component::kbd::Kbd::binding_for_action(
                        &ToggleFocus,
                        None,
                        window,
                    ))
                    .build(window, cx)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| this.set_focus(true, window, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.set_focus(true, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(Icon::new(IconName::Maximize).small())
            .child("Focus")
            .into_any_element()
    }

    /// The title row in focus mode: where you are, the one tab, and the way
    /// out.
    fn render_focus_title(&self, cx: &mut Context<Self>) -> AnyElement {
        let pane = self.project.focused;
        let tabs = self
            .project
            .panes
            .get(pane)
            .map(|pane| pane.tabs.clone())
            .unwrap_or_default();
        let active = self.project.panes.get(pane).and_then(|p| p.active.clone());
        let folder = active
            .as_ref()
            .and_then(|path| {
                let root = &self.project.workspace.as_ref()?.root;
                let parent = path.parent()?.strip_prefix(root).ok()?;
                Some(
                    std::iter::once(name(root))
                        .chain(
                            parent
                                .iter()
                                .map(|part| part.to_string_lossy().into_owned()),
                        )
                        .map(|part| format!("{part} / "))
                        .collect::<String>(),
                )
            })
            .unwrap_or_default();
        let index = active
            .as_ref()
            .and_then(|path| tabs.iter().position(|tab| tab == path));
        let others = tabs.len().saturating_sub(1);
        div()
            .h_full()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .pr(px(10.))
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(ui(12.))
                    .text_color(cx.theme().muted_foreground.opacity(0.75))
                    .child(folder)
                    .when_some(active.zip(index), |el, (path, index)| {
                        el.child(self.render_tab(pane, index, &path, cx))
                    })
                    .when(others > 0, |el| {
                        el.child(div().text_size(ui(11.)).child(format!("+{others} tabs")))
                    }),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("focus-exit")
                    .role(Role::Button)
                    .aria_label("Exit Focus Mode")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .text_size(ui(12.))
                    .text_color(cx.theme().muted_foreground)
                    .hover(|el| el.text_color(cx.theme().foreground))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| this.set_focus(false, window, cx)))
                    .child("Exit focus")
                    .child(Self::kbd_label("Esc", cx)),
            )
            .into_any_element()
    }

    /// The title-bar Run: the glass Play, "Run", and the command it starts,
    /// on a paper chip so it reads as the one primary action up there.
    /// While a Run is going the same chip is Stop, with a red square.
    fn render_run_button(&self, busy: bool, cx: &Context<Self>) -> AnyElement {
        let command = if busy {
            None
        } else {
            self.active_run_command()
        };
        let label = if busy { "Stop" } else { "Run" };
        let red = run_stop_color(cx);
        div()
            .id(if busy { "stop" } else { "run" })
            .role(Role::Button)
            .aria_label(label)
            .focusable()
            .tab_index(0)
            .h(px(26.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .pl(px(6.))
            .pr(px(10.))
            .rounded(px(6.))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .cursor_pointer()
            .hover(|el| el.border_color(cx.theme().muted_foreground.opacity(0.5)))
            .focus_visible(|el| el.border_color(cx.theme().ring))
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(label)
                    .key_binding(
                        gpui::Keystroke::parse("cmd-r")
                            .ok()
                            .map(gpui_component::kbd::Kbd::new),
                    )
                    .build(window, cx)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| this.run_or_stop(None, window, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.run_or_stop(None, window, cx);
                    cx.stop_propagation();
                }
            }))
            .when(busy, |el| {
                el.pl(px(9.))
                    .child(div().size(px(8.)).rounded(px(1.5)).bg(red))
            })
            .when(!busy, |el| {
                el.child(Self::glass(GlassIcon::Run, 16., Tone::Blue))
            })
            .child(
                div()
                    .text_size(ui(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .when(busy, |el| el.text_color(red))
                    .child(label),
            )
            .when_some(command, |el, command| {
                el.child(
                    div()
                        .max_w(px(220.))
                        .truncate()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(ui(11.))
                        .text_color(cx.theme().muted_foreground)
                        .child(command),
                )
            })
            .into_any_element()
    }

    /// "Update available" in the title bar, left of the run control. Plain
    /// green text with the arrow, no pill; clicking hands over to the
    /// platform updater.
    pub(super) fn render_update_badge(
        &self,
        update: &AvailableUpdate,
        cx: &Context<Self>,
    ) -> AnyElement {
        let tooltip: SharedString = format!("Folio {} is available", update.version).into();
        div()
            .id("update-available")
            .role(Role::Button)
            .aria_label(tooltip.clone())
            .focusable()
            .tab_index(0)
            .h(px(24.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .text_size(ui(11.))
            .text_color(cx.theme().success)
            .hover(|el| el.opacity(0.75))
            .focus_visible(|el| el.text_color(cx.theme().accent_foreground))
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| this.open_available_update(window, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.open_available_update(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(Icon::new(IconName::ArrowUp).xsmall())
            .child("Update available")
            .into_any_element()
    }

    pub(super) fn render_disk_notice(
        &self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let removed = self.project.disk_notice.get(&path) == Some(&DiskNotice::Removed);
        div()
            .px_4()
            .py_1()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .text_size(ui(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(if removed {
                        "This file was removed on disk"
                    } else {
                        "This file changed on disk"
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(!removed, |el| {
                        let path = path.clone();
                        el.child(
                            Button::new("reload-disk")
                                .text()
                                .xsmall()
                                .label("Reload")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.reload_from_disk(path.clone(), window, cx);
                                })),
                        )
                    })
                    .child({
                        let path = path.clone();
                        Button::new("keep-disk")
                            .text()
                            .xsmall()
                            .label("Keep")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.project.disk_notice.remove(&path);
                                cx.notify();
                            }))
                    }),
            )
    }

    pub(super) fn render_workspace(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let split = self.project.panes.len() > 1;
        let focus = self.focus;
        div()
            .id("workspace")
            .size_full()
            .flex()
            // The side panel sits on the window ground; its right edge is
            // the gap before the editor sheet, and dragging it resizes.
            .when(self.sidebar && !focus, |el| {
                el.child(self.render_side_panel(cx)).child(
                    div()
                        .id("sidebar-resize")
                        .w(px(PANEL_GAP))
                        .h_full()
                        .flex_shrink_0()
                        .cursor_col_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, _| this.resizing = true),
                        ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .bg(cx.theme().background)
                    // The editor sheet. In focus mode it is the whole window.
                    .when(!focus, |el| {
                        el.rounded(px(WORKSPACE_RADIUS))
                            .border_1()
                            .border_color(cx.theme().border)
                    })
                    .child(if split {
                        let vertical = self.project.split_axis == SplitAxis::Vertical;
                        div()
                            .id("editor-panes")
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .w_full()
                            .flex()
                            .when(vertical, |el| el.flex_col())
                            .child(
                                div()
                                    .when(vertical, |el| {
                                        el.h(relative(self.pane_ratio)).w_full().min_h_0()
                                    })
                                    .when(!vertical, |el| {
                                        el.w(relative(self.pane_ratio)).h_full().min_w_0()
                                    })
                                    .child(self.render_pane(0, window, cx)),
                            )
                            .child(
                                div()
                                    .id("pane-resize")
                                    .when(vertical, |el| {
                                        el.h(px(5.))
                                            .w_full()
                                            .flex()
                                            .items_center()
                                            .cursor_row_resize()
                                    })
                                    .when(!vertical, |el| {
                                        el.w(px(5.))
                                            .h_full()
                                            .flex()
                                            .justify_center()
                                            .cursor_col_resize()
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, _| this.resizing_pane = true),
                                    )
                                    .child(if vertical {
                                        div()
                                            .h(px(1.))
                                            .w_full()
                                            .bg(sidebar_rule(cx))
                                            .into_any_element()
                                    } else {
                                        div()
                                            .w(px(1.))
                                            .h_full()
                                            .bg(sidebar_rule(cx))
                                            .into_any_element()
                                    }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .min_h_0()
                                    .when(vertical, |el| el.w_full())
                                    .when(!vertical, |el| el.h_full())
                                    .child(self.render_pane(1, window, cx)),
                            )
                            .into_any_element()
                    } else {
                        self.render_pane(0, window, cx)
                    })
                    // The terminal drawer hangs under the content
                    // column alone; the sidebar keeps its full
                    // height, and the editor above shrinks.
                    .when(self.terminal_open && !self.terminal_right && !focus, |el| {
                        el.child(self.render_terminal_dock(window, cx))
                    }),
            )
            // On the right the terminal is a sheet of its own beside the
            // editor's. Focus mode hides it; the shell keeps running.
            .when(self.terminal_open && self.terminal_right && !focus, |el| {
                el.child(self.render_terminal_dock(window, cx))
            })
            .into_any_element()
    }

    /// The page viewer for a file that is too large for the editor, or is not
    /// UTF-8. It is read-only by construction: there is no Document behind it.
    fn render_large_file(&self, large: &LargeFile, cx: &mut Context<Self>) -> AnyElement {
        let pages = large.pages.max(1);
        let page = (large.page as u64).min(pages - 1) as usize;
        let mut header = format!(
            "{} · {} · read-only · page {}/{}",
            name(&large.path),
            preview::size_label(large.size),
            page + 1,
            pages
        );
        if large.loading {
            header.push_str(" · reading…");
        }
        let body = if let Some(error) = &large.error {
            div()
                .p_4()
                .text_size(ui(12.))
                .text_color(cx.theme().warning)
                .child(error.clone())
                .into_any_element()
        } else if large.text.is_empty() && large.loading {
            div()
                .p_4()
                .text_size(ui(12.))
                .text_color(cx.theme().muted_foreground)
                .child("Reading page…")
                .into_any_element()
        } else {
            let text = TextView::markdown(
                format!("large-preview:{}:{}", large.path.to_string_lossy(), page),
                preview::text_fence(&large.text),
            )
            .selectable(true)
            .scrollable(true)
            .text_size(ui(12.))
            .font_family(self.code_font())
            .h_full()
            .w_full()
            .p_4();
            text.into_any_element()
        };
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_size(ui(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(div().truncate().child(header)),
            )
            .child(div().flex_1().min_h_0().w_full().child(body))
            .child(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(ui(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(Self::icon_button(
                        "large-prev",
                        IconName::ChevronLeft,
                        "Previous page",
                        move |this, _, cx| this.load_large_page(page.saturating_sub(1), cx),
                        cx,
                    ))
                    .child(format!("{} / {}", page + 1, pages))
                    .child(Self::icon_button(
                        "large-next",
                        IconName::ChevronRight,
                        "Next page",
                        move |this, _, cx| this.load_large_page(page + 1, cx),
                        cx,
                    ))
                    .child(div().flex_1())
                    .child(if large.binary {
                        "hex · editing disabled"
                    } else {
                        "text · editing disabled"
                    }),
            )
            .into_any_element()
    }

    pub(super) fn render_markdown_bar(&self, preview: bool, cx: &Context<Self>) -> AnyElement {
        div()
            .flex_shrink_0()
            .px_3()
            .py_1()
            .flex()
            .items_center()
            .justify_end()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(self.diff_choice(
                "md-preview",
                "Preview",
                preview,
                |this, window, cx| this.set_markdown_preview(true, window, cx),
                cx,
            ))
            .child(self.diff_choice(
                "md-source",
                "Source",
                !preview,
                |this, window, cx| this.set_markdown_preview(false, window, cx),
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn render_markdown_preview(
        &self,
        path: &Path,
        text: SharedString,
        cx: &Context<Self>,
    ) -> AnyElement {
        let mut style = TextViewStyle::default()
            .paragraph_gap(rems(0.75))
            .heading_font_size(|level, base| match level {
                1 => base * 1.6,
                2 => base * 1.3,
                3 => base * 1.1,
                _ => base,
            });
        style.heading_base_font_size = px(15.);
        style.highlight_theme = cx.theme().highlight_theme.clone();
        style.is_dark = cx.theme().is_dark();
        // TextView resolves a plain path relative to the process, not to the
        // note. Turn every existing relative image into a file:// URL first.
        let text = localize_markdown_images(&text, path.parent());
        let base = path.parent().map(Path::to_path_buf);
        let mut view =
            TextView::markdown(format!("markdown-preview:{}", path.to_string_lossy()), text)
                .selectable(true)
                .scrollable(true)
                .style(style)
                .on_link_click(move |url, _event, window, cx| {
                    // The handler is Send + Sync and cannot hold the view; the
                    // global parks a weak one for exactly this call.
                    let Some(target) = cx.try_global::<MarkdownLinkTarget>() else {
                        return;
                    };
                    let folio = target.0.clone();
                    let base = base.clone();
                    let url = url.to_string();
                    let _ = folio.update(cx, |this, cx| {
                        this.open_markdown_link(base.as_deref(), &url, window, cx);
                    });
                })
                .h_full()
                .w_full()
                .p_6();
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mdx"))
        {
            view = view.markdown_mdx();
        }
        view.into_any_element()
    }

    /// `⌘`-scroll, pinch, or the footer buttons. 100% means fit-to-window:
    /// above it the image is natural size × factor and the pane scrolls.
    pub(super) fn zoom_image(&mut self, delta: f32, cx: &mut Context<Self>) {
        let next = (self.project.image_zoom + delta).clamp(0.25, 8.0);
        if (next - self.project.image_zoom).abs() > f32::EPSILON {
            self.project.image_zoom = next;
            cx.notify();
        }
    }

    pub(super) fn reset_image_view(&mut self, cx: &mut Context<Self>) {
        self.project.image_zoom = 1.0;
        self.project.image_scroll = ScrollHandle::new();
        cx.notify();
    }

    /// Follow a Markdown link. Relative paths resolve against the note that
    /// contains them and open in Folio; http(s), mail and data URLs go to the
    /// system opener; a file outside the project opens in the default app.
    pub(super) fn open_markdown_link(
        &mut self,
        base: Option<&Path>,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let url = url
            .trim()
            .trim_matches(|character| character == '<' || character == '>');
        if url.is_empty() || url.starts_with('#') {
            return;
        }
        if is_web_url(url) {
            cx.open_url(url);
            return;
        }
        let path = url.strip_prefix("file://").unwrap_or(url);
        let path = path.split(['#', '?']).next().unwrap_or(path);
        let path = decode_percent(path);
        if path.is_empty() {
            return;
        }
        let target = if Path::new(&path).is_absolute() {
            PathBuf::from(path)
        } else if let Some(base) = base {
            base.join(path)
        } else {
            return;
        };
        if let Some(workspace) = self.project.workspace.clone()
            && let Ok(resolved) = workspace.resolve(&target)
            && resolved.is_file()
        {
            self.open_file(resolved, window, cx);
            return;
        }
        // Outside the project (or no longer a file): hand it to the OS rather
        // than pretending it is a Folio document.
        if target.is_file() {
            cx.open_url(&format!("file://{}", target.display()));
        }
    }

    pub(super) fn render_pane(
        &self,
        index: usize,
        _: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane = self.project.panes.get(index).cloned().unwrap_or_default();
        let focused = self.project.focused == index;
        let showing_diff = self
            .project
            .diff
            .as_ref()
            .is_some_and(|diff| pane.active.as_ref() == Some(&diff.path));
        let locked = pane
            .active
            .as_ref()
            .is_some_and(|path| self.project.read_only.contains(path));
        let doc = (!showing_diff)
            .then(|| {
                pane.active
                    .as_ref()
                    .and_then(|path| self.project.documents.get(path))
            })
            .flatten();
        let image = (!showing_diff)
            .then(|| {
                self.project
                    .image
                    .as_ref()
                    .filter(|(path, _)| pane.active.as_ref() == Some(path))
            })
            .flatten();
        let large = (!showing_diff)
            .then(|| {
                self.project
                    .large_file
                    .as_ref()
                    .filter(|large| pane.active.as_ref() == Some(&large.path))
            })
            .flatten();
        let blame = focused.then(|| self.blame_text(cx)).flatten();
        let split = self.project.panes.len() > 1;
        let empty_solo = !split;
        div()
            .id(("pane", index))
            .size_full()
            .flex()
            .flex_col()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    if this.project.focused != index {
                        this.focus_pane(index, window, cx);
                    }
                }),
            )
            .on_drag_move(cx.listener(move |this, _: &DragMoveEvent<TabDrag>, _, cx| {
                let end = this
                    .project
                    .panes
                    .get(index)
                    .map(|pane| pane.tabs.len())
                    .unwrap_or(0);
                if this.tab_drop != Some((index, end)) {
                    this.tab_drop = Some((index, end));
                    cx.notify();
                }
            }))
            .on_drop(cx.listener(move |this, drag: &TabDrag, window, cx| {
                let end = this
                    .project
                    .panes
                    .get(index)
                    .map(|pane| pane.tabs.len())
                    .unwrap_or(0);
                this.move_tab_to(&drag.path, index, end, cx);
                this.focus_pane(this.project.focused, window, cx);
            }))
            // One pane: its tabs are in the title bar. Split: each pane
            // keeps a strip of its own so it is clear which tab is where.
            .when(self.project.panes.len() > 1 && !self.focus, |el| {
                el.child(self.render_tabs(index, false, cx))
            })
            .when(
                !self.focus && !showing_diff && pane.active.is_some(),
                |el| el.child(self.render_pane_header(index, cx)),
            )
            .when(self.focus && focused && !showing_diff, |el| {
                el.child(self.render_focus_heading(index, cx))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_col()
                    // Focus mode reads in a column, not across the window.
                    .when(self.focus, |el| {
                        el.max_w(px(FOCUS_COLUMN)).mx_auto().pb(px(96.))
                    })
                    .when(showing_diff, |el| el.child(self.render_diff(cx)))
                    .when_some(doc, |el, doc| {
                        el.when(
                            pane.active
                                .as_ref()
                                .is_some_and(|path| self.project.disk_notice.contains_key(path)),
                            |el| {
                                el.child(self.render_disk_notice(
                                    pane.active.clone().unwrap_or_default(),
                                    cx,
                                ))
                            },
                        )
                        .when(doc.large, |el| {
                            el.child(
                                div()
                                    .px_4()
                                    .py_1()
                                    .text_size(ui(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Large file · syntax highlighting off"),
                            )
                        })
                        .when(
                            !doc.large
                                && pane
                                    .active
                                    .as_ref()
                                    .is_some_and(|path| buffer::is_markdown(path)),
                            |el| el.child(self.render_markdown_bar(doc.preview, cx)),
                        )
                        .when(doc.preview, |el| {
                            let path = pane.active.clone().unwrap_or_default();
                            let text = doc.editor.read(cx).value();
                            let menu_path = path.clone();
                            el.child(
                                div()
                                    .id("markdown-preview")
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .overflow_hidden()
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(
                                            move |this, event: &MouseDownEvent, window, cx| {
                                                this.open_menu(
                                                    MenuTarget::Editor {
                                                        path: menu_path.clone(),
                                                    },
                                                    event.position,
                                                    window,
                                                    cx,
                                                );
                                                cx.stop_propagation();
                                            },
                                        ),
                                    )
                                    .child(self.render_markdown_preview(&path, text, cx)),
                            )
                        })
                        .when(!doc.preview, |el| {
                            el.child({
                                let base = doc.editor.read(cx).base_state().clone();
                                let typeface = self.code_typeface();
                                let size = px(self.settings.code_font_size);
                                base.update(cx, |state, _| {
                                    state.set_context_menu_enabled(false);
                                    state.set_font(typeface, size);
                                });
                                let menu_path = pane.active.clone().unwrap_or_default();
                                div()
                                    .key_context("FolioEditor")
                                    .size_full()
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(
                                            move |this, event: &MouseDownEvent, window, cx| {
                                                this.open_menu(
                                                    MenuTarget::Editor {
                                                        path: menu_path.clone(),
                                                    },
                                                    event.position,
                                                    window,
                                                    cx,
                                                );
                                                cx.stop_propagation();
                                            },
                                        ),
                                    )
                                    .child(
                                        Input::from_base(&base)
                                            .appearance(false)
                                            .bordered(false)
                                            .focus_bordered(false)
                                            .readonly(
                                                self.saving
                                                    || self.loading
                                                    || self.prompting
                                                    || locked,
                                            )
                                            .text_size(px(self.settings.code_font_size))
                                            .font(self.code_typeface())
                                            .line_height(gpui::relative(1.6))
                                            .rounded_none()
                                            .h_full(),
                                    )
                            })
                        })
                        .when_some(
                            blame.filter(|_| !doc.preview),
                            |el, text| {
                                el.child(
                                    div()
                                        .px_4()
                                        .py_1()
                                        .text_size(ui(11.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(text),
                                )
                            },
                        )
                    })
                    .when_some(image, |el, (path, image)| {
                        let zoom = self.project.image_zoom;
                        let fitted = (zoom - 1.0).abs() < f32::EPSILON;
                        let natural = image.size(0);
                        let width = u32::from(natural.width) as f32;
                        let height = u32::from(natural.height) as f32;
                        let viewer = div()
                            .id("image-viewer")
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .on_pinch(cx.listener(|this, event: &PinchEvent, _, cx| {
                                this.zoom_image(event.delta, cx);
                                cx.stop_propagation();
                            }))
                            .on_scroll_wheel(cx.listener(
                                |this, event: &ScrollWheelEvent, _, cx| {
                                    if !(event.modifiers.platform || event.modifiers.control) {
                                        return;
                                    }
                                    let delta = match &event.delta {
                                        ScrollDelta::Lines(delta) => delta.y,
                                        ScrollDelta::Pixels(delta) => f32::from(delta.y),
                                    };
                                    if delta != 0. {
                                        this.zoom_image(delta.signum() * 0.1, cx);
                                    }
                                    cx.stop_propagation();
                                },
                            ));
                        let viewer = if fitted {
                            viewer.child(
                                div().size_full().p_6().child(
                                    img(image.clone())
                                        .size_full()
                                        .object_fit(ObjectFit::Contain),
                                ),
                            )
                        } else {
                            viewer
                                .overflow_scroll()
                                .track_scroll(&self.project.image_scroll)
                                .child(
                                    div()
                                        .min_w_full()
                                        .min_h_full()
                                        .p_6()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            img(image.clone())
                                                .w(px(width * zoom))
                                                .h(px(height * zoom))
                                                .object_fit(ObjectFit::Fill),
                                        ),
                                )
                        };
                        el.child(viewer).child(
                            div()
                                .px_4()
                                .py_2()
                                .flex()
                                .items_center()
                                .gap_3()
                                .text_size(ui(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child(div().flex_1().min_w_0().truncate().child(format!(
                                    "{} · {} × {} · static preview",
                                    name(path),
                                    u32::from(natural.width),
                                    u32::from(natural.height)
                                )))
                                .child(
                                    div()
                                        .flex()
                                        .flex_shrink_0()
                                        .items_center()
                                        .gap_1()
                                        .child(Self::icon_button(
                                            "image-zoom-out",
                                            IconName::Minus,
                                            "Zoom out",
                                            |this, _, cx| this.zoom_image(-0.25, cx),
                                            cx,
                                        ))
                                        .child(
                                            div()
                                                .w(px(38.))
                                                .text_right()
                                                .child(format!("{:.0}%", zoom * 100.)),
                                        )
                                        .child(Self::icon_button(
                                            "image-zoom-in",
                                            IconName::Plus,
                                            "Zoom in",
                                            |this, _, cx| this.zoom_image(0.25, cx),
                                            cx,
                                        ))
                                        .child(Self::icon_button(
                                            "image-fit",
                                            IconName::Maximize,
                                            "Fit to window",
                                            |this, _, cx| this.reset_image_view(cx),
                                            cx,
                                        )),
                                ),
                        )
                    })
                    .when_some(large, |el, large| {
                        el.child(self.render_large_file(large, cx))
                    })
                    .when(
                        !showing_diff && doc.is_none() && image.is_none() && large.is_none(),
                        |el| {
                            el.child(
                                div()
                                    .size_full()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .justify_center()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_size(if empty_solo { ui(24.) } else { ui(15.) })
                                            .text_color(cx.theme().accent_foreground)
                                            .child(if empty_solo {
                                                "Some room to read a little code."
                                            } else {
                                                "Open a file"
                                            }),
                                    )
                                    .when(empty_solo, |el| {
                                        el.child(
                                            div()
                                                .text_size(ui(12.))
                                                .text_color(cx.theme().muted_foreground)
                                                .child("Pick a file on the left, or press ⌘ P"),
                                        )
                                    }),
                            )
                        },
                    ),
            )
            .into_any_element()
    }
}

/// Rewrite relative Markdown image targets to `file://` URLs when the image
/// exists beside the note. Reference-style images and raw HTML `<img>` are
/// left alone; the common inline form is what a README uses.
fn localize_markdown_images(text: &str, base: Option<&Path>) -> String {
    let Some(base) = base else {
        return text.to_string();
    };
    static IMAGE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let image = IMAGE.get_or_init(|| {
        regex::Regex::new(r#"!\[([^\]]*)\]\(\s*(<[^>]+>|[^)\s]+)(?:\s+"[^"]*")?\s*\)"#)
            .expect("markdown image pattern")
    });
    image
        .replace_all(text, |captures: &regex::Captures<'_>| {
            let alt = &captures[1];
            let raw = captures[2].trim_matches(|character| character == '<' || character == '>');
            if raw.is_empty()
                || is_web_url(raw)
                || raw.starts_with("file://")
                || Path::new(raw).is_absolute()
            {
                return captures[0].to_string();
            }
            let path = base.join(decode_percent(raw));
            if path.is_file() {
                format!("![{alt}](file://{})", path.display())
            } else {
                captures[0].to_string()
            }
        })
        .into_owned()
}

fn is_web_url(url: &str) -> bool {
    url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("mailto:")
        || url.starts_with("data:")
        || url.starts_with('#')
}

/// Minimal `%XX` decoding for the characters a URL-encoded file name can
/// contain. Invalid escapes are left as written.
fn decode_percent(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16)
        {
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn markdown_images_resolve_relative_to_the_note() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join("docs");
        std::fs::create_dir(&docs).unwrap();
        let image = docs.join("diagram.png");
        let spaced = docs.join("my image.png");
        std::fs::write(&image, b"png").unwrap();
        std::fs::write(&spaced, b"png").unwrap();

        let text = "![one](diagram.png)\n\
                    ![two](missing.png)\n\
                    ![three](https://example.com/x.png)\n\
                    ![four](<my image.png>)\n\
                    ![five](my%20image.png)\n";
        let out = localize_markdown_images(text, Some(&docs));
        assert!(
            out.contains(&format!("file://{}", image.display())),
            "{out}"
        );
        assert!(
            out.contains(&format!("file://{}", spaced.display())),
            "{out}"
        );
        assert!(out.contains("![two](missing.png)"), "{out}");
        assert!(out.contains("https://example.com/x.png"), "{out}");
        assert!(out.contains("![five](file://"), "{out}");
    }

    #[test]
    fn percent_decoding_only_touches_valid_escapes() {
        assert_eq!(decode_percent("a%20b"), "a b");
        assert_eq!(decode_percent("100%"), "100%");
        assert_eq!(decode_percent("%ZZ"), "%ZZ");
        assert_eq!(decode_percent("%E4%B8%AD"), "中");
    }
}
