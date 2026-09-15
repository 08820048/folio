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
    pairs_bytes(text.as_bytes())
}

/// Same as [`pairs`], from UTF-8 bytes. Folio uses this so a Rope can be
/// scanned from a single chunk without allocating a `String`.
pub fn pairs_bytes(bytes: &[u8]) -> Vec<Pair> {
    let mut i = 0;
    let mut stack: Vec<(u8, usize, usize)> = Vec::new();
    let mut out = Vec::new();
    // Nested template literals. While this is non-empty and `interp` is
    // empty we are skipping `...`; `${` pushes an interpolation depth.
    let mut templates: usize = 0;
    let mut interp: Vec<usize> = Vec::new();
    while i < bytes.len() {
        if templates > 0 && interp.is_empty() {
            i = skip_template_body(bytes, i, &mut templates, &mut interp);
            continue;
        }
        let b = bytes[i];
        let prev = if i == 0 { b'\0' } else { bytes[i - 1] };
        match b {
            b'"' if triple(bytes, i, b'"') => i = skip_triple(bytes, i, b'"'),
            b'\'' if triple(bytes, i, b'\'') => i = skip_triple(bytes, i, b'\''),
            b'"' => i = skip_string(bytes, i, b'"', 0),
            b'\'' => i = skip_single(bytes, i),
            b'`' => {
                templates += 1;
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = skip_line(bytes, i),
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = skip_block(bytes, i),
            b'r' | b'b' | b'c' | b'f'
                if !is_ident(prev)
                    && let Some(end) = raw_string_start(bytes, i) =>
            {
                i = skip_string(bytes, end.0, b'"', end.1);
            }
            open @ (b'(' | b'[' | b'{') => {
                if open == b'{' {
                    if let Some(depth) = interp.last_mut() {
                        *depth += 1;
                    }
                }
                stack.push((open, i, stack.len()));
                i += 1;
            }
            close @ (b')' | b']' | b'}') => {
                if close == b'}' {
                    if let Some(depth) = interp.last_mut() {
                        if *depth == 1 {
                            interp.pop();
                            i += 1;
                            continue;
                        }
                        *depth -= 1;
                    }
                }
                if stack.last().is_some_and(|(open, _, _)| mates(*open, close)) {
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

/// Same rule as [`around`], for `(open, close)` pairs the editor stores.
pub fn around_offsets(pairs: &[(usize, usize)], offset: usize) -> Option<(usize, usize)> {
    let span = |pair: &&(usize, usize)| pair.1.saturating_sub(pair.0);
    pairs
        .iter()
        .filter(|(open, close)| offset == *open || offset == *close)
        .min_by_key(span)
        .or_else(|| {
            pairs
                .iter()
                .filter(|(open, close)| offset == *open + 1 || offset == *close + 1)
                .min_by_key(span)
        })
        .copied()
}

fn mates(open: u8, close: u8) -> bool {
    matches!((open, close), (b'(', b')') | (b'[', b']') | (b'{', b'}'))
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn triple(bytes: &[u8], i: usize, quote: u8) -> bool {
    bytes.get(i + 1) == Some(&quote) && bytes.get(i + 2) == Some(&quote)
}

fn skip_triple(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 3;
    while i + 2 < bytes.len() {
        if bytes[i] == quote && bytes[i + 1] == quote && bytes[i + 2] == quote {
            return i + 3;
        }
        i += 1;
    }
    bytes.len()
}

fn skip_string(bytes: &[u8], start: usize, quote: u8, hashes: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if hashes == 0 && bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == quote && trailing_hashes(bytes, i + 1, hashes) {
            return i + 1 + hashes;
        }
        if hashes == 0 && bytes[i] == b'\n' {
            return i;
        }
        i += 1;
    }
    i
}

fn trailing_hashes(bytes: &[u8], start: usize, hashes: usize) -> bool {
    bytes
        .get(start..start + hashes)
        .is_some_and(|slice| slice.iter().all(|b| *b == b'#'))
}

/// `'...'` on one line is a string; `'static` and `'a` are not.
fn skip_single(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() && bytes[i] != b'\n' {
        if bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == b'\'' {
            return i + 1;
        }
        i += 1;
    }
    start + 1
}

/// Skip the body of a template literal until `${` or the closing backtick.
fn skip_template_body(
    bytes: &[u8],
    mut i: usize,
    templates: &mut usize,
    interp: &mut Vec<usize>,
) -> usize {
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == b'`' {
            *templates -= 1;
            return i + 1;
        }
        if bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'{') {
            interp.push(1);
            return i + 2;
        }
        i += 1;
    }
    i
}

fn raw_string_start(bytes: &[u8], i: usize) -> Option<(usize, usize)> {
    // r" r#" br" cr" fr" b#" — prefix then optional hashes then quote.
    let mut k = i;
    if matches!(bytes[k], b'b' | b'c' | b'f') {
        k += 1;
        if bytes.get(k) != Some(&b'r') {
            // b" is a byte string, not raw.
            if bytes.get(k) == Some(&b'"') {
                return None;
            }
            return None;
        }
        k += 1;
    } else if bytes[k] == b'r' {
        k += 1;
    } else {
        return None;
    }
    let mut hashes = 0;
    while bytes.get(k) == Some(&b'#') {
        hashes += 1;
        k += 1;
    }
    if bytes.get(k) == Some(&b'"') {
        Some((k, hashes))
    } else {
        None
    }
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
        assert_eq!(pairs("x('a(b)')").len(), 1);
        assert_eq!(pairs("'static (x)").len(), 1);
        assert_eq!(pairs("\"\"\" (\n ) \"\"\"").len(), 0);
        assert_eq!(pairs("r#\" ( ) \"#").len(), 0);
        assert_eq!(pairs("r\"(\"").len(), 0);
    }

    #[test]
    fn template_interpolations_are_scanned() {
        let found = pairs("a(`${foo(bar)}`)");
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|p| p.open < 8 && p.close > 8)); // foo(...)
        assert!(found.iter().any(|p| p.open == 1)); // a(...)
        assert_eq!(pairs("`${ {a: 1} }`").len(), 1);
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
        assert_eq!(around(&found, 0).map(|p| p.open), Some(0));
        assert_eq!(around(&found, 1).map(|p| p.open), Some(0));
        assert_eq!(around(&found, 2).map(|p| p.open), Some(2));
        assert_eq!(around(&found, 5).map(|p| (p.open, p.close)), Some((0, 5)));
        assert_eq!(around(&found, 6).map(|p| (p.open, p.close)), Some((0, 5)));
        assert_eq!(around(&found, 4).map(|p| (p.open, p.close)), Some((2, 4)));
        assert!(around(&pairs("abc"), 1).is_none());
        let offsets: Vec<(usize, usize)> = found.iter().map(|p| (p.open, p.close)).collect();
        assert_eq!(around_offsets(&offsets, 5), Some((0, 5)));
        assert_eq!(around_offsets(&offsets, 4), Some((2, 4)));
        assert_eq!(around_offsets(&offsets, 1), Some((0, 5)));
    }
}
