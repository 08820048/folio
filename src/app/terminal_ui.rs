use super::*;

impl Folio {
    /// The active project's terminal tabs, creating its first the first
    /// time the drawer asks. A terminal lives until its tab closes or its
    /// project does; hiding the drawer keeps the children running.
    pub(super) fn ensure_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        if self.terminals.contains_key(&root) {
            return;
        }
        let view = cx.new(|cx| TerminalView::new(&root, 80, 24, cx));
        let watch = Self::watch_terminal(&view, cx);
        self.terminals.insert(
            root,
            TerminalTabs {
                views: vec![view],
                active: 0,
                last_run: None,
                _watches: vec![watch],
            },
        );
    }

    pub(super) fn watch_terminal(
        view: &Entity<TerminalView>,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe(view, |_, _, _: &TerminalBusy, cx| {
            cx.notify();
        })
    }

    /// A fresh terminal tab, focused and showing.
    pub(super) fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        self.terminal_open = true;
        let view = cx.new(|cx| TerminalView::new(&root, 80, 24, cx));
        view.update(cx, |view, cx| view.focus_handle().focus(window, cx));
        let watch = Self::watch_terminal(&view, cx);
        let tabs = self.terminals.entry(root).or_insert(TerminalTabs {
            views: vec![],
            active: 0,
            last_run: None,
            _watches: vec![],
        });
        tabs.views.push(view);
        tabs._watches.push(watch);
        tabs.active = tabs.views.len() - 1;
        cx.notify();
    }

    pub(super) fn active_terminal(&self) -> Option<&Entity<TerminalView>> {
        let root = self.project.workspace.as_ref()?.root.clone();
        let tabs = self.terminals.get(&root)?;
        tabs.views.get(tabs.active)
    }

    /// Close one terminal tab. The drawer goes away with the last one, and
    /// the tab that slid into the closed one's place takes the keyboard.
    pub(super) fn close_terminal_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        let empty = {
            let Some(tabs) = self.terminals.get_mut(&root) else {
                return;
            };
            if index >= tabs.views.len() {
                return;
            }
            tabs.views.remove(index);
            if index < tabs._watches.len() {
                drop(tabs._watches.remove(index));
            }
            if tabs.views.is_empty() {
                true
            } else {
                if index < tabs.active {
                    tabs.active -= 1;
                }
                tabs.active = tabs.active.min(tabs.views.len() - 1);
                if let Some(view) = tabs.views.get(tabs.active) {
                    view.update(cx, |view, cx| view.focus_handle().focus(window, cx));
                }
                false
            }
        };
        if empty {
            self.terminals.remove(&root);
            self.terminal_open = false;
            if let Some(doc) = self
                .project
                .active()
                .and_then(|p| self.project.documents.get(p))
            {
                doc.editor.focus_handle(cx).focus(window, cx);
            } else {
                self.tree_focus.focus(window, cx);
            }
        }
        cx.notify();
    }

    /// `⌘J`: show or hide the terminal dock.
    pub(super) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.workspace.is_none() || self.project_loading || self.prompting {
            return;
        }
        if self.terminal_open {
            self.terminal_open = false;
            if let Some(doc) = self
                .project
                .active()
                .and_then(|p| self.project.documents.get(p))
            {
                doc.editor.focus_handle(cx).focus(window, cx);
            } else {
                self.tree_focus.focus(window, cx);
            }
        } else {
            self.terminal_open = true;
            self.ensure_terminal(cx);
            if let Some(view) = self.active_terminal() {
                view.update(cx, |view, cx| view.focus_handle().focus(window, cx));
            }
        }
        cx.notify();
    }

    /// The drawer: a drag handle on top, a tab strip with one entry per
    /// terminal — each closing itself, the last closing the drawer — a
    /// `+` for another one, and the active terminal filling the rest. The
    /// grid sizes itself to the pixels the body gives it.
    pub(super) fn render_terminal_dock(&self, _: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some((root, active)) = self
            .project
            .workspace
            .as_ref()
            .map(|w| w.root.clone())
            .and_then(|root| {
                self.terminals
                    .get(&root)
                    .map(|tabs| (root, tabs.active.min(tabs.views.len().saturating_sub(1))))
            })
        else {
            return div().into_any_element();
        };
        let entity = self.terminals[&root].views[active].clone();
        let tab_count = self.terminals[&root].views.len();
        let labels: Vec<String> = self.terminals[&root]
            .views
            .iter()
            .map(|view| {
                let view = view.read(cx);
                match (view.title().map(str::to_string), view.is_run(), view.exit()) {
                    (Some(title), _, _) => title,
                    (None, true, Some(Some(code))) => format!("Run · exited ({code})"),
                    (None, true, Some(None)) => "Run · exited".to_string(),
                    (None, true, None) => "Run".to_string(),
                    (None, false, Some(Some(code))) => format!("Terminal · exited ({code})"),
                    (None, false, Some(None)) => "Terminal · exited".to_string(),
                    (None, false, None) => "Terminal".to_string(),
                }
            })
            .collect();
        // The tab strip reads like the one above the code: a tab per
        // terminal, each with its own close, a button that swaps the dock
        // between drawer and right-hand column, and a `+` for another.
        // Its bottom rule is what makes the dock read as its own panel
        // when the column sits beside the editor's tab strip at the same
        // height.
        let tab_strip =
            div()
                .h(px(26.))
                .flex()
                .items_center()
                .pr_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .children((0..tab_count).map(|index| {
                    self.render_terminal_tab(index, &labels[index], index == active, cx)
                }))
                .child(div().flex_1())
                .child(Self::icon_button(
                    "terminal-side",
                    if self.terminal_right {
                        IconName::PanelBottom
                    } else {
                        IconName::PanelRight
                    },
                    if self.terminal_right {
                        "Move Terminal Below"
                    } else {
                        "Move Terminal Right"
                    },
                    |this, _, cx| {
                        this.terminal_right = !this.terminal_right;
                        cx.notify();
                    },
                    cx,
                ))
                .child(Self::icon_button(
                    "terminal-new",
                    IconName::Plus,
                    "New Terminal",
                    |this, window, cx| this.new_terminal(window, cx),
                    cx,
                ));
        let body = div().flex_1().min_h_0().overflow_hidden().child(entity);
        if self.terminal_right {
            // A column beside the editor: the left edge drags the width,
            // the way the drawer's top edge drags the height.
            div()
                .id("terminal-dock")
                .debug_selector(|| "terminal-dock".to_string())
                .flex_shrink_0()
                .w(px(self.terminal_width))
                .h_full()
                .flex()
                .border_l_1()
                .border_color(cx.theme().border)
                .rounded_tr(px(WORKSPACE_RADIUS))
                .rounded_br(px(WORKSPACE_RADIUS))
                .child(
                    div()
                        .id("terminal-resize")
                        .w(px(4.))
                        .h_full()
                        .cursor_col_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, _| this.resizing_terminal = true),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .flex_col()
                        .child(tab_strip)
                        .child(body),
                )
                .into_any_element()
        } else {
            div()
                .flex_shrink_0()
                .h(px(self.terminal_height))
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(cx.theme().border)
                .rounded_br(px(WORKSPACE_RADIUS))
                .when(!self.sidebar, |el| el.rounded_bl(px(WORKSPACE_RADIUS)))
                // The top edge drags the height, the way the sidebar's edge
                // drags the width.
                .child(
                    div()
                        .id("terminal-resize")
                        .h(px(4.))
                        .cursor_row_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, _| this.resizing_terminal = true),
                        ),
                )
                .child(tab_strip)
                .child(body)
                .into_any_element()
        }
    }

    /// One terminal tab: the terminal's mark and title, and the close that
    /// takes the tab — and, if it was the last, the drawer — with it.
    pub(super) fn render_terminal_tab(
        &self,
        index: usize,
        label: &str,
        active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(("terminal-tab", index))
            .role(Role::Button)
            .aria_label(label.to_string())
            .aria_selected(active)
            .h_full()
            .min_w_0()
            .max_w(px(220.))
            .px_2()
            .flex()
            .items_center()
            .gap_1p5()
            .flex_shrink_0()
            .border_r_1()
            .border_color(cx.theme().border)
            .cursor_pointer()
            .text_size(ui(11.))
            .text_color(if active {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .when(active, |el| el.bg(cx.theme().list_active))
            .hover(|el| el.bg(cx.theme().list_hover))
            .on_click(
                cx.listener(move |this, _, window, cx| {
                    this.activate_terminal_tab(index, window, cx)
                }),
            )
            .child(
                Icon::new(FolioIcon::SquareTerminal)
                    .size_4()
                    .text_color(if active {
                        cx.theme().accent_foreground
                    } else {
                        cx.theme().muted_foreground
                    }),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(label.to_string()),
            )
            .child(
                div()
                    .id(("terminal-tab-close", index))
                    .role(Role::Button)
                    .aria_label("Close Terminal")
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
                    // click and merely activate itself.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_terminal_tab(index, window, cx)
                    }))
                    .child(Icon::new(IconName::Close).xsmall()),
            )
            .into_any_element()
    }

    pub(super) fn activate_terminal_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        let Some(tabs) = self.terminals.get_mut(&root) else {
            return;
        };
        if index >= tabs.views.len() {
            return;
        }
        tabs.active = index;
        if let Some(view) = tabs.views.get(index) {
            view.update(cx, |view, cx| view.focus_handle().focus(window, cx));
        }
        cx.notify();
    }
}
