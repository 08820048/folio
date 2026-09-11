//! A small LSP client: enough of the protocol for hover and go-to-definition.
//!
//! One server per project per language, started the first time a file of that
//! language is opened and kept for as long as the project is. That is the whole
//! point of a language server: what it costs is indexing the project, and what
//! it gives back — the type of a symbol three crates away — comes from having
//! done it. Starting one per request would pay that cost every time.
//!
//! Requests are serialized under one lock, and the reply is read back under the
//! same one: a language server is a pipe, and two requests in flight on it
//! would have each other's answers. Nothing here is asynchronous — the callers
//! are background tasks, which is where a blocking read belongs.

use std::{
    collections::HashMap,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::Mutex,
};

use serde_json::{Value, json};

/// A language server this machine might have.
pub struct Server {
    pub command: &'static str,
    pub args: &'static [&'static str],
    /// What the protocol calls this language, which is not what the editor
    /// calls its grammar.
    pub language: &'static str,
}

/// The server for a grammar name, if one is installed.
pub fn server_for(language: &str) -> Option<Server> {
    let server = match language {
        "rust" => Server {
            command: "rust-analyzer",
            args: &[],
            language: "rust",
        },
        "typescript" | "tsx" => Server {
            command: "typescript-language-server",
            args: &["--stdio"],
            language: "typescript",
        },
        "javascript" | "jsx" => Server {
            command: "typescript-language-server",
            args: &["--stdio"],
            language: "javascript",
        },
        "python" => Server {
            command: "pyright-langserver",
            args: &["--stdio"],
            language: "python",
        },
        "go" => Server {
            command: "gopls",
            args: &[],
            language: "go",
        },
        "c" | "cpp" => Server {
            command: "clangd",
            args: &[],
            language: "cpp",
        },
        _ => return None,
    };
    installed(server.command).then_some(server)
}

/// Whether a command can be run. A language server is installed or it is not,
/// and there is no registry to ask — so the directories on `PATH` are looked
/// through, without starting anything to find out.
fn installed(command: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(command);
        candidate.is_file() && is_executable(&candidate)
    })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    true
}

/// A running language server.
pub struct Client {
    inner: Mutex<Inner>,
}

struct Inner {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    /// Per open document: its version, and where the server was last told the
    /// document ends — which is what makes it possible to describe the whole of
    /// it as one change.
    documents: HashMap<PathBuf, Document>,
    /// Whether the server asked for incremental changes. Both kinds are
    /// answered, by sending the whole document as a single change.
    incremental: bool,
    next_id: i64,
}

#[derive(Clone, Copy)]
struct Document {
    version: i64,
    /// Where the document ended when the server was last told about it.
    end: Position,
}

impl Client {
    /// Start the server for `root`, and introduce the client to it.
    pub fn start(server: &Server, root: &Path) -> io::Result<Client> {
        let mut child = Command::new(server.command)
            .args(server.args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("piped");
        let stdout = BufReader::new(child.stdout.take().expect("piped"));

        let client = Client {
            inner: Mutex::new(Inner {
                child,
                stdin,
                stdout,
                documents: HashMap::new(),
                incremental: false,
                next_id: 0,
            }),
        };

        let root_uri = uri(root);
        let result = client.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "workspaceFolders": [{"uri": root_uri, "name": "project"}],
                "capabilities": {
                    "textDocument": {
                        "hover": {"contentFormat": ["markdown", "plaintext"]},
                        "definition": {"linkSupport": true},
                        "synchronization": {"didSave": false},
                    },
                    "workspace": {"workspaceFolders": true},
                },
                "clientInfo": {"name": "Folio", "version": env!("CARGO_PKG_VERSION")},
            }),
        )?;
        client
            .inner
            .lock()
            .expect("not poisoned")
            .remember_sync_kind(&result);
        client.notify("initialized", json!({}))?;
        Ok(client)
    }

    /// Send a request and wait for its answer.
    pub fn request(&self, method: &str, params: Value) -> io::Result<Value> {
        let mut inner = self.inner.lock().expect("not poisoned");
        let id = inner.next_id;
        inner.next_id += 1;
        inner.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;

        // Anything that is not this answer is dealt with and passed over: the
        // server asking the client to do something, or a notification about
        // work it is doing.
        loop {
            let message = inner.read()?;
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(io::Error::other(error.to_string()));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            if message.get("method").is_some() && message.get("id").is_some() {
                // A request from the server, which this client does not
                // implement: answering "nothing" is better than leaving it
                // waiting.
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                inner.write(&json!({"jsonrpc": "2.0", "id": id, "result": Value::Null}))?;
            }
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> io::Result<()> {
        self.inner
            .lock()
            .expect("not poisoned")
            .write(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    /// Tell the server what a document holds, opening it the first time.
    pub fn sync(&self, path: &Path, text: &str, server_language: &str) -> io::Result<()> {
        let mut inner = self.inner.lock().expect("not poisoned");
        let next = Document {
            version: inner.documents.get(path).map_or(1, |last| last.version + 1),
            end: position_of(text, text.len()),
        };

        match inner.documents.get(path).copied() {
            None => inner.write(&json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {"textDocument": {
                    "uri": uri(path),
                    "languageId": server_language,
                    "version": next.version,
                    "text": text,
                }},
            }))?,
            Some(last) => {
                // The whole document is sent as one change. That is what a
                // server that asked for incremental changes expects as much as
                // one that did not: a change covering every line is a change,
                // and it saves this client from tracking edits — which is what
                // the editor already does, for its own reasons.
                let change = if inner.incremental {
                    json!({
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": last.end.line, "character": last.end.character},
                        },
                        "text": text,
                    })
                } else {
                    json!({"text": text})
                };
                inner.write(&json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/didChange",
                    "params": {
                        "textDocument": {"uri": uri(path), "version": next.version},
                        "contentChanges": [change],
                    },
                }))?;
            }
        }

        inner.documents.insert(path.to_path_buf(), next);
        Ok(())
    }
}

impl Inner {
    fn remember_sync_kind(&mut self, initialize_result: &Value) {
        let sync = &initialize_result["capabilities"]["textDocumentSync"];
        // Either a number or an object with a `change` in it, and either may be
        // missing.
        let change = match sync {
            Value::Number(number) => number.as_i64(),
            Value::Object(_) => sync.get("change").and_then(Value::as_i64),
            _ => None,
        };
        self.incremental = change == Some(2);
    }

    fn write(&mut self, message: &Value) -> io::Result<()> {
        let body = message.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
        self.stdin.flush()
    }

    fn read(&mut self) -> io::Result<Value> {
        let mut length = None;
        loop {
            let mut header = String::new();
            if self.stdout.read_line(&mut header)? == 0 {
                return Err(io::Error::other("The language server stopped"));
            }
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some(value) = header.strip_prefix("Content-Length:") {
                length = value.trim().parse::<usize>().ok();
            }
        }

        let length = length.ok_or_else(|| io::Error::other("A message with no length"))?;
        let mut body = vec![0; length];
        self.stdout.read_exact(&mut body)?;
        serde_json::from_slice(&body).map_err(io::Error::other)
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // A server left running is a process the user cannot see and did not
        // ask for.
        let _ = self.child.kill();
    }
}

/// A position in a document: a line, and a character counted the way the
/// protocol counts them — in UTF-16 code units, not bytes and not characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub character: usize,
}

/// Where `offset` — a byte offset into `text` — is, as the protocol counts.
pub fn position_of(text: &str, offset: usize) -> Position {
    let offset = offset.min(text.len());
    let mut line = 0;
    let mut line_start = 0;
    for (at, byte) in text.bytes().enumerate() {
        if at >= offset {
            break;
        }
        if byte == b'\n' {
            line += 1;
            line_start = at + 1;
        }
    }
    let character = text
        .get(line_start..offset)
        .map_or(0, |slice| slice.encode_utf16().count());
    Position { line, character }
}

/// The byte offset of a position. Clamped to the text: a server can name a
/// position that does not exist, since it is describing a file that may have
/// changed since.
pub fn offset_of(text: &str, position: Position) -> usize {
    let mut line = 0;
    let mut line_start = 0;
    for (at, byte) in text.bytes().enumerate() {
        if line == position.line {
            break;
        }
        if byte == b'\n' {
            line += 1;
            line_start = at + 1;
        }
    }
    if line != position.line {
        return text.len();
    }

    let mut units = 0;
    for (at, character) in text[line_start..].char_indices() {
        if character == '\n' || units >= position.character {
            return line_start + at;
        }
        units += character.len_utf16();
    }
    text.len()
}

/// A definition response as links, whichever of the three shapes the protocol
/// allows it to arrive in: one location, a list of them, or links.
///
/// A location is turned into a link, with the whole of its range standing in
/// for the part of it the target should be selected at — which is what a server
/// that has no opinion about the selection is saying.
pub fn definition_links(result: &Value) -> Vec<lsp_types::LocationLink> {
    let entries = match result {
        Value::Array(entries) => entries.clone(),
        Value::Null => return Vec::new(),
        entry => vec![entry.clone()],
    };

    entries
        .iter()
        .filter_map(|entry| {
            let uri = entry
                .get("targetUri")
                .or_else(|| entry.get("uri"))
                .and_then(Value::as_str)?;
            let range = entry
                .get("targetSelectionRange")
                .or_else(|| entry.get("targetRange"))
                .or_else(|| entry.get("range"))?;
            let range = read_range(range)?;
            let origin = entry
                .get("targetRange")
                .or_else(|| entry.get("range"))
                .and_then(read_range)
                .unwrap_or(range);
            Some(lsp_types::LocationLink {
                origin_selection_range: read_range_opt(entry.get("originSelectionRange")),
                target_uri: uri.parse().ok()?,
                target_range: origin,
                target_selection_range: range,
            })
        })
        .collect()
}

/// Where a link points, as a path and the position to put the caret at.
pub fn target(link: &lsp_types::LocationLink) -> Option<(PathBuf, lsp_types::Position)> {
    let path = path_of(link.target_uri.as_str())?;
    let start = link.target_selection_range.start;
    Some((path, start))
}

fn read_range(value: &Value) -> Option<lsp_types::Range> {
    Some(lsp_types::Range {
        start: read_lsp_position(value.get("start")?)?,
        end: read_lsp_position(value.get("end")?)?,
    })
}

fn read_range_opt(value: Option<&Value>) -> Option<lsp_types::Range> {
    read_range(value?)
}

fn read_lsp_position(value: &Value) -> Option<lsp_types::Position> {
    Some(lsp_types::Position {
        line: value.get("line")?.as_u64()? as u32,
        character: value.get("character")?.as_u64()? as u32,
    })
}

/// The text of a hover, as lines.
///
/// A server answers in markdown, usually one fenced block with the type in it,
/// and the fences are taken off rather than rendered: rendering markdown is a
/// feature of its own, and a hover that shows the type without its backticks is
/// most of the value.
pub fn hover_lines(result: &Value) -> Vec<String> {
    let contents = result.get("contents").unwrap_or(result);
    let mut text = String::new();
    match contents {
        Value::String(value) => text.push_str(value),
        Value::Object(object) => {
            if let Some(Value::String(value)) = object.get("value") {
                text.push_str(value);
            }
        }
        Value::Array(parts) => {
            for part in parts {
                match part {
                    Value::String(value) => text.push_str(value),
                    Value::Object(object) => {
                        if let Some(Value::String(value)) = object.get("value") {
                            text.push_str(value);
                        }
                    }
                    _ => {}
                }
                text.push('\n');
            }
        }
        _ => {}
    }

    let mut lines: Vec<String> = text
        .lines()
        .map(|line| line.trim_end())
        .filter(|line| !line.trim_start().starts_with("```"))
        .map(str::to_string)
        .collect();
    // Blank lines between paragraphs are the server's, but a blank line at the
    // end is the string it wrote them in.
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines
}

/// The same position counted in characters rather than in UTF-16 code units,
/// which is what the editor's own jumps are counted in. The two differ from the
/// first character outside the basic plane on a line.
pub fn char_position(text: &str, position: Position) -> Position {
    let offset = offset_of(text, position);
    let line_start = text[..offset].rfind('\n').map_or(0, |at| at + 1);
    Position {
        line: position.line,
        character: text[line_start..offset].chars().count(),
    }
}

/// A `file://` URI for a path, with the characters that are not allowed in one
/// percent-encoded.
pub fn uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'/' | b'-' | b'_' | b'.' | b'~' => uri.push(byte as char),
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => uri.push(byte as char),
            byte => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

/// The path a `file://` URI names.
pub fn path_of(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut bytes = Vec::with_capacity(rest.len());
    let mut parts = rest.split('%');
    if let Some(first) = parts.next() {
        bytes.extend_from_slice(first.as_bytes());
    }
    for part in parts {
        let (hex, rest) = part.split_at(part.len().min(2));
        bytes.push(u8::from_str_radix(hex, 16).ok()?);
        bytes.extend_from_slice(rest.as_bytes());
    }
    Some(PathBuf::from(String::from_utf8(bytes).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_are_counted_the_way_the_protocol_counts_them() {
        let text = "let x = 1;\nlet 中文 = 2;\nlet 🦀 = 3;\n";

        // A byte offset on the first line.
        assert_eq!(
            position_of(text, 4),
            Position {
                line: 0,
                character: 4
            }
        );
        // On the second line: three ASCII characters in, which is three UTF-16
        // units — and six bytes.
        let second = text.find("中文").unwrap();
        assert_eq!(
            position_of(text, second),
            Position {
                line: 1,
                character: 4
            }
        );
        // A crab is four bytes and two UTF-16 units, so a position after one is
        // further along than the bytes suggest.
        let crab = text.find('🦀').unwrap();
        assert_eq!(
            position_of(text, crab + '🦀'.len_utf8()),
            Position {
                line: 2,
                character: 6
            }
        );
    }

    #[test]
    fn offsets_are_the_other_way_round() {
        let text = "let x = 1;\nlet 中文 = 2;\nlet 🦀 = 3;\n";
        for offset in [0, 4, 11, 15, 22, 30, 40] {
            let position = position_of(text, offset);
            assert_eq!(offset_of(text, position), offset, "at byte {offset}");
        }
        // A position past the end of a line stops at the line's end; one past
        // the end of the file stops at the file's.
        assert_eq!(
            offset_of(
                text,
                Position {
                    line: 0,
                    character: 99
                }
            ),
            10
        );
        assert_eq!(
            offset_of(
                text,
                Position {
                    line: 99,
                    character: 0
                }
            ),
            text.len()
        );
    }

    #[test]
    fn uris_survive_the_characters_paths_have() {
        let path = Path::new("/Users/x/项目 name/main.rs");
        let uri = uri(path);
        assert!(uri.starts_with("file:///Users/x/"));
        assert_eq!(path_of(&uri).as_deref(), Some(path));
        assert_eq!(path_of("https://example.com"), None);
    }

    #[test]
    fn the_server_for_a_language_is_the_one_that_speaks_it() {
        // The mapping does not depend on what is installed, which is the part
        // this can check without one.
        for language in ["rust", "typescript", "python", "go", "c", "cpp"] {
            if let Some(server) = server_for(language) {
                assert!(!server.command.is_empty());
                assert!(!server.language.is_empty());
            }
        }
        assert!(server_for("markdown").is_none());
        assert!(server_for("text").is_none());
    }

    #[test]
    fn a_position_counted_in_characters_is_not_one_counted_in_utf16() {
        // "let 🦀 = 3;": the crab is two UTF-16 units and one character.
        let text = "let 🦀 = 3;\n";
        let after = position_of(text, text.find(" = 3").unwrap());
        assert_eq!(after.character, 6);
        assert_eq!(char_position(text, after).character, 5);
        // Everything inside the basic plane counts the same either way.
        let ascii = "let x = 1;\n";
        let position = position_of(ascii, 6);
        assert_eq!(char_position(ascii, position), position);
    }

    #[test]
    fn a_definition_is_read_in_every_shape_the_protocol_allows() {
        let link = json!({
            "targetUri": "file:///a/b.rs",
            "targetRange": {"start": {"line": 1, "character": 0}, "end": {"line": 3, "character": 1}},
            "targetSelectionRange": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 9}},
        });
        // One link, a list of them, and a plain location.
        let one = definition_links(&link);
        assert_eq!(one.len(), 1);
        assert_eq!(
            target(&one[0]).map(|(path, _)| path),
            Some(PathBuf::from("/a/b.rs"))
        );
        assert_eq!(
            target(&one[0]).map(|(_, position)| position.character),
            Some(4)
        );
        assert_eq!(definition_links(&json!([link])).len(), 1);

        let location = definition_links(&json!({
            "uri": "file:///a/%E4%B8%AD.rs",
            "range": {"start": {"line": 7, "character": 2}, "end": {"line": 7, "character": 5}},
        }));
        assert_eq!(location.len(), 1);
        assert_eq!(
            target(&location[0]).map(|(path, _)| path),
            Some(PathBuf::from("/a/中.rs"))
        );
        // A location has no selection of its own, so the whole range is it.
        assert_eq!(
            location[0].target_selection_range.start,
            location[0].target_range.start
        );

        assert!(definition_links(&Value::Null).is_empty());
        assert!(definition_links(&json!([])).is_empty());
    }

    #[test]
    fn a_hover_is_read_without_its_fences() {
        let markup = json!({
            "contents": {"kind": "markdown", "value": "```rust\nfn main() -> ()\n```\n\n---\n"}
        });
        assert_eq!(
            hover_lines(&markup),
            vec![
                "fn main() -> ()".to_string(),
                "".to_string(),
                "---".to_string()
            ]
        );

        // Plain strings, and a list of them, which is the older shape.
        assert_eq!(
            hover_lines(&json!({"contents": "let x: u32"})),
            vec!["let x: u32"]
        );
        assert_eq!(
            hover_lines(&json!({"contents": [{"value": "one"}, "two"]})),
            vec!["one".to_string(), "two".to_string()]
        );
        assert!(hover_lines(&json!({"contents": "  "})).is_empty());
    }
}
