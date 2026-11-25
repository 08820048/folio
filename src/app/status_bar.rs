//! The status bar under the workspace card: branch and change count on the
//! left; caret, language, encoding, indentation and line endings on the
//! right. Plain muted text, no separators or chips (Black Glass design).

use super::*;

/// Height of the status bar strip.
pub(super) const STATUS_BAR_HEIGHT: f32 = 26.;

/// "Ln 19, Col 42" for the active editor.
///
/// The caret moves without any event Folio listens to, so this view
/// observes the active editor's input state itself and redraws alone.
pub(super) struct CursorStatus {
    folio: WeakEntity<Folio>,
    watching: Option<EntityId>,
    _observe: Option<Subscription>,
}

impl CursorStatus {
    pub(super) fn new(folio: WeakEntity<Folio>) -> Self {
        Self {
            folio,
            watching: None,
            _observe: None,
        }
    }
}

impl Render for CursorStatus {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.folio.upgrade().and_then(|folio| {
            let folio = folio.read(cx);
            let path = folio.project.active()?;
            if folio.project.diff.is_some() {
                return None;
            }
            Some(folio.project.documents.get(path)?.editor.clone())
        });
        let Some(editor) = editor else {
            self.watching = None;
            self._observe = None;
            return div();
        };
        let base = editor.read(cx).base_state().clone();
        if self.watching != Some(base.entity_id()) {
            self.watching = Some(base.entity_id());
            self._observe = Some(cx.observe(&base, |_, _, cx| cx.notify()));
        }
        let offset = base
            .read(cx)
            .selected_ranges()
            .last()
            .map(|range| range.end)
            .unwrap_or(0);
        let point = editor.read(cx).snapshot().offset_to_point(offset);
        div().child(format!("Ln {}, Col {}", point.row + 1, point.column + 1))
    }
}

/// A language id from `buffer::language` as people write it.
pub(super) fn language_label(id: &str) -> SharedString {
    let known = match id {
        "text" => "Plain Text",
        "javascript" => "JavaScript",
        "typescript" => "TypeScript",
        "tsx" => "TSX",
        "json" => "JSON",
        "html" => "HTML",
        "css" => "CSS",
        "scss" => "SCSS",
        "sql" => "SQL",
        "yaml" => "YAML",
        "toml" => "TOML",
        "graphql" => "GraphQL",
        "php" => "PHP",
        "cpp" => "C++",
        "csharp" => "C#",
        "cmake" => "CMake",
        "ini" => "INI",
        "asm" => "Assembly",
        "powershell" => "PowerShell",
        "markdown" | "markdown_inline" => "Markdown",
        "dockerfile" => "Dockerfile",
        "make" => "Makefile",
        "gitignore" => "Git Ignore",
        "protobuf" => "Protocol Buffers",
        _ => "",
    };
    if !known.is_empty() {
        return known.into();
    }
    let mut chars = id.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()).into(),
        None => "Plain Text".into(),
    }
}

impl Folio {
    pub(super) fn render_status_bar(&self, cx: &Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let changed = self.project.git_status.len();
        let doc = self
            .project
            .active()
            .and_then(|path| self.project.documents.get(path).map(|doc| (path, doc)));
        div()
            .id("status-bar")
            .h(px(STATUS_BAR_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(18.))
            .pl(px(if self.activity_bar {
                ACTIVITY_BAR_WIDTH + WORKSPACE_RAIL_GAP + 8.
            } else {
                WORKSPACE_INSET + 8.
            }))
            .pr(px(WORKSPACE_INSET + 8.))
            .text_size(ui(11.))
            .text_color(muted)
            .when_some(self.project.git_branch.clone(), |el, branch| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            Self::glass(GlassIcon::Branch, 13., glass_tone(false, cx)).opacity(0.8),
                        )
                        .child(
                            div()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(branch),
                        ),
                )
            })
            .when(changed > 0, |el| {
                el.child(
                    div()
                        .id("status-changed")
                        .cursor_pointer()
                        .text_color(git_modified_color(cx))
                        .hover(|el| el.text_color(cx.theme().foreground))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_changes_list(cx)))
                        .child(format!("{changed} changed")),
                )
            })
            .child(div().flex_1())
            .when_some(doc, |el, (path, doc)| {
                let language = if doc.large {
                    "text"
                } else {
                    buffer::language(path)
                };
                el.child(self.cursor_status.clone())
                    .child(language_label(language))
                    .child("UTF-8")
                    .child(doc.indent_label.clone())
                    .child(doc.line_ending)
            })
            .into_any_element()
    }
}
