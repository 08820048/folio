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

    /// The title-bar button: show or hide the icon rail. The file tree
    /// stays as it is.
    pub(super) fn toggle_activity_bar(&mut self, cx: &mut Context<Self>) {
        self.settings.activity_bar = !self.activity_bar;
        self.apply_settings(cx);
    }

    /// How far the rounded card starts from the window's left edge: the
    /// rail, when it is showing, plus the gap that keeps the card off it.
    pub(super) fn workspace_left_inset(&self) -> f32 {
        if self.activity_bar {
            ACTIVITY_BAR_WIDTH + WORKSPACE_RAIL_GAP
        } else {
            WORKSPACE_INSET
        }
    }

    pub(super) fn render_titlebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let project = self
            .project
            .workspace
            .as_ref()
            .map(|w| name(&w.root))
            .unwrap_or_default();
        let relative = self
            .project
            .active()
            .and_then(|p| {
                self.project
                    .workspace
                    .as_ref()
                    .and_then(|w| p.strip_prefix(&w.root).ok())
            })
            .map(|p| p.to_string_lossy().into_owned());
        let doc = self
            .project
            .active()
            .and_then(|p| self.project.documents.get(p));
        TitleBar::new()
            .bg(if self.project.workspace.is_some() {
                workspace_chrome(cx)
            } else {
                cx.theme().background
            })
            // The workspace card already draws its own top edge. A second
            // line here would sit on top of that one.
            .border_color(gpui::transparent_black())
            .on_close_window(cx.listener(|this, _, window, cx| this.close_window(window, cx)))
            .child(
                div()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .pr_3()
                    .when(self.project.workspace.is_some(), |el| {
                        el.child(Self::icon_button(
                            "activity-bar-toggle",
                            if self.activity_bar {
                                FolioIcon::PanelLeftDashed
                            } else {
                                FolioIcon::PanelRightDashed
                            },
                            "Toggle activity bar",
                            |this, _, cx| this.toggle_activity_bar(cx),
                            cx,
                        ))
                    })
                    .child(div().text_size(ui(12.)).child(project))
                    .when(relative.is_some(), |el| {
                        el.child(div().text_color(cx.theme().muted_foreground).child("/"))
                    })
                    .child(
                        div()
                            .flex_1()
                            .text_size(ui(12.))
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(relative.unwrap_or_default()),
                    )
                    .when(doc.is_some_and(|d| d.dirty), |el| {
                        el.child(
                            div()
                                .text_color(cx.theme().accent_foreground)
                                .child(Icon::new(IconName::Asterisk).xsmall()),
                        )
                    })
                    .when(self.loading, |el| {
                        el.child(
                            div()
                                .text_size(ui(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Loading…"),
                        )
                    })
                    .when_some(self.available_update.as_ref(), |el, update| {
                        el.child(self.render_update_badge(update, cx))
                    })
                    .when(self.run_is_busy(cx), |el| {
                        el.child(Self::icon_button(
                            "stop",
                            Icon::new(IconName::Pause).text_color(run_stop_color(cx)),
                            "Stop",
                            |this, window, cx| this.stop_run(window, cx),
                            cx,
                        ))
                    })
                    .when(!self.run_is_busy(cx) && self.can_run_active(cx), |el| {
                        el.child(Self::icon_button(
                            "run",
                            Icon::new(IconName::Play).text_color(cx.theme().success),
                            "Run",
                            |this, window, cx| this.run_code(None, window, cx),
                            cx,
                        ))
                    }),
            )
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
        div()
            .id("workspace")
            .size_full()
            .flex()
            .rounded(px(WORKSPACE_RADIUS))
            .overflow_hidden()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .when(self.sidebar, |el| {
                el.child(match self.sidebar_kind {
                    SidebarKind::Tree => self.render_tree(cx),
                    SidebarKind::Changes => self.render_changes_list(cx),
                })
                .child(
                    div()
                        .id("sidebar-resize")
                        .w(px(5.))
                        .h_full()
                        .flex()
                        .justify_center()
                        .cursor_col_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, _| this.resizing = true),
                        )
                        .child(div().w(px(1.)).h_full().bg(sidebar_rule(cx))),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    // The right corners stop being the card's edge when
                    // the terminal column takes that side.
                    .when(!(self.terminal_open && self.terminal_right), |el| {
                        el.rounded_tr(px(WORKSPACE_RADIUS))
                            .rounded_br(px(WORKSPACE_RADIUS))
                    })
                    .when(!self.sidebar, |el| {
                        el.rounded_tl(px(WORKSPACE_RADIUS))
                            .rounded_bl(px(WORKSPACE_RADIUS))
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
                    .when(self.terminal_open && !self.terminal_right, |el| {
                        el.child(self.render_terminal_dock(window, cx))
                    }),
            )
            // On the right the dock is a column of its own, after the
            // content column, with the card's right edge.
            .when(self.terminal_open && self.terminal_right, |el| {
                el.child(self.render_terminal_dock(window, cx))
            })
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
            .child(self.render_tabs(index, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_col()
                    .when(!self.sidebar && !showing_diff && !split, |el| el.px_6())
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
                        el.child(
                            div().flex_1().min_h_0().w_full().p_6().child(
                                img(image.clone())
                                    .size_full()
                                    .object_fit(ObjectFit::Contain),
                            ),
                        )
                        .child(
                            div()
                                .px_4()
                                .py_2()
                                .text_size(ui(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} · {} × {} · static preview",
                                    name(path),
                                    u32::from(image.size(0).width),
                                    u32::from(image.size(0).height)
                                )),
                        )
                    })
                    .when(!showing_diff && doc.is_none() && image.is_none(), |el| {
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
                    }),
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
