//! Rainbow pairs of `()`, `[]` and `{}`.
//!
//! Strings and comments are skipped so a parenthesis in `"foo(bar)"` does not
//! steal a colour from the code around it. Unmatched brackets are left alone.

/// One matched pair. `open` and `close` are UTF-8 byte offsets of the
/// brackets themselves. `depth` is how many pairs wrap this one, starting
/// at 0, and is what a rainbow colour is chosen from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub open: usize,
    pub close: usize,
    pub depth: usize,
}

/// Every matched pair in `text`, inner pairs after the ones that wrap them.
pub fn pairs(text: &str) -> Vec<Pair> {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut stack: Vec<(u8, usize, usize)> = Vec::new();
    let mut out = Vec::new();
    while i < bytes.len() {
        match bytes[i] {
            b'"' => i = skip_string(bytes, i, b'"'),
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = skip_line(bytes, i),
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = skip_block(bytes, i),
            open @ (b'(' | b'[' | b'{') => {
                stack.push((open, i, stack.len()));
                i += 1;
            }
            close @ (b')' | b']' | b'}') => {
                if stack
                    .last()
                    .is_some_and(|(open, _, _)| mates(*open, close))
                {
                    let (_, open, depth) = stack.pop().expect("checked");
                    out.push(Pair {
                        open,
                        close: i,
                        depth,
                    });
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// The innermost pair the caret is sitting on, or the one it just passed.
pub fn around(pairs: &[Pair], offset: usize) -> Option<&Pair> {
    let on = pairs
        .iter()
        .filter(|pair| offset == pair.open || offset == pair.close)
        .max_by_key(|pair| pair.depth);
    if on.is_some() {
        return on;
    }
    pairs
        .iter()
        .filter(|pair| offset == pair.open + 1 || offset == pair.close + 1)
        .max_by_key(|pair| pair.depth)
}

fn mates(open: u8, close: u8) -> bool {
    matches!(
        (open, close),
        (b'(', b')') | (b'[', b']') | (b'{', b'}')
    )
}

fn skip_string(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == quote {
            return i + 1;
        }
        if bytes[i] == b'\n' {
            return i;
        }
        i += 1;
    }
    i
}

fn skip_line(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

fn skip_block(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_pairs_share_a_colour_with_their_own_closer() {
        let found = pairs("fn main() { let x = (1, [2]); }");
        let by_open: Vec<_> = found
            .iter()
            .map(|pair| (pair.open, pair.close, pair.depth))
            .collect();
        assert_eq!(
            by_open,
            vec![(7, 8, 0), (24, 26, 2), (20, 27, 1), (10, 30, 0)]
        );
    }

    #[test]
    fn strings_and_comments_do_not_take_a_colour() {
        let around_string = pairs(r#"foo("bar(baz)")"#);
        assert_eq!(around_string.len(), 1);
        assert_eq!(around_string[0].open, 3);
        assert!(pairs("foo // (\n").is_empty());
        assert!(pairs("foo /* { */ ").is_empty());
        assert_eq!(pairs("a(b) // (\n c[d]").len(), 2);
    }

    #[test]
    fn unmatched_brackets_are_ignored() {
        assert!(pairs("(]").is_empty());
        assert_eq!(pairs("(foo").len(), 0);
        assert_eq!(pairs("foo)").len(), 0);
        assert_eq!(pairs("(a[b)]").len(), 1);
        assert_eq!(
            pairs("(a[b)]")[0],
            Pair {
                open: 2,
                close: 5,
                depth: 1
            }
        );
    }

    #[test]
    fn the_caret_picks_the_pair_it_is_on_or_just_after() {
        let found = pairs("(a[b])");
        // ( a [ b ] )
        // 0 1 2 3 4 5
        assert_eq!(around(&found, 0).map(|p| p.open), Some(0));
        assert_eq!(around(&found, 1).map(|p| p.open), Some(0));
        assert_eq!(around(&found, 2).map(|p| p.open), Some(2));
        assert_eq!(around(&found, 5).map(|p| (p.open, p.close)), Some((0, 5)));
        assert_eq!(around(&found, 6).map(|p| (p.open, p.close)), Some((0, 5)));
        assert_eq!(around(&found, 4).map(|p| (p.open, p.close)), Some((2, 4)));
        assert!(around(&pairs("abc"), 1).is_none());
    }
}
