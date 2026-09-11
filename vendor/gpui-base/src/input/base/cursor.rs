use std::ops::{Range, RangeBounds};

/// A selection in the text, represented by start and end byte indices.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    pub start: usize,
    pub end: usize,
}

impl Selection {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Clears the selection, setting start and end to 0.
    pub fn clear(&mut self) {
        self.start = 0;
        self.end = 0;
    }

    /// Checks if the given offset is within the selection range.
    pub fn contains(&self, offset: usize) -> bool {
        offset >= self.start && offset < self.end
    }
}

/// Every selection the editor is editing through.
///
/// Never empty — an editor with nothing selected still has a cursor, which is a
/// selection whose range is empty. In the order of the text, and never
/// overlapping, with one exception: the last one is the one the cursor is in,
/// and a command that has to say which one that is moves it to the end from
/// wherever it was. Read it through [`Selections::in_text_order`] when the
/// order is what matters rather than which one the cursor is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selections(Vec<Selection>);

impl Default for Selections {
    fn default() -> Self {
        Self(vec![Selection::default()])
    }
}

impl Selections {
    /// The one the cursor is in.
    pub fn primary(&self) -> Selection {
        // The invariant is that this is never empty. A default here keeps a
        // broken one from taking a render down with it.
        self.0.last().copied().unwrap_or_default()
    }

    pub fn primary_mut(&mut self) -> &mut Selection {
        if self.0.is_empty() {
            self.0.push(Selection::default());
        }
        self.0.last_mut().expect("just pushed")
    }

    /// All of them, in order.
    pub fn all(&self) -> &[Selection] {
        &self.0
    }

    /// All of them, in the order of the text, whichever one is primary.
    ///
    /// What an edit through the set and the clipboard read: a caret is a
    /// position in the text, and the text does not care which one is primary.
    pub fn in_text_order(&self) -> Vec<Selection> {
        let mut all = self.0.clone();
        all.sort_by_key(|selection| (selection.start, selection.end));
        all
    }

    /// How many.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Add one, keeping the set ordered and non-overlapping, and make it the
    /// one the cursor is in: the selection just made is the one the next
    /// keystroke should be read against.
    pub fn add(&mut self, selection: impl Into<Selection>) {
        let selection = selection.into();
        self.0.push(selection);
        self.reorder(Some(selection));
    }

    /// Replace the set wholesale, under the same invariant. The last one — the
    /// rightmost — ends up as the primary.
    pub fn replace_all(&mut self, selections: impl IntoIterator<Item = impl Into<Selection>>) {
        self.0 = selections.into_iter().map(Into::into).collect();
        self.reorder(None);
    }

    /// Replace the set wholesale, keeping `primary` as the one the cursor is
    /// in.
    ///
    /// "The last one" and "the one the cursor is in" are the same slot, so a
    /// command that multiplies the selection the user was reading — select all
    /// occurrences — has to put that one back last, or the view would follow
    /// the rightmost match in the file instead of staying where it was.
    pub fn replace_all_keeping(
        &mut self,
        selections: impl IntoIterator<Item = impl Into<Selection>>,
        primary: impl Into<Selection>,
    ) {
        self.0 = selections.into_iter().map(Into::into).collect();
        self.reorder(Some(primary.into()));
    }

    /// Replace the set with an empty selection at each of these offsets, which
    /// is what an edit that applied to every selection leaves behind.
    pub fn carets(&mut self, offsets: impl IntoIterator<Item = usize>) {
        self.0 = offsets
            .into_iter()
            .map(|offset| Selection::new(offset, offset))
            .collect();
        self.reorder(None);
    }

    /// Sort, then fold anything that now overlaps into one selection, then
    /// move `primary` to the end if it survived.
    ///
    /// Overlap, not adjacency: two selections that merely touch — the two `ab`s
    /// of `abab` — are two selections, and typing over them has to give `ab`'s
    /// worth of text twice. Only a shared byte is a question with one answer.
    fn reorder(&mut self, primary: Option<Selection>) {
        if self.0.is_empty() {
            self.0.push(Selection::default());
            return;
        }
        // Direction lives on the editor, not in here, so an entry is always the
        // bytes it covers, low end first.
        for selection in &mut self.0 {
            if selection.start > selection.end {
                std::mem::swap(&mut selection.start, &mut selection.end);
            }
        }
        self.0.sort_by_key(|selection| (selection.start, selection.end));
        let mut merged: Vec<Selection> = Vec::with_capacity(self.0.len());
        for selection in self.0.drain(..) {
            match merged.last_mut() {
                Some(last) if selection.start < last.end => {
                    last.end = last.end.max(selection.end);
                }
                _ => merged.push(selection),
            }
        }
        self.0 = merged;

        if let Some(primary) = primary {
            // Containment rather than equality: the one asked for may have
            // been folded into a neighbour, and that neighbour is now it.
            if let Some(index) = self
                .0
                .iter()
                .position(|s| s.start <= primary.start && primary.end <= s.end)
            {
                let primary = self.0.remove(index);
                self.0.push(primary);
            }
        }
    }

    /// Collapse to one selection. What a click, a movement, or anything else
    /// that is not a cursor command does.
    pub fn select_only(&mut self, selection: impl Into<Selection>) {
        self.0.clear();
        self.0.push(selection.into());
    }
}

impl From<Range<usize>> for Selections {
    fn from(value: Range<usize>) -> Self {
        Self(vec![value.into()])
    }
}

impl From<Selection> for Selections {
    fn from(value: Selection) -> Self {
        Self(vec![value])
    }
}

impl From<Selections> for Range<usize> {
    fn from(value: Selections) -> Self {
        value.primary().into()
    }
}

impl From<Range<usize>> for Selection {
    fn from(value: Range<usize>) -> Self {
        Self::new(value.start, value.end)
    }
}
impl From<Selection> for Range<usize> {
    fn from(value: Selection) -> Self {
        value.start..value.end
    }
}
impl RangeBounds<usize> for Selection {
    fn start_bound(&self) -> std::ops::Bound<&usize> {
        std::ops::Bound::Included(&self.start)
    }

    fn end_bound(&self) -> std::ops::Bound<&usize> {
        std::ops::Bound::Excluded(&self.end)
    }
}

#[cfg(test)]
mod tests {
    use crate::input::Position;

    #[test]
    fn test_line_column_from_to() {
        assert_eq!(
            Position::new(1, 2),
            Position {
                line: 1,
                character: 2
            }
        );
    }
}
