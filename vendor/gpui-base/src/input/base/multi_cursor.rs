//! More than one cursor: the commands that build a set of selections, and the
//! edit that lands at every one of them.
//!
//! The three parts of a multi-cursor are split by where they belong. The set
//! itself is [`Selections`], in `cursor.rs`. Drawing it is `element.rs`. This
//! is what the editor *does* with it, together with the three hooks in
//! `state.rs` where a keystroke, a deletion and an escape arrive.
//!
//! What is here is deliberately a closed set: the commands that make more than
//! one selection, the edit that applies to all of them, and the command that
//! takes it back down to one. A movement or a click collapses the set — see
//! [`Selections::select_only`] — so there is no state where a caret is somewhere
//! the cursor commands do not know about.

use std::ops::Range;

use gpui::{Context, EntityInputHandler as _, Window};
use sum_tree::Bias;

use super::{InputBaseState, Selection};
use crate::input::change::Change;
use crate::input::{RopeExt as _, SearchMatcher};

/// How many selections a command will make before it stops.
///
/// Each one is a caret to draw and a range to edit, and select-all-occurrences
/// on a common word in a large file asks for thousands without meaning to.
/// Editors that have this feature all stop somewhere; this is where.
const MAX_SELECTIONS: usize = 1000;

/// The commands, and the two small things they need to know about the text.
impl InputBaseState {
    /// Every selection, in the order of the text. One entry for an ordinary
    /// editor.
    pub fn selected_ranges(&self) -> Vec<Range<usize>> {
        self.selections
            .in_text_order()
            .iter()
            .map(|s| Range::from(*s))
            .collect()
    }

    /// What the cursor commands multiply: the primary selection, or the word
    /// the caret is in when nothing is selected.
    fn occurrence_target(&self) -> Option<Range<usize>> {
        let primary = self.selections.primary();
        if !primary.is_empty() {
            return Some(primary.into());
        }
        self.text.word_range(self.cursor())
    }

    /// Every occurrence of `needle`, in the order they appear.
    ///
    /// The editor's own search matcher does the finding, so a literal string
    /// here means what it means in the find panel.
    fn occurrences(&self, needle: &str) -> Vec<Range<usize>> {
        let mut matcher = SearchMatcher::new();
        matcher.update(&self.text);
        matcher.update_query(needle, false);
        let mut ranges: Vec<Range<usize>> = matcher.matched_ranges().as_ref().clone();
        ranges.truncate(MAX_SELECTIONS);
        ranges
    }

    /// `⌘D`: the word under the caret, then each next place it appears.
    ///
    /// The first press only selects the word — one selection is not a
    /// multi-cursor, and that is also what the key does in every editor that
    /// has it.
    pub fn select_next_occurrence(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.occurrence_target() else {
            return;
        };
        let needle = self.text.slice(target.clone()).to_string();
        if needle.is_empty() {
            return;
        }
        let occurrences = self.occurrences(&needle);

        if self.selections.len() == 1 && self.selections.primary().is_empty() {
            self.selections.select_only(target.clone());
            self.update_preferred_column();
            cx.notify();
            return;
        }
        if self.selections.len() >= MAX_SELECTIONS {
            return;
        }

        // The next one after the caret, and failing that the first one that is
        // not already in the set: pressing the key at the last occurrence
        // wraps, and stops when nothing is left to add.
        let taken = self.selected_ranges();
        let from = self.selections.primary().end;
        let next = occurrences
            .iter()
            .find(|range| range.start >= from && !taken.contains(range))
            .or_else(|| occurrences.iter().find(|range| !taken.contains(range)));
        let Some(next) = next.cloned() else {
            return;
        };

        self.selections.add(next.clone());
        self.scroll_to(next.end, None, cx);
        self.pause_blink_cursor(cx);
        self.update_preferred_column();
        cx.notify();
    }

    /// `⌘⇧L`: every occurrence of the selection, across the whole file.
    ///
    /// The selection the user was reading stays the primary, so the view does
    /// not jump to the last one in the file.
    pub fn select_all_occurrences(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.occurrence_target() else {
            return;
        };
        let needle = self.text.slice(target.clone()).to_string();
        if needle.is_empty() {
            return;
        }
        let occurrences = self.occurrences(&needle);
        if occurrences.is_empty() {
            return;
        }

        self.selections
            .replace_all_keeping(occurrences, target.clone());
        self.scroll_to(target.end, None, cx);
        self.pause_blink_cursor(cx);
        self.update_preferred_column();
        cx.notify();
    }

    /// `⌥⌘↑`: another caret on the line above.
    pub fn add_cursor_above(&mut self, cx: &mut Context<Self>) {
        self.add_cursor_on_line(-1, cx);
    }

    /// `⌥⌘↓`: another caret on the line below.
    pub fn add_cursor_below(&mut self, cx: &mut Context<Self>) {
        self.add_cursor_on_line(1, cx);
    }

    /// A caret on the neighbouring line, outside the outermost one, in the
    /// column it was in.
    ///
    /// Outside the outermost rather than beside each: this is the key that
    /// grows a block one line at a time, and growing from the newest caret
    /// outwards is what does that.
    fn add_cursor_on_line(&mut self, step: isize, cx: &mut Context<Self>) {
        if self.mode.is_single_line() || self.selections.len() >= MAX_SELECTIONS {
            return;
        }
        let anchor = if step < 0 {
            self.selections.all().iter().map(|s| s.start).min()
        } else {
            self.selections.all().iter().map(|s| s.end).max()
        };
        let Some(anchor) = anchor else {
            return;
        };

        let point = self.text.offset_to_point(anchor);
        let neighbour = point.row as isize + step;
        if neighbour < 0 || neighbour as usize >= self.text.lines_len() {
            return;
        }
        let neighbour = neighbour as usize;
        // The column is a byte offset within the line, so it has to land on a
        // character boundary of the line it lands on — the neighbouring line
        // can be shorter, and can have wider characters under the same bytes.
        let column = point.column.min(self.text.line_len(neighbour));
        let offset = self.text.line_start_offset(neighbour) + column;
        let offset = self.text.clip_offset(offset, Bias::Left);

        self.selections.add(Selection::new(offset, offset));
        self.scroll_to(offset, None, cx);
        self.pause_blink_cursor(cx);
        self.update_preferred_column();
        cx.notify();
    }

    /// Back to one cursor, the one the caret was in.
    ///
    /// Answers whether there was a set to collapse, so the caller can let the
    /// key through when there was not.
    pub fn collapse_selections(&mut self, cx: &mut Context<Self>) -> bool {
        if self.selections.len() <= 1 {
            return false;
        }
        self.selections.select_only(self.selections.primary());
        self.selected_word_range = None;
        self.pause_blink_cursor(cx);
        self.update_preferred_column();
        cx.notify();
        true
    }
}

/// Editing through the set.
///
/// These are what a keystroke, a deletion and a paste become when there is more
/// than one selection. They are public because they are the way an application
/// types into a multi-cursor editor itself; the platform's own path reaches
/// them from the middle of `state.rs`.
impl InputBaseState {
    /// `⌫` and `⌦` with more than one cursor: each one grows over the character
    /// next to it, and then every one of them deletes.
    pub fn delete_at_every_selection(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selections: Vec<Selection> = self
            .selections
            .all()
            .iter()
            .map(|selection| {
                if !selection.is_empty() {
                    // With something selected there is nothing to grow over:
                    // the selection is what goes.
                    return *selection;
                }
                if forward {
                    Selection::new(
                        selection.end,
                        self.next_boundary(selection.end).max(selection.end),
                    )
                } else {
                    Selection::new(self.previous_boundary(selection.start), selection.start)
                }
            })
            .collect();
        self.selections.replace_all(selections);
        self.replace_in_every_selection("", window, cx);
    }

    /// `Enter` with more than one cursor: a line break at each, indented to the
    /// line it breaks and laid out around a bracket pair, exactly as it is for
    /// one cursor.
    pub fn insert_line_break_at_every_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let breaks: Vec<(String, usize)> = self
            .selections
            .in_text_order()
            .iter()
            .map(|selection| self.line_break_at(*selection))
            .collect();
        self.replace_in_every_selection_with(&breaks, window, cx);
    }

    /// The same text at every selection. What typing and pasting are.
    pub fn replace_in_every_selection(
        &mut self,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let texts = vec![(new_text.to_string(), new_text.len()); self.selections.len()];
        self.replace_in_every_selection_with(&texts, window, cx);
    }

    /// One edit at every selection, as one step on the undo stack.
    ///
    /// Each edit is the text to insert and where in it the caret belongs —
    /// usually the end of it, but not always, since a line break between a pair
    /// of brackets puts the caret in the middle of what it inserted.
    ///
    /// The edits have to run last selection first, or each one would move the
    /// bytes the ones before it are addressed by. The undo stack is turned off
    /// for them and a single change covering the whole span is recorded
    /// instead: undoing a multi-cursor edit is undoing one edit, and the order
    /// the editor's history replays a batch in is only correct for the edits
    /// that were recorded one after another.
    fn replace_in_every_selection_with(
        &mut self,
        texts: &[(String, usize)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_editable() || texts.len() != self.selections.len() {
            return;
        }
        let selections: Vec<Selection> = self.selections.in_text_order();
        if selections.len() < 2 {
            return;
        }
        let span = selections[0].start..selections[selections.len() - 1].end;
        let before = self.text.clone();

        let was_ignoring = self.history.is_ignoring();
        self.history.set_ignoring(true);
        // A completion popup belongs to one caret; opening one per selection
        // would leave the last one standing over an edit it never made.
        let was_silent = self.silent_replace_text;
        self.silent_replace_text = true;

        let mut carets = Vec::with_capacity(selections.len());
        for (selection, (text, caret)) in selections.iter().zip(texts).rev() {
            let range = self.range_to_utf16(&Range::from(*selection));
            self.replace_text_in_range(Some(range), text, window, cx);
            // The edit leaves the caret at the end of what it inserted; where
            // it belongs may be earlier in it.
            let tail = text.len() - caret;
            carets.push(self.selections.primary().end.saturating_sub(tail));
        }

        self.silent_replace_text = was_silent;
        self.history.set_ignoring(was_ignoring);
        // Collected right to left, so put them back in the order of the text,
        // and move each one by what the edits before it changed the length by:
        // a caret is where it ends up in the finished text, and until the loop
        // has passed the ones to its left it is only correct for the text as it
        // stood at the time.
        carets.reverse();
        let mut shift = 0isize;
        for (caret, (selection, (text, _))) in carets.iter_mut().zip(selections.iter().zip(texts)) {
            *caret = (*caret as isize + shift).max(0) as usize;
            shift += text.len() as isize - selection.len() as isize;
        }

        let new_span = span.start..*carets.last().expect("one per selection");
        let old_text = before.slice(span.clone()).to_string();
        let new_text = self.text.slice(new_span.clone()).to_string();
        self.history
            .push(Change::new(span, &old_text, new_span, &new_text));
        self.history.end_grouping();

        self.selections.carets(carets);
        self.ime_marked_range.take();
        self.update_preferred_column();
        cx.notify();
    }
}
