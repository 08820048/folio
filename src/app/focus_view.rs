//! Focus mode's floating parts: the dock that holds what the side panel
//! and the title bar hold in the daily layout, and the Run pill above it.

use super::*;

impl Folio {
    /// The editor has a selection or more than one caret, so Esc is its.
    pub(super) fn active_has_selection(&self, cx: &App) -> bool {
        let Some(doc) = self
            .project
            .active()
            .and_then(|path| self.project.documents.get(path))
        else {
            return false;
        };
        let base = doc.editor.read(cx).base_state().read(cx);
        let ranges = base.selected_ranges();
        ranges.len() > 1 || ranges.iter().any(|range| !range.is_empty())
    }

    /// One dock entry: a glass icon on a 40pt target, the name as a tooltip.
    fn dock_button(
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
            .focusable()
            .tab_index(0)
            .relative()
            .size(px(40.))
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
                Self::glass(icon, 26., glass_tone(active, cx)).when(!active, |el| {
                    el.opacity(0.75).group_hover(id, |style| style.opacity(1.))
                }),
            )
            .when(active, |el| {
                el.child(
                    div()
                        .absolute()
                        .bottom(px(1.))
                        .left(px(18.))
                        .size(px(4.))
                        .rounded_full()
                        .bg(cx.theme().accent_foreground),
                )
            })
            .into_any_element()
    }

    /// A floating sheet: paper, a hairline and a soft drop. GPUI cannot
    /// blur what is behind an element, so it is opaque.
    fn floating(cx: &Context<Self>) -> Div {
        div()
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .shadow_lg()
    }

    /// The bottom-centre dock in focus mode, and the Run pill over it while
    /// something runs.
    pub(super) fn render_focus_dock(&self, cx: &mut Context<Self>) -> AnyElement {
        let running = self.run_is_busy(cx);
        let changed = self.project.git_status.len();
        let command = self.active_run_command();
        let dock = Self::floating(cx)
            .flex()
            .items_center()
            .gap_1()
            .px(px(10.))
            .py(px(6.))
            .rounded(px(18.))
            .child(Self::dock_button(
                "dock-files",
                GlassIcon::Files,
                "Files",
                false,
                |this, window, cx| {
                    this.set_focus(false, window, cx);
                    this.show_side(SidebarKind::Tree, window, cx);
                },
                cx,
            ))
            .child(Self::dock_button(
                "dock-quick-open",
                GlassIcon::QuickOpen,
                "Quick Open",
                self.panel == Some(Panel::Files),
                |this, window, cx| this.show_quick_open(false, window, cx),
                cx,
            ))
            .child(Self::dock_button(
                "dock-search",
                GlassIcon::Search,
                "Search",
                self.panel == Some(Panel::Search),
                |this, window, cx| this.show_search(false, window, cx),
                cx,
            ))
            .child(
                div()
                    .relative()
                    .child(Self::dock_button(
                        "dock-changes",
                        GlassIcon::Changes,
                        "Changes",
                        false,
                        |this, window, cx| {
                            this.set_focus(false, window, cx);
                            this.show_side(SidebarKind::Changes, window, cx);
                        },
                        cx,
                    ))
                    .when(changed > 0, |el| {
                        el.child(
                            div()
                                .absolute()
                                .top(px(2.))
                                .right(px(2.))
                                .min_w(px(14.))
                                .h(px(14.))
                                .px(px(3.))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(git_modified_color(cx))
                                .text_color(cx.theme().background)
                                .text_size(px(9.))
                                .font_weight(FontWeight::BOLD)
                                .child(changed.to_string()),
                        )
                    }),
            )
            .child(Self::dock_button(
                "dock-terminal",
                GlassIcon::Terminal,
                "Terminal",
                false,
                |this, window, cx| {
                    this.set_focus(false, window, cx);
                    if !this.terminal_open {
                        this.toggle_terminal(window, cx);
                    }
                },
                cx,
            ))
            .child(div().w(px(1.)).h(px(24.)).mx(px(6.)).bg(cx.theme().border))
            .when(self.can_run_active(cx) || running, |el| {
                el.child(Self::dock_button(
                    "dock-run",
                    GlassIcon::Run,
                    if running { "Stop" } else { "Run" },
                    running,
                    |this, window, cx| this.run_or_stop(None, window, cx),
                    cx,
                ))
            })
            .child(Self::dock_button(
                "dock-settings",
                GlassIcon::Settings,
                "Settings",
                false,
                |this, _, cx| this.show_settings(cx),
                cx,
            ));
        div()
            .absolute()
            .bottom(px(24.))
            .left_0()
            .right_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.))
            .when(running, |el| {
                el.child(
                    Self::floating(cx)
                        .h(px(32.))
                        .pl(px(12.))
                        .pr(px(6.))
                        .rounded(px(16.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .text_size(ui(12.))
                        .child(div().size(px(6.)).rounded_full().bg(cx.theme().success))
                        .child(div().font_weight(FontWeight::MEDIUM).child("Running"))
                        .when_some(command, |el, command| {
                            el.child(
                                div()
                                    .max_w(px(260.))
                                    .truncate()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_size(ui(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(command),
                            )
                        })
                        .child(
                            div()
                                .id("focus-show-output")
                                .cursor_pointer()
                                .text_color(cx.theme().accent_foreground)
                                .hover(|el| el.opacity(0.8))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.set_focus(false, window, cx);
                                    if !this.terminal_open {
                                        this.toggle_terminal(window, cx);
                                    }
                                }))
                                .child("Show output"),
                        )
                        .child(
                            div()
                                .id("focus-stop")
                                .cursor_pointer()
                                .h(px(22.))
                                .px_2()
                                .rounded_full()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .text_color(run_stop_color(cx))
                                .font_weight(FontWeight::MEDIUM)
                                .hover(|el| el.opacity(0.8))
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.stop_run(window, cx)),
                                )
                                .child(div().size(px(7.)).rounded(px(1.5)).bg(run_stop_color(cx)))
                                .child("Stop"),
                        ),
                )
            })
            .child(dock)
            .into_any_element()
    }
}
