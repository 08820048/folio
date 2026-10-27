use super::*;

pub(super) struct AboutView;

const SITE_URL: &str = "https://folioedit.dev";
const SITE_LABEL: &str = "folioedit.dev";

impl Render for AboutView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("about")
            .key_context("FolioAbout")
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_size(ui(13.))
            .on_action(cx.listener(|_, _: &CloseWindow, window, _| window.remove_window()))
            .child(
                TitleBar::new()
                    .bg(cx.theme().background)
                    .border_color(cx.theme().border)
                    .child(div().h_full().flex_1()),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .px(px(40.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(div().text_size(ui(22.)).child("Folio"))
                    // Read from the manifest rather than written out here, so
                    // it cannot drift from the version that was built.
                    .child(
                        div()
                            .mt(px(6.))
                            .text_size(ui(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
                    )
                    // Two centered lines instead of one that fills the window:
                    // a single wrapped sentence was stretching to the chrome.
                    .child(
                        div()
                            .mt(px(16.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_1()
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                div()
                                    .whitespace_nowrap()
                                    .child("A local code reading editor,"),
                            )
                            .child(div().whitespace_nowrap().child("built with Rust and GPUI.")),
                    )
                    .child(
                        div()
                            .id("about-site")
                            .mt(px(16.))
                            .px_3()
                            .py_2()
                            .role(Role::Link)
                            .aria_label(SITE_URL)
                            .focusable()
                            .tab_index(0)
                            .cursor_pointer()
                            .text_size(ui(12.))
                            .text_color(cx.theme().accent_foreground)
                            .hover(|el| el.text_color(cx.theme().foreground))
                            .focus_visible(|el| el.text_color(cx.theme().foreground))
                            .on_click(|_, _, cx| cx.open_url(SITE_URL))
                            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    cx.open_url(SITE_URL);
                                    cx.stop_propagation();
                                }
                            }))
                            .child(SITE_LABEL),
                    ),
            )
    }
}

/// The `⌘,` window: a plain form over `Folio`'s settings. It owns its own text
/// fields and pushes every change back through the app view, so there is one
/// copy of the settings and one place that applies them.
/// A page of settings. These are the leaves of the sidebar tree, and each one
/// is one screen of rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Interface,
    Type,
    Indentation,
    Ignored,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Page::Interface => "Interface",
            Page::Type => "Font & Size",
            Page::Indentation => "Indentation",
            Page::Ignored => "Ignored Folders",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Page::Interface => "interface",
            Page::Type => "type",
            Page::Indentation => "indentation",
            Page::Ignored => "ignored",
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Page::Interface => "How the interface itself looks",
            Page::Type => "The font code is drawn in",
            Page::Indentation => "Tabs, spaces and how wide they are",
            Page::Ignored => "Which files a project shows",
        }
    }
}

/// A heading in the sidebar and the pages under it. Zed's shape: the headings
/// collapse, and only the pages are screens.
pub(super) struct Group {
    title: &'static str,
    pub(super) pages: &'static [Page],
}

pub(super) const GROUPS: &[Group] = &[
    Group {
        title: "Appearance",
        pages: &[Page::Interface],
    },
    Group {
        title: "Editor",
        pages: &[Page::Type, Page::Indentation],
    },
    Group {
        title: "Files",
        pages: &[Page::Ignored],
    },
];

pub(super) struct SettingsView {
    pub(super) folio: WeakEntity<Folio>,
    /// A snapshot of what is stored. The view renders this instead of reading
    /// the app view: the settings window is built from inside `Folio`'s own
    /// update, and rendering it while that is in flight would be a re-entrant
    /// read. It is only ever written through `set_settings` and `change`.
    settings: Settings,
    /// Which page is showing, and which headings are open. A view of its own
    /// rather than part of the stored settings: it is where you were, not what
    /// you chose.
    pub(super) page: Page,
    pub(super) expanded: [bool; GROUPS.len()],
    pub(super) font_select: Entity<FontSelect>,
    pub(super) code_font_select: Entity<FontSelect>,
    pub(super) ignore_query: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub(super) fn new(
        folio: WeakEntity<Folio>,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let installed = cx.text_system().all_font_names();
        let mut make_select = |current: &Option<String>, code: bool| {
            let options = font_options(installed.clone(), current.as_deref(), code);
            let value = current.clone().unwrap_or_else(|| {
                if code {
                    "JetBrains Mono"
                } else {
                    "System default"
                }
                .to_owned()
            });
            cx.new(|cx| {
                let mut select = SelectState::new(SearchableVec::new(options), None, window, cx)
                    .searchable(true);
                select.set_selected_value(&value, window, cx);
                select
            })
        };
        let font_select = make_select(&settings.font_family, false);
        let code_font_select = make_select(&settings.code_font_family, true);
        let ignore_query =
            cx.new(|cx| InputState::new(window, cx).placeholder("node_modules, target"));
        ignore_query.update(cx, |input, cx| {
            input.set_value(settings.ignored.join(", "), window, cx)
        });
        let mut subscriptions = vec![cx.subscribe_in(
            &ignore_query,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    this.commit_text(window, cx);
                }
            },
        )];
        for (select, code) in [(&font_select, false), (&code_font_select, true)] {
            subscriptions.push(cx.subscribe_in(
                select,
                window,
                move |this: &mut Self, _, event: &SelectEvent<SearchableVec<String>>, _, cx| {
                    let SelectEvent::Confirm(Some(value)) = event else {
                        return;
                    };
                    let default = if code {
                        "JetBrains Mono"
                    } else {
                        "System default"
                    };
                    let family = (value != default).then(|| value.clone());
                    this.change(
                        move |settings| {
                            if code {
                                settings.code_font_family = family;
                            } else {
                                settings.font_family = family;
                            }
                        },
                        cx,
                    );
                },
            ));
        }
        Self {
            folio,
            settings,
            page: Page::Type,
            // Open to begin with: four pages do not need hiding, and a tree
            // that starts folded reads as a list of nothing.
            expanded: [true; GROUPS.len()],
            font_select,
            code_font_select,
            ignore_query,
            _subscriptions: subscriptions,
        }
    }

    /// Adopt what the app view settled on. Called after every apply, so a
    /// value the app clamped or rejected shows up here too.
    pub(super) fn set_settings(&mut self, settings: Settings, cx: &mut Context<Self>) {
        if self.settings != settings {
            self.settings = settings;
            cx.notify();
        }
    }

    /// Apply a change through the app view, so the theme, the tree, the file
    /// and the other window all move together. The snapshot is updated first
    /// so this window draws the new value on its next frame either way.
    pub(super) fn change(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        change(&mut self.settings);
        self.settings = self.settings.clone().clamped();
        if let Some(folio) = self.folio.upgrade() {
            let settings = self.settings.clone();
            folio.update(cx, |folio, cx| {
                folio.settings = settings;
                folio.apply_settings(cx);
            });
        }
        cx.notify();
    }

    /// Committing the ignore list never changes the font selections.
    pub(super) fn commit_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let ignored = self.ignore_query.read(cx).value().to_string();
        self.change(
            move |settings| {
                settings.ignored = ignored
                    .split(',')
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty())
                    .collect();
            },
            cx,
        );
    }

    fn font_picker(&self, select: &Entity<FontSelect>, label: &'static str) -> AnyElement {
        div()
            .id(label)
            .role(Role::Group)
            .aria_label(label)
            .w(px(FIELD_WIDTH))
            .child(
                Select::new(select)
                    .small()
                    .search_placeholder("Search fonts…")
                    .menu_width(px(300.))
                    .menu_max_h(px(280.)),
            )
            .into_any_element()
    }

    /// A row: a fixed-width label, then the control.
    /// A settings row, the shape Zed gives them: what the setting is called
    /// and what it does stacked on the left, its control flush right, and a
    /// hairline underneath to separate it from the next one.
    fn row(
        title: &'static str,
        description: &'static str,
        control: AnyElement,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .px_6()
            .py_3()
            .flex()
            .items_center()
            .gap_6()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(ui(12.))
                            .text_color(cx.theme().foreground)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    ),
            )
            .child(div().flex_shrink_0().child(control))
            .into_any_element()
    }

    /// A field in a row. Wide enough for a font name or a short path list, and
    /// the same width everywhere so the controls line up down the column.
    fn field(&self, input: &Entity<InputState>) -> AnyElement {
        div()
            .w(px(FIELD_WIDTH))
            .child(Input::new(input).focus_bordered(false))
            .into_any_element()
    }

    /// A sidebar row that opens and closes the pages under it. Clicking the
    /// row itself does the toggling, which is what the chevron promises.
    fn group_row(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let group = &GROUPS[index];
        let open = self.expanded[index];
        // A heading counts as current while one of its pages is showing, so
        // the open page is never hidden inside a collapsed-looking heading.
        let current = group.pages.contains(&self.page);
        div()
            .id(format!("settings-group-{}", index))
            .role(Role::Button)
            .aria_label(group.title)
            .aria_expanded(open)
            .focusable()
            .tab_index(0)
            .h(px(30.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_sm()
            .cursor_pointer()
            .text_size(ui(12.))
            .text_color(if current {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .hover(|el| el.bg(cx.theme().list_hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.expanded[index] = !this.expanded[index];
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.expanded[index] = !this.expanded[index];
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .xsmall(),
            )
            .child(group.title)
            .into_any_element()
    }

    /// A page under a heading. Indented to sit under it, the way Zed nests
    /// them.
    fn page_row(&self, page: Page, cx: &Context<Self>) -> AnyElement {
        let selected = self.page == page;
        div()
            .id(format!("settings-page-{}", page.key()))
            .role(Role::Button)
            .aria_label(page.title())
            .focusable()
            .tab_index(0)
            .h(px(28.))
            .pl(px(26.))
            .pr_2()
            .flex()
            .items_center()
            .rounded_sm()
            .cursor_pointer()
            .text_size(ui(12.))
            .text_color(if selected {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .when(selected, |el| el.bg(cx.theme().list_active))
            .hover(|el| el.bg(cx.theme().list_hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.page = page;
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.page = page;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .child(page.title())
            .into_any_element()
    }

    /// The sidebar. Same surface as the pane beside it — the split is shown by
    /// the hairline, not by two shades.
    fn render_sidebar(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .w(px(204.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .bg(cx.theme().background)
            .border_r_1()
            .border_color(cx.theme().border)
            .children(GROUPS.iter().enumerate().flat_map(|(index, group)| {
                let open = self.expanded[index];
                std::iter::once(self.group_row(index, cx)).chain(open.then(|| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(group.pages.iter().map(|page| self.page_row(*page, cx)))
                        .into_any_element()
                }))
            }))
            .into_any_element()
    }

    /// The bordered control the settings rows are built from: a rounded
    /// outline with its actions split by hairline dividers, the shape Zed's
    /// settings use. One box reads as one control where loose buttons read as
    /// three, and it keeps the row's hit area in one piece.
    fn control_box(children: Vec<AnyElement>, cx: &Context<Self>) -> AnyElement {
        div()
            .h(px(28.))
            .flex()
            .items_center()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            // So a hovered end segment stops at the rounded corners.
            .overflow_hidden()
            .children(children)
            .into_any_element()
    }

    fn control_divider(cx: &Context<Self>) -> AnyElement {
        div()
            .w(px(1.))
            .h_full()
            .flex_shrink_0()
            .bg(cx.theme().border)
            .into_any_element()
    }

    /// One segment of a control. `selected` only means something in a group
    /// that shows state; a stepper's segments are never selected.
    fn control_segment(
        id: String,
        label: &'static str,
        selected: bool,
        width: Pixels,
        content: AnyElement,
        activate: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        let activate = std::rc::Rc::new(activate);
        let keyboard = activate.clone();
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(if selected {
                format!("{label} (on)")
            } else {
                label.to_string()
            })
            .focusable()
            .tab_index(0)
            .w(width)
            .h_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_size(ui(11.))
            .text_color(if selected {
                cx.theme().accent_foreground
            } else {
                cx.theme().muted_foreground
            })
            .when(selected, |el| el.bg(cx.theme().list_active))
            .hover(|el| {
                el.bg(cx.theme().list_hover)
                    .text_color(cx.theme().foreground)
            })
            .focus_visible(|el| el.text_color(cx.theme().accent_foreground))
            .on_click(cx.listener(move |this, _, window, cx| activate(this, window, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    keyboard(this, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(content)
            .into_any_element()
    }

    /// A numeric setting: the value between a `−` and a `+`, in one box.
    fn stepper(
        &self,
        id: &'static str,
        value: String,
        step: fn(&mut Settings, f32),
        cx: &Context<Self>,
    ) -> AnyElement {
        let button = |id: String, icon: IconName, label: &'static str, delta: f32| {
            Self::control_segment(
                id,
                label,
                false,
                px(30.),
                Icon::new(icon).xsmall().into_any_element(),
                move |this, _, cx| this.change(move |settings| step(settings, delta), cx),
                cx,
            )
        };
        Self::control_box(
            vec![
                button(
                    format!("settings-down-{id}"),
                    IconName::Minus,
                    "Decrease",
                    -1.,
                ),
                Self::control_divider(cx),
                div()
                    .w(px(58.))
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(ui(12.))
                    .child(value)
                    .into_any_element(),
                Self::control_divider(cx),
                button(format!("settings-up-{id}"), IconName::Plus, "Increase", 1.),
            ],
            cx,
        )
    }

    /// A two-choice setting, in the same box as a stepper.
    fn choice(
        &self,
        id: &'static str,
        left: (&'static str, bool, fn(&mut Settings)),
        right: (&'static str, bool, fn(&mut Settings)),
        cx: &Context<Self>,
    ) -> AnyElement {
        let segment = |side: &str, option: (&'static str, bool, fn(&mut Settings))| {
            let (label, selected, set) = option;
            Self::control_segment(
                format!("{id}-{side}"),
                label,
                selected,
                px(72.),
                div().child(label).into_any_element(),
                move |this, _, cx| this.change(set, cx),
                cx,
            )
        };
        Self::control_box(
            vec![
                segment("left", left),
                Self::control_divider(cx),
                segment("right", right),
            ],
            cx,
        )
    }

    /// The rows of the open page, under its title. Everything is in one
    /// column: the window is tall enough for the longest page, so nothing here
    /// scrolls.
    fn render_page(&self, settings: &Settings, cx: &Context<Self>) -> AnyElement {
        let page = self.page;
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_6()
                    .py_4()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_size(ui(15.)).child(page.title()))
                    .child(
                        div()
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(page.subtitle()),
                    ),
            )
            .child(div().h(px(1.)).flex_shrink_0().bg(cx.theme().border))
            .children(match page {
                Page::Interface => vec![
                    Self::row(
                        "Interface font",
                        "Menus, the sidebar and settings. Does not change code.",
                        self.font_picker(&self.font_select, "Interface font"),
                        cx,
                    ),
                    Self::row(
                        "Size",
                        "Scales every label in the app.",
                        self.stepper(
                            "interface-size",
                            format!("{:.0}", settings.font_size),
                            |settings, step| settings.font_size += step,
                            cx,
                        ),
                        cx,
                    ),
                ],
                Page::Type => vec![
                    Self::row(
                        "Code font",
                        "Updates open files and the terminal immediately.",
                        self.font_picker(&self.code_font_select, "Code font"),
                        cx,
                    ),
                    Self::row(
                        "Soft wrap",
                        "Break long lines to the editor width. Off keeps a character grid.",
                        self.choice(
                            "soft-wrap",
                            ("On", settings.soft_wrap, |settings| {
                                settings.soft_wrap = true
                            }),
                            ("Off", !settings.soft_wrap, |settings| {
                                settings.soft_wrap = false
                            }),
                            cx,
                        ),
                        cx,
                    ),
                    Self::row(
                        "Cursor animation",
                        "Whether the cursor smoothly animates when moving around the editor.",
                        self.choice(
                            "cursor-animation",
                            ("On", settings.cursor_animation, |settings| {
                                settings.cursor_animation = true
                            }),
                            ("Off", !settings.cursor_animation, |settings| {
                                settings.cursor_animation = false
                            }),
                            cx,
                        ),
                        cx,
                    ),
                    Self::row(
                        "Code size",
                        "Updates the code in open files immediately.",
                        self.stepper(
                            "code-size",
                            format!("{:.0}", settings.code_font_size),
                            |settings, step| settings.code_font_size += step,
                            cx,
                        ),
                        cx,
                    ),
                ],
                Page::Indentation => vec![
                    Self::row(
                        "Tab size",
                        "Applies to files opened from now on.",
                        self.stepper(
                            "tab-size",
                            settings.tab_size.to_string(),
                            |settings, step| {
                                settings.tab_size =
                                    (settings.tab_size as f32 + step).clamp(1., 8.) as usize
                            },
                            cx,
                        ),
                        cx,
                    ),
                    Self::row(
                        "Indent with",
                        "Spaces or tab characters.",
                        self.choice(
                            "indent",
                            ("Spaces", !settings.hard_tabs, |settings| {
                                settings.hard_tabs = false
                            }),
                            ("Tabs", settings.hard_tabs, |settings| {
                                settings.hard_tabs = true
                            }),
                            cx,
                        ),
                        cx,
                    ),
                ],
                Page::Ignored => vec![Self::row(
                    "Folders",
                    "Comma-separated folder names, hidden from the tree and skipped by search.",
                    self.field(&self.ignore_query),
                    cx,
                )],
            })
            .into_any_element()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.settings.clone();
        for (select, family, default) in [
            (&self.font_select, &settings.font_family, "System default"),
            (
                &self.code_font_select,
                &settings.code_font_family,
                "JetBrains Mono",
            ),
        ] {
            let value = family.as_deref().unwrap_or(default);
            if select.read(cx).selected_value().map(String::as_str) != Some(value) {
                select.update(cx, |select, cx| {
                    select.set_selected_value(&value.to_owned(), window, cx);
                });
            }
        }
        div()
            .id("settings")
            .key_context("FolioSettings")
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_size(ui(13.))
            .on_action(cx.listener(|_, _: &CloseWindow, window, _| window.remove_window()))
            .on_action(cx.listener(|this, _: &Quit, window, cx| {
                if let Some(folio) = this.folio.upgrade() {
                    folio.update(cx, |folio, cx| folio.request(Next::Quit, window, cx));
                }
            }))
            // Drawn by the app rather than AppKit, so it carries the theme's
            // own background in both appearances. The traffic lights are still
            // the real ones, sitting over the transparent title bar area.
            .child(
                TitleBar::new()
                    .bg(cx.theme().background)
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .h_full()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .pr_3()
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child("Settings"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_sidebar(cx))
                    .child(self.render_page(&settings, cx)),
            )
    }
}
