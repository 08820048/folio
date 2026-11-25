//! The command palette (`⇧⌘P`, or `>` in quick open): every command Folio
//! has, by name, with its shortcut, run by dispatching the same action a
//! key binding or a menu item would.

use super::*;

/// One palette entry. `action` builds the action fresh each time, since
/// GPUI dispatches a boxed value.
pub(super) struct Command {
    pub(super) name: &'static str,
    pub(super) action: fn() -> Box<dyn Action>,
}

macro_rules! commands {
    ($($name:literal => $action:expr),* $(,)?) => {
        &[$(Command { name: $name, action: || Box::new($action) }),*]
    };
}

/// Everything the palette offers, in the order shown for an empty query
/// (after the recently run ones). Names read the way the menus do.
pub(super) const COMMANDS: &[Command] = commands![
    "Quick Open…" => QuickOpen,
    "Go to Line…" => GoToLine,
    "Find in Project…" => ProjectSearch,
    "Replace in Project…" => ProjectReplace,
    "Save" => Save,
    "Save All" => SaveAll,
    "Run" => Run,
    "Run Again" => Rerun,
    "Stop" => StopRun,
    "Focus Mode" => ToggleFocus,
    "Toggle Side Panel" => ToggleSidebar,
    "Show Changes" => ToggleChanges,
    "Show File Changes (Diff)" => ToggleDiff,
    "Next Change" => NextHunk,
    "Previous Change" => PrevHunk,
    "Next Changed File" => NextChangeFile,
    "Previous Changed File" => PrevChangeFile,
    "Toggle Terminal" => ToggleTerminal,
    "Close Terminal" => CloseTerminal,
    "Toggle Blame" => ToggleBlame,
    "Toggle Markdown Preview" => ToggleMarkdownPreview,
    "Split Editor" => SplitEditor,
    "Split Editor Left" => SplitEditorLeft,
    "Split Editor Up" => SplitEditorUp,
    "Split Editor Down" => SplitEditorDown,
    "Join Editors" => JoinEditor,
    "Next Tab" => NextTab,
    "Previous Tab" => PrevTab,
    "Reopen Closed Tab" => ReopenClosedTab,
    "Toggle Comment" => ToggleComment,
    "Toggle Block Comment" => ToggleBlockComment,
    "Duplicate Lines" => DuplicateLines,
    "Move Lines Up" => MoveLinesUp,
    "Move Lines Down" => MoveLinesDown,
    "Fold" => Fold,
    "Unfold" => Unfold,
    "Select Next Occurrence" => SelectNextOccurrence,
    "Select All Occurrences" => SelectAllOccurrences,
    "Add Cursor Above" => AddCursorAbove,
    "Add Cursor Below" => AddCursorBelow,
    "Reveal in Finder" => RevealInFinder,
    "Open in Terminal" => OpenInTerminal,
    "Copy File Path" => CopyFilePath,
    "Copy Relative File Path" => CopyRelativeFilePath,
    "Zoom In" => ZoomIn,
    "Zoom Out" => ZoomOut,
    "Actual Size" => ZoomReset,
    "Open Project…" => OpenProject,
    "New Project…" => NewProject,
    "Clone from GitHub…" => CloneFromGithub,
    "Close Project" => CloseProject,
    "Settings…" => OpenSettings,
    "Check for Updates…" => CheckForUpdates,
    "About Folio" => OpenAbout,
];

/// How well `query` (lowercase) matches `name`: `None` when its characters
/// do not all appear in order. Higher is better: a run of consecutive
/// characters, a match at the start of a word, and a shorter name all help.
pub(super) fn score(name: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let name: Vec<char> = name.to_lowercase().chars().collect();
    let mut score = 0;
    let mut at = 0;
    let mut previous: Option<usize> = None;
    for wanted in query.chars().filter(|c| !c.is_whitespace()) {
        let found = (at..name.len()).find(|&i| name[i] == wanted)?;
        let word_start = found == 0 || !name[found - 1].is_alphanumeric();
        score += if previous == Some(found.wrapping_sub(1)) {
            6
        } else if word_start {
            8
        } else {
            1
        };
        previous = Some(found);
        at = found + 1;
    }
    Some(score * 4 - name.len() as i32)
}

/// The shortcut that runs `action`, written out (`⌘K Z` for a chord). Looked
/// up by key context rather than by focus: the palette's list is built while
/// the frame's dispatch tree is, so "what would the focused element do" has
/// no answer yet.
pub(super) fn shortcut(action: &dyn Action, window: &Window) -> Option<String> {
    let mut stack = Vec::new();
    for name in ["Folio", "FolioEditor"] {
        stack.push(KeyContext::parse(name).ok()?);
        if let Some(binding) =
            window.highest_precedence_binding_for_action_in_context(action, stack.last()?.clone())
        {
            return Some(
                binding
                    .keystrokes()
                    .iter()
                    .map(|key| gpui_component::kbd::Kbd::format(key.as_keystroke()))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
    }
    None
}

impl Folio {
    /// The palette is quick open with a `>` in front.
    pub(super) fn show_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.workspace.is_none() || self.project_loading || self.prompting {
            return;
        }
        self.clone_open = false;
        self.panel = Some(Panel::Files);
        self.query.update(cx, |query, cx| {
            query.set_value(">", window, cx);
            query.focus(window, cx);
        });
        self.filter(cx);
        cx.notify();
    }

    /// Fill `command_matches` for the text after the `>`.
    pub(super) fn filter_commands(&mut self, query: &str) {
        let query = query.trim().to_lowercase();
        let mut ranked: Vec<(i32, usize)> = COMMANDS
            .iter()
            .enumerate()
            .filter_map(|(index, command)| {
                let recent = self
                    .recent_commands
                    .iter()
                    .position(|name| *name == command.name)
                    .map(|rank| 1000 - rank as i32)
                    .unwrap_or(0);
                score(command.name, &query).map(|score| {
                    // An empty query is recent first, then table order.
                    let order = if query.is_empty() {
                        recent * 1000 - index as i32
                    } else {
                        score * 10 + recent.min(1) * 5
                    };
                    (order, index)
                })
            })
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        self.command_matches = ranked.into_iter().map(|(_, index)| index).collect();
        self.match_selected = 0;
    }

    /// Run the selected command: close the palette first so the action
    /// reaches the editor it was meant for, then dispatch it.
    pub(super) fn run_selected_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(command) = self
            .command_matches
            .get(self.match_selected)
            .and_then(|index| COMMANDS.get(*index))
        else {
            return;
        };
        self.recent_commands.retain(|name| *name != command.name);
        self.recent_commands.insert(0, command.name);
        self.recent_commands.truncate(8);
        self.close_panel(window, cx);
        window.dispatch_action((command.action)(), cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn scoring_prefers_word_starts_and_runs() {
        assert!(score("Toggle Terminal", "tt").is_some());
        assert!(score("Save", "sx").is_none());
        let save = score("Save", "sa").unwrap();
        let save_all = score("Save All", "sa").unwrap();
        assert!(save > save_all, "the shorter exact name first");
        let split = score("Split Editor", "se").unwrap();
        let select = score("Select All Occurrences", "se").unwrap();
        assert!(split > 0 && select > 0);
        assert!(
            score("Toggle Comment", "comment").unwrap()
                > score("Toggle Block Comment", "comment").unwrap()
        );
    }

    #[core::prelude::v1::test]
    fn every_command_name_is_unique() {
        let mut names: Vec<_> = COMMANDS.iter().map(|c| c.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len());
    }
}
