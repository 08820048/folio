use super::*;

impl Folio {
    pub(super) fn icon_button(
        id: impl Into<ElementId>,
        // `impl Into<Icon>` rather than `IconName`, so the vendored icons in
        // `src/assets.rs` can be passed here too.
        icon: impl Into<Icon>,
        label: &'static str,
        activate: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        let activate = std::rc::Rc::new(activate);
        let keyboard = activate.clone();
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .focusable()
            .tab_index(0)
            .size(px(24.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(cx.theme().muted_foreground)
            .hover(|el| el.text_color(cx.theme().foreground))
            .focus_visible(|el| el.text_color(cx.theme().accent_foreground))
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(label).build(window, cx)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| activate(this, window, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    keyboard(this, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(Icon::new(icon).small())
            .into_any_element()
    }

    /// [`Self::icon_button`] with a glass icon: full colour, slightly dimmed
    /// until hovered. No background on any state.
    pub(super) fn glass_button(
        id: &'static str,
        icon: GlassIcon,
        label: &'static str,
        active: bool,
        activate: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        let activate = std::rc::Rc::new(activate);
        let keyboard = activate.clone();
        div()
            .id(id)
            .group(id)
            .role(Role::Button)
            .aria_label(label)
            .aria_selected(active)
            .focusable()
            .tab_index(0)
            .size(px(24.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(label).build(window, cx)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| activate(this, window, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    keyboard(this, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                Self::glass(icon, 16., glass_tone(active, cx))
                    .opacity(0.85)
                    .group_hover(id, |style| style.opacity(1.)),
            )
            .into_any_element()
    }

    /// A glass icon at `size` points, in its own colours.
    pub(super) fn glass(icon: GlassIcon, size: f32, tone: Tone) -> Img {
        img(icon.path(tone)).size(px(size)).flex_shrink_0()
    }

    /// One segment of the side panel's switch: glass icon and label. The
    /// selected one is a raised paper chip (white, hairline, soft drop),
    /// the rest sit flat on the track. Segments size to their content, so
    /// nothing spills out of a chip; `label` is off for the unselected ones
    /// when the panel is too narrow for three words, and the name moves to
    /// a tooltip.
    fn panel_segment(
        id: &'static str,
        icon: GlassIcon,
        name: &'static str,
        active: bool,
        label: bool,
        activate: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        let activate = std::rc::Rc::new(activate);
        let keyboard = activate.clone();
        let dark = cx.theme().is_dark();
        div()
            .id(id)
            .role(Role::Tab)
            .aria_label(name)
            .aria_selected(active)
            .focusable()
            .tab_index(0)
            .flex_auto()
            .h(px(28.))
            .px(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(5.))
            .rounded(px(7.))
            .border_1()
            .border_color(gpui::transparent_black())
            .cursor_pointer()
            .text_size(ui(12.))
            .whitespace_nowrap()
            .text_color(if active {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .when(active, |el| {
                el.bg(rgb(if dark { 0x2C2E32 } else { 0xFFFFFF }))
                    .border_color(if dark {
                        gpui::white().opacity(0.08)
                    } else {
                        gpui::black().opacity(0.07)
                    })
                    .shadow(vec![BoxShadow {
                        color: gpui::black().opacity(if dark { 0.35 } else { 0.08 }),
                        offset: point(px(0.), px(1.)),
                        blur_radius: px(2.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .font_weight(FontWeight::MEDIUM)
            })
            .when(!active, |el| {
                el.hover(|el| el.text_color(cx.theme().foreground))
            })
            .when(!label, |el| {
                el.tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(name).build(window, cx)
                })
            })
            .focus_visible(|el| el.border_color(cx.theme().ring))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| activate(this, window, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    keyboard(this, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                Self::glass(icon, 15., glass_tone(active, cx))
                    .when(!active, |el| el.grayscale(true).opacity(0.7)),
            )
            .when(label, |el| el.child(name))
            .into_any_element()
    }

    /// The left panel in the daily layout: a Files / Changes / Search switch
    /// on top, the tree or the change list under it, and the terminal and
    /// settings at the foot. It sits on the window ground, not in a card.
    pub(super) fn render_side_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let searching = self.panel == Some(Panel::Search);
        let tree = self.sidebar_kind == SidebarKind::Tree && !searching;
        let changes = self.sidebar_kind == SidebarKind::Changes && !searching;
        let dark = cx.theme().is_dark();
        // Three icon-and-word segments need about 270pt; narrower, only the
        // selected one keeps its word.
        let words = self.sidebar_width >= 270.;
        div()
            .id("side-panel")
            .w(px(self.sidebar_width))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_shrink_0()
                    .mx_1()
                    .mb(px(8.))
                    .p(px(3.))
                    .flex()
                    .gap(px(2.))
                    .rounded(px(10.))
                    .bg(if dark {
                        gpui::white().opacity(0.05)
                    } else {
                        gpui::black().opacity(0.05)
                    })
                    .child(Self::panel_segment(
                        "panel-files",
                        GlassIcon::Files,
                        "Files",
                        tree,
                        words || tree,
                        |this, window, cx| this.show_side(SidebarKind::Tree, window, cx),
                        cx,
                    ))
                    .child(Self::panel_segment(
                        "panel-changes",
                        GlassIcon::Changes,
                        "Changes",
                        changes,
                        words || changes,
                        |this, window, cx| this.show_side(SidebarKind::Changes, window, cx),
                        cx,
                    ))
                    .child(Self::panel_segment(
                        "panel-search",
                        GlassIcon::Search,
                        "Search",
                        searching,
                        words || searching,
                        |this, window, cx| this.show_search(false, window, cx),
                        cx,
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(match self.sidebar_kind {
                        SidebarKind::Tree => self.render_tree(cx),
                        SidebarKind::Changes => self.render_changes_list(cx),
                    }),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(32.))
                    .px_1()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(Self::glass_button(
                        "panel-terminal",
                        GlassIcon::Terminal,
                        "Terminal",
                        self.terminal_open,
                        |this, window, cx| this.toggle_terminal(window, cx),
                        cx,
                    ))
                    .child(Self::glass_button(
                        "panel-settings",
                        GlassIcon::Settings,
                        "Settings",
                        false,
                        |this, _, cx| this.show_settings(cx),
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// One start-page action: a large bare glass icon, the title, a one-line
    /// hint and its shortcut. Hover turns the title steel blue; there is no
    /// background block (AGENTS.md).
    fn launcher_action(
        id: &'static str,
        icon: GlassIcon,
        title: &'static str,
        hint: &'static str,
        kbd: Option<&'static str>,
        activate: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        let activate = std::rc::Rc::new(activate);
        let keyboard = activate.clone();
        let accent = cx.theme().accent_foreground;
        div()
            .id(id)
            .group(id)
            .role(Role::Button)
            .aria_label(title)
            .focusable()
            .tab_index(0)
            .flex()
            .items_center()
            .gap(px(14.))
            .py(px(10.))
            .cursor_pointer()
            .focus_visible(|el| el.text_color(accent))
            .on_click(cx.listener(move |this, _, window, cx| activate(this, window, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    keyboard(this, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(Self::glass(icon, 36., glass_tone(false, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(ui(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .group_hover(id, |style| style.text_color(accent))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(hint),
                    ),
            )
            .when_some(kbd, |el, kbd| el.child(Self::kbd_label(kbd, cx)))
            .into_any_element()
    }

    /// A shortcut written out as a small keycap.
    pub(super) fn kbd_label(label: &'static str, cx: &Context<Self>) -> AnyElement {
        div()
            .flex_shrink_0()
            .px(px(6.))
            .py(px(1.))
            .rounded(px(4.))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .text_size(ui(11.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(cx.theme().muted_foreground)
            .child(label)
            .into_any_element()
    }

    pub(super) fn render_launcher(&self, cx: &mut Context<Self>) -> AnyElement {
        let mac = cfg!(target_os = "macos");
        let actions = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(Self::launcher_action(
                "open-project",
                GlassIcon::Open,
                if self.loading {
                    "Opening…"
                } else {
                    "Open Project…"
                },
                "Any folder, Git or not",
                Some(if mac { "⌘O" } else { "Ctrl+O" }),
                |this, window, cx| this.request(Next::Picker, window, cx),
                cx,
            ))
            .child(Self::launcher_action(
                "new-project",
                GlassIcon::Files,
                "New Project…",
                "An empty folder to start in",
                Some(if mac { "⇧⌘O" } else { "Ctrl+Shift+O" }),
                |this, window, cx| this.request(Next::NewProject, window, cx),
                cx,
            ))
            .child(Self::launcher_action(
                "clone-github",
                GlassIcon::Branch,
                if self.cloning {
                    "Cloning…"
                } else {
                    "Clone from GitHub…"
                },
                "owner/repo or a URL",
                None,
                |this, window, cx| this.request(Next::CloneFromGithub, window, cx),
                cx,
            ));
        let intro = div()
            .w(px(320.))
            .flex_shrink_0()
            .pt_2()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(ui(28.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Folio"),
            )
            .child(
                div()
                    .mt(px(10.))
                    .text_size(ui(13.))
                    .text_color(cx.theme().muted_foreground)
                    .line_height(relative(1.5))
                    .child("Open a repository, read what changed,")
                    .child("fix a line, run it."),
            )
            .child(div().mt(px(36.)).child(actions));
        let recent = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .pt(px(16.))
            .px(px(12.))
            .pb(px(12.))
            .rounded(px(14.))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px(px(12.))
                    .pb(px(10.))
                    .child(Self::glass(GlassIcon::Recent, 16., glass_tone(false, cx)))
                    .child(
                        div()
                            .text_size(ui(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().muted_foreground)
                            .child("Recent"),
                    ),
            )
            .when(self.recent.is_empty(), |el| {
                el.child(
                    div()
                        .px(px(12.))
                        .py_4()
                        .text_size(ui(13.))
                        .text_color(cx.theme().muted_foreground)
                        .child("Projects you open show up here."),
                )
            })
            .children(self.recent.iter().enumerate().map(|(i, item)| {
                let path = item.path.clone();
                let remove = path.clone();
                let keyboard_path = path.clone();
                div()
                    .id(("recent", i))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(6.))
                    .hover(|el| el.bg(cx.theme().list_hover))
                    .child(
                        div()
                            .id(("recent-open", i))
                            .role(Role::Button)
                            .aria_label(format!("Open project {}", name(&path)))
                            .focusable()
                            .tab_index(0)
                            .on_key_down(cx.listener(
                                move |this, event: &KeyDownEvent, window, cx| {
                                    if event.keystroke.key == "enter" {
                                        this.request(Next::Open(keyboard_path.clone()), window, cx);
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .cursor_default()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.request(Next::Open(path.clone()), window, cx)
                            }))
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_size(ui(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .when(!item.available, |el| {
                                                el.text_color(cx.theme().muted_foreground)
                                            })
                                            .child(format!(
                                                "{}{}",
                                                name(&item.path),
                                                if item.available { "" } else { " · missing" }
                                            )),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_size(ui(11.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(relative_time(item.last_opened)),
                                    ),
                            )
                            .child(
                                div()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_size(ui(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(tilde(&item.path)),
                            ),
                    )
                    .child(Self::icon_button(
                        ("remove", i),
                        IconName::Close,
                        "Remove from recent projects",
                        move |this, _, cx| this.remove_recent(remove.clone(), cx),
                        cx,
                    ))
            }));
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .items_center()
            .justify_center()
            .pb(px(40.))
            .bg(workspace_chrome(cx))
            .child(
                div()
                    .w(px(880.))
                    .max_w_full()
                    .px_6()
                    .flex()
                    .items_start()
                    .gap(px(56.))
                    .child(intro)
                    .child(recent),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(18.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .text_size(ui(11.))
                    .text_color(cx.theme().muted_foreground.opacity(0.7))
                    .child(format!(
                        "Drop a folder anywhere to open it  ·  v{}",
                        env!("CARGO_PKG_VERSION")
                    )),
            )
            .into_any_element()
    }

    pub(super) fn select_project(
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
            self.toggle_directory(root.to_path_buf(), cx);
        } else {
            self.switch_project(root, window, cx);
            self.project.expanded.insert(root.to_path_buf());
            self.rebuild_rows();
            cx.notify();
        }
    }

    pub(super) fn render_project_header(
        &self,
        project: &Project,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let root = project.workspace.as_ref().unwrap().root.clone();
        let keyboard_root = root.clone();
        let menu_root = root.clone();
        let label = name(&root);
        let current = project.id == self.project.id;
        let expanded = current && project.expanded.contains(&root);
        div()
            .id(("project-root", project.id as usize))
            .role(Role::Button)
            .aria_label(label.clone())
            .aria_expanded(expanded)
            .focusable()
            .tab_index(0)
            .h(px(PROJECT_HEADER_HEIGHT))
            .flex_shrink_0()
            .mx_1()
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .text_size(ui(12.))
            .text_color(if current {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .cursor_default()
            .hover(|el| {
                el.bg(cx.theme().list_hover)
                    .text_color(cx.theme().accent_foreground)
            })
            .on_click(
                cx.listener(move |this, _, window, cx| this.select_project(&root, window, cx)),
            )
            .on_drop(cx.listener({
                let onto = keyboard_root.clone();
                move |this, drag: &TreeDrag, window, cx| {
                    this.drop_tree_entry(drag.path.clone(), onto.clone(), window, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    // The menu always acts on the project it opened over, so
                    // make that project current and open before offering it.
                    if this
                        .project
                        .workspace
                        .as_ref()
                        .is_none_or(|w| w.root != menu_root)
                    {
                        this.switch_project(&menu_root, window, cx);
                        this.project.expanded.insert(menu_root.clone());
                        this.rebuild_rows();
                    }
                    if !this.project.expanded.contains(&menu_root) {
                        this.toggle_directory(menu_root.clone(), cx);
                    }
                    this.open_menu(
                        MenuTarget::Tree {
                            path: menu_root.clone(),
                            root: true,
                        },
                        event.position,
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let current = this
                    .project
                    .workspace
                    .as_ref()
                    .is_some_and(|w| w.root == keyboard_root);
                let expanded = current && this.project.expanded.contains(&keyboard_root);
                match event.keystroke.key.as_str() {
                    "enter" | "space" => this.select_project(&keyboard_root, window, cx),
                    "left" if expanded => this.select_project(&keyboard_root, window, cx),
                    "right" if !expanded => this.select_project(&keyboard_root, window, cx),
                    "left" | "right" => {}
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(
                Icon::new(if expanded {
                    IconName::FolderOpen
                } else {
                    IconName::Folder
                })
                .small(),
            )
            .child(div().flex_1().min_w_0().truncate().child(label))
            .when(!project.tree_query.is_empty(), |el| {
                el.child(
                    div()
                        .flex_shrink_0()
                        .max_w(px(80.))
                        .truncate()
                        .text_size(ui(10.))
                        .text_color(cx.theme().accent_foreground)
                        .child(format!("[{}]", project.tree_query)),
                )
            })
            .when(project.documents.values().any(|d| d.dirty), |el| {
                el.child(Icon::new(IconName::Asterisk).xsmall())
            })
            .into_any_element()
    }

    pub(super) fn changes_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let files = self.changed_files();
        if files.is_empty() {
            return;
        }
        match event.keystroke.key.as_str() {
            "up" => {
                self.changes_selected = self.changes_selected.saturating_sub(1);
                cx.notify();
            }
            "down" => {
                self.changes_selected = (self.changes_selected + 1).min(files.len() - 1);
                cx.notify();
            }
            "enter" => {
                let path = files[self.changes_selected.min(files.len() - 1)].0.clone();
                self.open_changed_file(path, window, cx);
            }
            "space" => {
                let path = files[self.changes_selected.min(files.len() - 1)].0.clone();
                self.toggle_reviewed(&path, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    pub(super) fn render_changes_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let files = self.changed_files();
        let reviewed = files
            .iter()
            .filter(|(path, _)| self.project.reviewed.contains(path))
            .count();
        let against = self.diff_base();
        div()
            .id("changes-list")
            .role(Role::List)
            .aria_label("Changes")
            .track_focus(&self.tree_focus)
            .tab_index(0)
            .key_context("FolioTree")
            .w(px(self.sidebar_width))
            .h_full()
            .flex_shrink_0()
            .rounded_tl(px(WORKSPACE_RADIUS))
            .rounded_bl(px(WORKSPACE_RADIUS))
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::tree_key))
            .child(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .h(px(TREE_ROW_HEIGHT))
                    .flex()
                    .items_center()
                    .text_size(ui(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(if files.is_empty() {
                        format!("No changes against {against}")
                    } else {
                        format!("{} files · {against}", files.len())
                    })
                    .when(reviewed > 0, |el| {
                        el.child(format!(" · {reviewed} reviewed"))
                    }),
            )
            .child(
                uniform_list(
                    "changes",
                    files.len(),
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        let files = this.changed_files();
                        let selected = this.changes_selected.min(files.len().saturating_sub(1));
                        range
                            .filter_map(|index| {
                                let (path, kind) = files.get(index)?;
                                Some(this.render_change_row(
                                    index,
                                    path,
                                    *kind,
                                    index == selected,
                                    cx,
                                ))
                            })
                            .collect()
                    }),
                )
                .flex_1(),
            )
            .into_any_element()
    }

    pub(super) fn render_change_row(
        &self,
        index: usize,
        path: &Path,
        kind: char,
        selected: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let label = self.relative_path(path);
        let open = path.to_path_buf();
        let mark = if kind == 'U' { "U" } else { "M" };
        let reviewed = self.project.reviewed.contains(path);
        let click_path = path.to_path_buf();
        let key_path = path.to_path_buf();
        div()
            .id(("change-row", index))
            .role(Role::ListItem)
            .aria_label(label.clone())
            .aria_selected(selected)
            .h(px(TREE_ROW_HEIGHT))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .text_size(ui(12.))
            .text_color(if selected {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .when(selected, |el| el.bg(cx.theme().list_active))
            .hover(|el| el.bg(cx.theme().list_hover))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_changed_file(open.clone(), window, cx);
            }))
            .child(
                div()
                    .w(px(14.))
                    .flex_shrink_0()
                    .text_size(ui(11.))
                    .text_color(if kind == 'U' {
                        cx.theme().success
                    } else {
                        cx.theme().warning
                    })
                    .child(mark),
            )
            .child(div().flex_1().min_w_0().truncate().child(label))
            .child(
                div()
                    .id(("change-reviewed", index))
                    .role(Role::Button)
                    .aria_label(if reviewed {
                        "Mark as unreviewed"
                    } else {
                        "Mark as reviewed"
                    })
                    .focusable()
                    .tab_index(0)
                    .size(px(20.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(if reviewed {
                        cx.theme().success
                    } else {
                        cx.theme().muted_foreground.opacity(0.45)
                    })
                    .hover(|el| el.text_color(cx.theme().foreground))
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(if reviewed {
                            "Mark as unreviewed"
                        } else {
                            "Mark as reviewed"
                        })
                        .build(window, cx)
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.toggle_reviewed(&click_path, cx)),
                    )
                    .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.toggle_reviewed(&key_path, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(
                        Icon::new(if reviewed {
                            IconName::CircleCheck
                        } else {
                            IconName::Check
                        })
                        .xsmall(),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_tree(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("sidebar")
            .role(Role::Tree)
            .aria_label("Project")
            .track_focus(&self.tree_focus)
            .tab_index(0)
            .key_context("FolioTree")
            .w(px(self.sidebar_width))
            .h_full()
            .flex_shrink_0()
            .rounded_tl(px(WORKSPACE_RADIUS))
            .rounded_bl(px(WORKSPACE_RADIUS))
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::tree_key))
            .children(self.project_order.iter().filter_map(|root| {
                let project = std::iter::once(&self.project)
                    .chain(self.parked.iter())
                    .find(|p| p.workspace.as_ref().is_some_and(|w| &w.root == root))?;
                let expanded = project.id == self.project.id && project.expanded.contains(root);
                Some(
                    div()
                        .flex()
                        .flex_col()
                        .min_h_0()
                        .when(expanded, |el| {
                            el.flex_1()
                                .max_h(px(expanded_project_max_height(self.project.rows.len())))
                        })
                        .when(!expanded, |el| el.flex_shrink_0())
                        .child(self.render_project_header(project, cx))
                        .when(expanded, |el| el.child(self.render_file_tree(cx))),
                )
            }))
            .child(
                div().p_2().flex_shrink_0().child(
                    Button::new("add-project")
                        .text()
                        .icon(IconName::Plus)
                        .label("Add Project")
                        .small()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.request(Next::Picker, window, cx)
                        })),
                ),
            )
            .into_any_element()
    }

    pub(super) fn render_file_tree(&self, cx: &mut Context<Self>) -> AnyElement {
        uniform_list(
            "tree",
            self.project.rows.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|i| {
                        if this.editing.as_ref().is_some_and(|edit| edit.row == i) {
                            return this.render_inline_edit(cx);
                        }
                        let row = this.project.rows[i].clone();
                        let path = row.entry.path.clone();
                        let drag_path = path.clone();
                        let drop_path = path.clone();
                        let menu_path = path.clone();
                        let selected =
                            this.project.showing(&path) || i == this.project.selected_row;
                        let status = this
                            .project
                            .workspace
                            .as_ref()
                            .and_then(|w| path.strip_prefix(&w.root).ok())
                            .and_then(|p| this.project.git_status.get(p.to_string_lossy().as_ref()))
                            .copied();
                        let muted = cx.theme().muted_foreground;
                        let icon = if row.entry.kind == EntryKind::Directory {
                            Icon::new(if this.project.expanded.contains(&path) {
                                IconName::FolderOpen
                            } else {
                                IconName::Folder
                            })
                            .text_color(muted)
                        } else {
                            match file_icons::file_icon(&row.entry.name) {
                                Some(kind) => Icon::new(kind)
                                    .text_color(kind.tint(cx.theme().is_dark()).unwrap_or(muted)),
                                None => Icon::new(IconName::File).text_color(muted),
                            }
                        }
                        .small();
                        let focus = this
                            .tree_hover_row
                            .filter(|&ix| ix < this.project.rows.len())
                            .or_else(|| {
                                (!this.project.rows.is_empty()).then_some(this.project.selected_row)
                            });
                        let guides =
                            tree_guide_elements(&this.project.rows, i, row.depth, focus, cx);
                        div()
                            .id(("row", i))
                            .role(Role::TreeItem)
                            .aria_label(row.entry.name.clone())
                            .aria_selected(selected)
                            .relative()
                            .w_full()
                            .h(px(TREE_ROW_HEIGHT))
                            .pl(tree_row_indent(row.depth))
                            .pr_3()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(ui(12.))
                            .cursor_default()
                            .when(selected, |el| el.bg(tree_row_selected(cx)))
                            .when(!selected, |el| el.hover(|el| el.bg(cx.theme().list_hover)))
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if *hovered {
                                    if this.tree_hover_row != Some(i) {
                                        this.tree_hover_row = Some(i);
                                        cx.notify();
                                    }
                                } else if this.tree_hover_row == Some(i) {
                                    this.tree_hover_row = None;
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.project.selected_row = i;
                                this.project.tree_query.clear();
                                this.tree_focus.focus(window, cx);
                                match row.entry.kind {
                                    EntryKind::Directory => this.toggle_directory(path.clone(), cx),
                                    EntryKind::File => this.open_file(path.clone(), window, cx),
                                }
                            }))
                            .on_drag(
                                TreeDrag {
                                    path: drag_path,
                                    name: row.entry.name.clone().into(),
                                },
                                |drag, _, _, cx| {
                                    cx.new(|_| TreeDragPreview {
                                        name: drag.name.clone(),
                                    })
                                },
                            )
                            .on_drop(cx.listener(move |this, drag: &TreeDrag, window, cx| {
                                this.drop_tree_entry(
                                    drag.path.clone(),
                                    drop_path.clone(),
                                    window,
                                    cx,
                                );
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    // Naming another row abandons the field that
                                    // was open, which shifts every index below it.
                                    if this.editing.is_some() {
                                        this.cancel_create(window, cx);
                                    }
                                    let index = this
                                        .project
                                        .rows
                                        .iter()
                                        .position(|row| row.entry.path == menu_path)
                                        .unwrap_or(i);
                                    this.project.selected_row =
                                        index.min(this.project.rows.len().saturating_sub(1));
                                    this.tree_focus.focus(window, cx);
                                    this.open_menu(
                                        MenuTarget::Tree {
                                            path: menu_path.clone(),
                                            root: false,
                                        },
                                        event.position,
                                        window,
                                        cx,
                                    );
                                    cx.stop_propagation();
                                }),
                            )
                            .children(guides)
                            .child(icon)
                            // A changed file is named in its git colour with the
                            // letter at the right: M brass, A green.
                            .child({
                                let status_color = status.map(|status| {
                                    if status == 'U' {
                                        git_added_color(cx)
                                    } else {
                                        git_modified_color(cx)
                                    }
                                });
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .when_some(status_color, |el, color| el.text_color(color))
                                    .child(row.entry.name)
                            })
                            .when_some(status, |el, status| {
                                el.child(
                                    div()
                                        .flex_shrink_0()
                                        .pr_1()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .text_size(ui(11.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(if status == 'U' {
                                            git_added_color(cx)
                                        } else {
                                            git_modified_color(cx)
                                        })
                                        .child(if status == 'U' { "A" } else { "M" }),
                                )
                            })
                            .when(selected, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .border_1()
                                        .border_color(tree_row_selected_border(cx)),
                                )
                            })
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.project.tree_scroll)
        .p_1()
        .flex_1()
        .into_any_element()
    }

    /// The text field that names a new entry, drawn in the row it will occupy.
    pub(super) fn render_inline_edit(&self, cx: &Context<Self>) -> AnyElement {
        let Some(edit) = &self.editing else {
            return div().into_any_element();
        };
        let depth = self
            .project
            .rows
            .get(edit.row)
            .map(|row| row.depth)
            .unwrap_or(0);
        let focus = self
            .tree_hover_row
            .filter(|&ix| ix < self.project.rows.len())
            .or_else(|| (!self.project.rows.is_empty()).then_some(self.project.selected_row));
        div()
            .id("inline-edit")
            .relative()
            .w_full()
            .h(px(TREE_ROW_HEIGHT))
            .pl(tree_row_indent(depth))
            .pr_3()
            .flex()
            .items_center()
            .gap_2()
            .children(tree_guide_elements(
                &self.project.rows,
                edit.row,
                depth,
                focus,
                cx,
            ))
            .child(
                Icon::new(if edit.kind == EntryKind::Directory {
                    IconName::Folder
                } else {
                    IconName::File
                })
                .small()
                .text_color(cx.theme().muted_foreground),
            )
            .child(div().flex_1().min_w_0().child(
                // No border and no background, so the field reads as part
                // of the row rather than a box dropped into the tree.
                Input::new(&edit.input).appearance(false).h(px(20.)),
            ))
            .into_any_element()
    }

    /// Swallows the click that dismisses the menu, so the tree underneath does
    /// not also act on it.
    pub(super) fn render_menu_backdrop(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .id("tree-menu-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_menu(cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| this.close_menu(cx)),
            )
            .on_scroll_wheel(cx.listener(|this, _, _, cx| this.close_menu(cx)))
            .into_any_element()
    }

    /// The right-click menu. Drawn last in the root stack so it covers the tree
    /// and the lookup panel, and darker than the sidebar it opens over so the
    /// edge is visible without relying on the shadow alone.
    pub(super) fn render_menu(&self, menu: &ContextMenu, cx: &Context<Self>) -> AnyElement {
        let (entries, rules) = surface_of(&menu.target).menu();
        let path = menu.target.path();
        div()
            .id("tree-menu")
            .role(Role::Menu)
            .absolute()
            .left(menu.position.x)
            .top(menu.position.y)
            .w(px(MENU_WIDTH))
            .occlude()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .shadow_lg()
            .p_1()
            .flex()
            .flex_col()
            .text_size(ui(12.))
            .text_color(cx.theme().foreground)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(
                visible_menu_items(&menu.target)
                    .into_iter()
                    .enumerate()
                    .flat_map(|(position, index)| {
                        let (item, label, shortcut) = entries[index];
                        let label = self.menu_label(item, path, label);
                        let enabled = self.menu_item_enabled(item, &menu.target, cx);
                        // A rule only ever sits between two entries.
                        let separator = (position > 0 && rules.contains(&index)).then(|| {
                            div()
                                .h(px(1.))
                                .my(px(4.))
                                .bg(cx.theme().border)
                                .into_any_element()
                        });
                        let row = div()
                            .id(("menu-item", index))
                            .role(Role::MenuItem)
                            .aria_label(label.clone())
                            .h(px(MENU_ROW))
                            .px_2()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .rounded_sm()
                            .when(index == menu.selected, |el| el.bg(cx.theme().list_active))
                            .when(enabled, |el| {
                                el.cursor_pointer()
                                    .hover(|el| el.bg(cx.theme().list_hover))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.run_menu_item(item, window, cx)
                                    }))
                            })
                            .when(!enabled, |el| {
                                el.cursor_default()
                                    .text_color(cx.theme().muted_foreground)
                                    .opacity(0.5)
                            })
                            .child(div().child(label))
                            .child(
                                div()
                                    .text_size(ui(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(shortcut_label(shortcut)),
                            )
                            .into_any_element();
                        separator.into_iter().chain(std::iter::once(row))
                    }),
            )
            .into_any_element()
    }

    pub(super) fn relative_path(&self, path: &Path) -> String {
        self.project
            .workspace
            .as_ref()
            .and_then(|workspace| path.strip_prefix(&workspace.root).ok())
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }

    pub(super) fn render_clone_prompt(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("clone-prompt")
            .absolute()
            .top(px(48.))
            .left(relative(0.5))
            .ml(px(-260.))
            .w(px(520.))
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .shadow_lg()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_size(ui(13.))
                    .child("Clone from GitHub"),
            )
            .child(Input::new(&self.clone_url).focus_bordered(false))
            .child(
                div()
                    .px_2()
                    .text_size(ui(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child("Enter to choose a folder · Esc to close"),
            )
            .into_any_element()
    }
}
