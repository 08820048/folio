use super::*;

impl Folio {
    /// The neighbour this pane's arrows would open, if that step is still
    /// inside the strip. The ends do not wrap.
    pub(super) fn tab_step_target(&self, pane: usize, delta: isize) -> Option<PathBuf> {
        let pane = self.project.panes.get(pane)?;
        let index = pane
            .active
            .as_ref()
            .and_then(|active| pane.tabs.iter().position(|tab| tab == active))?;
        pane.tabs.get(index.checked_add_signed(delta)?).cloned()
    }

    /// Open the tab one step left or right of the one this pane is showing.
    pub(super) fn step_tab(
        &mut self,
        pane: usize,
        delta: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.tab_step_target(pane, delta) else {
            return;
        };
        self.project.focused = pane;
        self.open_file(path, window, cx);
        self.scroll_to_active_tab();
    }

    /// Scroll the focused strip so its active tab is visible. The handle is
    /// attached to the scrolling container in `render_tabs`; a new tab opened
    /// beyond the right edge is otherwise drawn off-screen.
    pub(super) fn scroll_to_active_tab(&self) {
        let index = self
            .project
            .focused
            .min(self.project.panes.len().saturating_sub(1));
        let Some(pane) = self.project.panes.get(index) else {
            return;
        };
        let Some(active) = pane.active.as_ref() else {
            return;
        };
        if let Some(position) = pane.tabs.iter().position(|tab| tab == active) {
            pane.tab_scroll.scroll_to_item(position);
        }
    }

    /// Put back the last tab this project closed. A path that is no longer on
    /// disk is skipped rather than reopening an editor over nothing.
    pub(super) fn reopen_closed_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.project_loading || self.prompting {
            return;
        }
        let mut missing = false;
        while let Some((pane, path)) = self.project.closed_tabs.pop() {
            if !path.is_file() {
                missing = true;
                continue;
            }
            self.project.focused = pane.min(self.project.panes.len().saturating_sub(1));
            self.open_file(path, window, cx);
            self.scroll_to_active_tab();
            return;
        }
        if missing {
            self.warn("The closed file is no longer on disk", cx);
        } else {
            self.toast("No closed tabs", cx);
        }
    }

    /// The two arrows on the left of a tab strip: previous / next, no wrap.
    pub(super) fn render_tab_step(
        &self,
        pane: usize,
        delta: isize,
        cx: &Context<Self>,
    ) -> AnyElement {
        let (icon, label) = if delta < 0 {
            (IconName::ChevronLeft, "Previous Tab")
        } else {
            (IconName::ChevronRight, "Next Tab")
        };
        let enabled = self.tab_step_target(pane, delta).is_some();
        div()
            .id((if delta < 0 { "tab-prev" } else { "tab-next" }, pane))
            .role(Role::Button)
            .aria_label(label)
            .when(enabled, |el| el.focusable().tab_index(0).cursor_pointer())
            .size(px(20.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .text_color(if enabled {
                cx.theme().muted_foreground
            } else {
                cx.theme().muted_foreground.opacity(0.4)
            })
            .when(enabled, |el| {
                el.hover(|el| el.text_color(cx.theme().foreground))
                    .focus_visible(|el| el.text_color(cx.theme().accent_foreground))
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(label).build(window, cx)
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.step_tab(pane, delta, window, cx);
                    }))
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.step_tab(pane, delta, window, cx);
                            cx.stop_propagation();
                        }
                    }))
            })
            .child(Icon::new(icon).small())
            .into_any_element()
    }

    /// The open files, as a strip above the editor. Always drawn so the
    /// split button on the right — Zed's entry — is reachable with one file.
    /// No hairline under the strip: it is the same surface as the code, not
    /// a second chrome bar inside the rounded card.
    pub(super) fn render_tabs(&self, pane: usize, cx: &Context<Self>) -> AnyElement {
        let tabs = self
            .project
            .panes
            .get(pane)
            .map(|pane| pane.tabs.as_slice())
            .unwrap_or(&[]);
        let scroll = self
            .project
            .panes
            .get(pane)
            .map(|pane| pane.tab_scroll.clone())
            .unwrap_or_default();
        let end = tabs.len();
        div()
            .id(("tabs", pane))
            .debug_selector(|| format!("editor-tabs-{pane}"))
            .flex_shrink_0()
            .w_full()
            .h(px(TAB_HEIGHT))
            .flex()
            .items_center()
            .child(
                div()
                    .h_full()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .pl(px(2.))
                    .child(self.render_tab_step(pane, -1, cx))
                    .child(self.render_tab_step(pane, 1, cx)),
            )
            // The strip itself scrolls horizontally: with enough tabs, the
            // ones past the right edge would otherwise be unreachable except
            // by the two arrows. The arrows and the split button stay fixed.
            .child(
                div()
                    .id(("tab-scroll", pane))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .overflow_x_scroll()
                    .track_scroll(&scroll)
                    .children(
                        tabs.iter()
                            .enumerate()
                            .flat_map(|(index, path)| {
                                // A caret sits where the dragged tab would land,
                                // so the drop is not a guess.
                                let caret = (self.tab_drop == Some((pane, index)))
                                    .then(|| Self::tab_caret(cx));
                                caret
                                    .into_iter()
                                    .chain(std::iter::once(self.render_tab(pane, index, path, cx)))
                            })
                            .chain(
                                (self.tab_drop == Some((pane, end))).then(|| Self::tab_caret(cx)),
                            ),
                    )
                    .child(
                        div()
                            .id(("tab-rest", pane))
                            .flex_1()
                            .min_w(px(24.))
                            .h_full()
                            .on_drop(cx.listener(move |this, drag: &TabDrag, window, cx| {
                                this.move_tab_to(&drag.path, pane, end, cx);
                                this.focus_pane(this.project.focused, window, cx);
                            }))
                            .on_drag_move(cx.listener(
                                move |this, _: &DragMoveEvent<TabDrag>, _, cx| {
                                    if this.tab_drop != Some((pane, end)) {
                                        this.tab_drop = Some((pane, end));
                                        cx.notify();
                                    }
                                },
                            )),
                    ),
            )
            .child(self.render_split_button(pane, cx))
            .into_any_element()
    }

    /// Zed puts New / Split / Zoom on the tab bar's right. Folio only
    /// needs Split: the button opens the four-direction menu.
    pub(super) fn render_split_button(&self, pane: usize, cx: &Context<Self>) -> AnyElement {
        div()
            .id(("pane-split", pane))
            .role(Role::Button)
            .aria_label("Split Pane")
            .focusable()
            .tab_index(0)
            .size(px(24.))
            .mr_1()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(cx.theme().muted_foreground)
            .hover(|el| el.text_color(cx.theme().foreground))
            .focus_visible(|el| el.text_color(cx.theme().accent_foreground))
            .tooltip(|window, cx| {
                gpui_component::tooltip::Tooltip::new("Split Pane").build(window, cx)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.project.focused = pane;
                    this.open_menu(MenuTarget::Split { pane }, event.position, window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.project.focused = pane;
                    let size = window.viewport_size();
                    this.open_menu(
                        MenuTarget::Split { pane },
                        point(size.width - px(16.), px(72.)),
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }
            }))
            .child(Icon::new(FolioIcon::GapHorizontal).small())
            .into_any_element()
    }

    /// The insertion caret drawn between tabs while one is being dragged.
    pub(super) fn tab_caret(cx: &Context<Self>) -> AnyElement {
        div()
            .w(px(2.))
            .h_full()
            .flex_shrink_0()
            .bg(cx.theme().accent_foreground)
            .into_any_element()
    }

    /// One tab: the file's name, a mark when it has edits on disk to match,
    /// and the way to close it.
    pub(super) fn render_tab(
        &self,
        pane: usize,
        index: usize,
        path: &PathBuf,
        cx: &Context<Self>,
    ) -> AnyElement {
        let selected = self
            .project
            .panes
            .get(pane)
            .is_some_and(|pane| pane.active.as_ref() == Some(path));
        let dirty = self
            .project
            .documents
            .get(path)
            .is_some_and(|doc| doc.dirty);
        let open = path.clone();
        let close = path.clone();
        let menu_path = path.clone();
        div()
            .id(format!("tab-{pane}-{index}"))
            .role(Role::Button)
            .aria_label(name(path))
            .aria_selected(selected)
            .h_full()
            .min_w_0()
            .max_w(px(200.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .border_r_1()
            .border_color(sidebar_rule(cx))
            .cursor_pointer()
            .text_size(ui(11.))
            .text_color(if selected {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .when(selected, |el| el.bg(cx.theme().list_active))
            .hover(|el| el.bg(cx.theme().list_hover))
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_file(open.clone(), window, cx)),
            )
            // Dragging a tab decides where it lands from which half of this
            // one the pointer is on, the way an insertion caret reads.
            // This only drives the caret. Where the tab actually lands comes
            // from the drop itself, so a move event that never arrives costs a
            // caret and nothing else.
            .on_drag_move(cx.listener(move |this, _: &DragMoveEvent<TabDrag>, _, cx| {
                if this.tab_drop != Some((pane, index)) {
                    this.tab_drop = Some((pane, index));
                    cx.notify();
                }
            }))
            .on_drag(
                TabDrag {
                    path: path.clone(),
                    label: name(path).into(),
                },
                |drag, _, _, cx| {
                    cx.new(|_| TabDragPreview {
                        label: drag.label.clone(),
                    })
                },
            )
            .on_drop(cx.listener(move |this, drag: &TabDrag, window, cx| {
                this.move_tab_to(&drag.path, pane, index, cx);
                this.focus_pane(this.project.focused, window, cx);
            }))
            // The menu acts on the tab that was clicked, which is not
            // necessarily the one showing.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_menu(
                        MenuTarget::Tab {
                            path: menu_path.clone(),
                            index,
                        },
                        event.position,
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .child(div().flex_1().min_w_0().truncate().child(name(path)))
            .when(dirty, |el| {
                el.child(
                    Icon::new(IconName::Asterisk)
                        .xsmall()
                        .text_color(cx.theme().warning),
                )
            })
            .child(
                div()
                    .id(format!("tab-close-{pane}-{index}"))
                    .role(Role::Button)
                    .aria_label("Close")
                    .size(px(14.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(cx.theme().muted_foreground)
                    .hover(|el| el.text_color(cx.theme().foreground))
                    // Without this the tab underneath would take the same
                    // click and reopen the file being closed.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.request(Next::CloseTabs(vec![close.clone()]), window, cx)
                    }))
                    .child(Icon::new(IconName::Close).xsmall()),
            )
            .into_any_element()
    }

    /// The active file's changes, in place of the editor.
    pub(super) fn render_diff(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(view) = self.project.diff.as_ref() else {
            return div().into_any_element();
        };
        let added = view
            .lines
            .iter()
            .filter(|line| line.change == diff::Change::Added)
            .count();
        let removed = view
            .lines
            .iter()
            .filter(|line| line.change == diff::Change::Removed)
            .count();
        let summary = if view.loading {
            "Reading…".to_string()
        } else if let Some(error) = &view.error {
            error.clone()
        } else if view.lines.is_empty() {
            format!("No changes against {}", self.diff_base())
        } else if view.untracked {
            format!("Not tracked by git yet · {added} added")
        } else {
            format!("+{added} −{removed}")
        };
        let branch = self.diff_branch.as_deref().unwrap_or("main");
        let split = self.diff_layout == DiffLayout::Split;
        let reviewed = self.project.reviewed.contains(&view.path);
        let reviewed_path = view.path.clone();
        let count = if split {
            view.split.len()
        } else {
            view.lines.len()
        };
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            // This surface is the app's own, so it gets the app's menu rather
            // than the native one the editor brings with it.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    let Some(path) = this.project.active().cloned() else {
                        return;
                    };
                    this.open_menu(MenuTarget::Changes { path }, event.position, window, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_size(ui(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(self.relative_path(&view.path)),
                    )
                    .child(summary)
                    .child(self.diff_choice(
                        "diff-head",
                        "HEAD",
                        self.diff_against == DiffAgainst::Head,
                        |this, window, cx| this.set_diff_against(DiffAgainst::Head, window, cx),
                        cx,
                    ))
                    .child(self.diff_choice(
                        "diff-branch",
                        branch,
                        self.diff_against == DiffAgainst::Branch,
                        |this, window, cx| this.set_diff_against(DiffAgainst::Branch, window, cx),
                        cx,
                    ))
                    .child(self.diff_choice(
                        "diff-unified",
                        "Unified",
                        !split,
                        |this, _, cx| this.set_diff_layout(DiffLayout::Unified, cx),
                        cx,
                    ))
                    .child(self.diff_choice(
                        "diff-split",
                        "Split",
                        split,
                        |this, _, cx| this.set_diff_layout(DiffLayout::Split, cx),
                        cx,
                    ))
                    .child(self.diff_choice(
                        "diff-reviewed",
                        if reviewed {
                            "Reviewed"
                        } else {
                            "Mark reviewed"
                        },
                        reviewed,
                        move |this, _, cx| this.toggle_reviewed(&reviewed_path, cx),
                        cx,
                    ))
                    .child(Self::icon_button(
                        "diff-prev-file",
                        IconName::ChevronUp,
                        "Previous changed file",
                        |this, window, cx| this.move_change_file(false, window, cx),
                        cx,
                    ))
                    .child(Self::icon_button(
                        "diff-next-file",
                        IconName::ChevronDown,
                        "Next changed file",
                        |this, window, cx| this.move_change_file(true, window, cx),
                        cx,
                    ))
                    .child(Self::icon_button(
                        "diff-prev-hunk",
                        IconName::ArrowUp,
                        "Previous hunk",
                        |this, _, cx| this.move_hunk(false, cx),
                        cx,
                    ))
                    .child(Self::icon_button(
                        "diff-next-hunk",
                        IconName::ArrowDown,
                        "Next hunk",
                        |this, _, cx| this.move_hunk(true, cx),
                        cx,
                    ))
                    .child(Self::icon_button(
                        "diff-close",
                        IconName::Close,
                        "Hide file changes",
                        |this, window, cx| this.toggle_diff(window, cx),
                        cx,
                    )),
            )
            .when(count > 0, |el| {
                el.child(
                    uniform_list(
                        "diff",
                        count,
                        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|index| this.render_diff_list_row(index, cx))
                                .collect()
                        }),
                    )
                    .track_scroll(&view.scroll)
                    .flex_1(),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_diff_list_row(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        if self.diff_layout == DiffLayout::Split {
            self.render_split_row(index, cx)
        } else {
            self.render_diff_row(index, cx)
        }
    }

    pub(super) fn render_split_row(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let Some(row) = self
            .project
            .diff
            .as_ref()
            .and_then(|view| view.split.get(index))
            .cloned()
        else {
            return div().into_any_element();
        };
        if let Some(hunk) = row.hunk {
            return div()
                .w_full()
                .h(px(24.))
                .px_4()
                .flex()
                .items_center()
                .bg(cx.theme().list_hover)
                .text_size(ui(11.))
                .text_color(cx.theme().muted_foreground)
                .child(div().min_w_0().truncate().child(hunk))
                .into_any_element();
        }
        div()
            .w_full()
            .h(px(22.))
            .flex()
            .child(self.render_split_cell(index, row.old.as_ref(), true, cx))
            .child(self.render_split_cell(index, row.new.as_ref(), false, cx))
            .into_any_element()
    }

    pub(super) fn render_split_cell(
        &self,
        index: usize,
        cell: Option<&diff::SplitCell>,
        old: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let (background, marker, number, text) = match cell {
            Some(cell) => {
                let background = match cell.change {
                    diff::Change::Added => Some(cx.theme().success.opacity(0.16)),
                    diff::Change::Removed => Some(cx.theme().danger.opacity(0.16)),
                    _ => None,
                };
                let marker = match cell.change {
                    diff::Change::Added => "+",
                    diff::Change::Removed => "−",
                    _ => " ",
                };
                (background, marker, cell.number, cell.text.as_str())
            }
            None => (None, " ", None, ""),
        };
        div()
            .id((if old { "diff-old" } else { "diff-new" }, index))
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .font_family(self.code_font())
            .text_size(px(self.settings.code_font_size))
            .border_r_1()
            .border_color(if old {
                sidebar_rule(cx)
            } else {
                cx.theme().background
            })
            .when_some(background, |el, background| el.bg(background))
            .child(Self::diff_number(number, cx))
            .child(
                div()
                    .w(px(18.))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(marker),
            )
            .child(self.render_diff_text(text, number, old))
            .into_any_element()
    }

    pub(super) fn diff_choice(
        &self,
        id: &'static str,
        label: &str,
        selected: bool,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label.to_string())
            .px(px(6.))
            .rounded(px(4.))
            .cursor_pointer()
            .text_size(ui(11.))
            .text_color(if selected {
                cx.theme().accent_foreground
            } else {
                cx.theme().muted_foreground
            })
            .when(selected, |el| el.bg(cx.theme().list_active))
            .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
            .child(label.to_string())
            .into_any_element()
    }

    /// One row of the diff: both line numbers, then the line itself.
    pub(super) fn render_diff_row(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let Some(line) = self
            .project
            .diff
            .as_ref()
            .and_then(|view| view.lines.get(index))
        else {
            return div().into_any_element();
        };
        if line.change == diff::Change::Hunk {
            return div()
                .w_full()
                .h(px(24.))
                .px_4()
                .flex()
                .items_center()
                .bg(cx.theme().list_hover)
                .text_size(ui(11.))
                .text_color(cx.theme().muted_foreground)
                .child(div().min_w_0().truncate().child(line.text.clone()))
                .into_any_element();
        }
        let (background, marker) = match line.change {
            diff::Change::Added => (Some(cx.theme().success.opacity(0.16)), "+"),
            diff::Change::Removed => (Some(cx.theme().danger.opacity(0.16)), "−"),
            _ => (None, " "),
        };
        div()
            .w_full()
            .h(px(22.))
            .flex()
            .items_center()
            .font_family(self.code_font())
            .text_size(px(self.settings.code_font_size))
            .when_some(background, |el, background| el.bg(background))
            .child(Self::diff_number(line.old, cx))
            .child(Self::diff_number(line.new, cx))
            .child(
                div()
                    .w(px(18.))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(marker),
            )
            .child(self.render_diff_text(
                &line.text,
                match line.change {
                    diff::Change::Removed => line.old,
                    _ => line.new.or(line.old),
                },
                line.change == diff::Change::Removed,
            ))
            .into_any_element()
    }

    /// One diff line, colored like the editor. Hunk headers stay muted.
    pub(super) fn render_diff_text(
        &self,
        text: &str,
        number: Option<u32>,
        old: bool,
    ) -> AnyElement {
        let styles = number.and_then(|number| {
            let view = self.project.diff.as_ref()?;
            let map = if old {
                &view.old_styles
            } else {
                &view.new_styles
            };
            map.get(&number)
        });
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .child(match styles {
                Some(styles) if !styles.is_empty() => StyledText::new(text.to_string())
                    .with_highlights(styles.iter().cloned())
                    .into_any_element(),
                _ => text.to_string().into_any_element(),
            })
            .into_any_element()
    }

    /// A gutter cell. Empty on the side a line does not exist on, which is what
    /// keeps the two columns readable.
    pub(super) fn diff_number(number: Option<u32>, cx: &Context<Self>) -> AnyElement {
        div()
            .w(px(46.))
            .flex_shrink_0()
            .pr_2()
            .flex()
            .justify_end()
            .text_color(cx.theme().muted_foreground)
            .child(number.map(|number| number.to_string()).unwrap_or_default())
            .into_any_element()
    }
}
