//! The pane header (breadcrumb, caret, language, encoding, indentation,
//! line endings) and the caret view it embeds. Plain muted text, no chips.

use super::*;

/// Height of the row over a pane's code.
pub(super) const PANE_HEADER_HEIGHT: f32 = 32.;

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
    /// The row over a pane's code: the file's place in the project on the
    /// left; caret, language, encoding, indentation and line endings on the
    /// right. Branch and change count live in the title bar's command bar.
    pub(super) fn render_pane_header(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let Some(path) = self.project.panes.get(index).and_then(|p| p.active.clone()) else {
            return div().into_any_element();
        };
        let doc = self.project.documents.get(&path);
        let parts: Vec<String> = self
            .project
            .workspace
            .as_ref()
            .and_then(|w| path.strip_prefix(&w.root).ok())
            .map(|rel| {
                rel.iter()
                    .map(|part| part.to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_else(|| vec![name(&path)]);
        let last = parts.len().saturating_sub(1);
        let crumbs = div()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .overflow_hidden()
            .children(parts.into_iter().enumerate().flat_map(|(i, part)| {
                let sep = (i > 0).then(|| {
                    div()
                        .text_color(muted.opacity(0.6))
                        .child("›")
                        .into_any_element()
                });
                let label = div()
                    .when(i == last, |el| el.text_color(cx.theme().foreground))
                    .child(part)
                    .into_any_element();
                sep.into_iter().chain(std::iter::once(label))
            }));
        div()
            .id(("pane-header", index))
            .h(px(PANE_HEADER_HEIGHT))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(16.))
            .px(px(16.))
            .text_size(ui(11.))
            .text_color(muted)
            .child(crumbs)
            .child(div().flex_1())
            .when_some(doc, |el, doc| {
                let language = if doc.large {
                    "text"
                } else {
                    buffer::language(&path)
                };
                el.when(self.project.focused == index, |el| {
                    el.child(self.cursor_status.clone())
                })
                .child(language_label(language))
                .child("UTF-8")
                .child(doc.indent_label.clone())
                .child(doc.line_ending)
            })
            .into_any_element()
    }
}
