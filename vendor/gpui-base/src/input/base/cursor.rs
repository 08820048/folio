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
/// Ordered and never overlapping, and never empty: an editor with nothing
/// selected still has a cursor, which is a selection whose range is empty. The
/// last one is the one the cursor is in — what a movement, a query or a plain
/// read means. The editing paths take all of them.
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
