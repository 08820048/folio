//! A file's outline (functions, types, modules) from its Tree-sitter parse,
//! for Go to Symbol (`@` in quick open, `⇧⌘O` in the editor).
//!
//! There is no per-language outline query here. Grammars name their
//! definitions consistently enough (`function_item`, `class_declaration`,
//! `struct_specifier`, …) that one table of node kinds covers every grammar
//! Folio registers, and the name comes from the node's `name` field — or,
//! for C-like declarators, from the identifier at the end of the
//! `declarator` chain.

use std::collections::HashMap;

/// What a symbol is, for its marker in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Function,
    Type,
    Module,
}

impl Kind {
    /// One letter shown before the name.
    pub fn letter(self) -> &'static str {
        match self {
            Self::Function => "ƒ",
            Self::Type => "T",
            Self::Module => "M",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: Kind,
    /// Zero-based line of the definition.
    pub line: usize,
    /// How many enclosing symbols it has (a method in an impl is 1).
    pub depth: usize,
}

/// Node kinds that are definitions, across the registered grammars.
fn kind_of(node: &str) -> Option<Kind> {
    Some(match node {
        "function_item"
        | "function_signature_item"
        | "function_declaration"
        | "function_definition"
        | "method_definition"
        | "method_declaration"
        | "method_signature"
        | "constructor_declaration"
        | "generator_function_declaration"
        | "function_signature"
        | "abstract_method_signature"
        | "func_literal_declaration"
        | "singleton_method"
        | "method"
        | "function" => Kind::Function,
        "struct_item"
        | "enum_item"
        | "trait_item"
        | "union_item"
        | "type_item"
        | "impl_item"
        | "class_declaration"
        | "abstract_class_declaration"
        | "class_definition"
        | "class_specifier"
        | "struct_specifier"
        | "enum_specifier"
        | "union_specifier"
        | "interface_declaration"
        | "enum_declaration"
        | "struct_declaration"
        | "protocol_declaration"
        | "record_declaration"
        | "type_alias_declaration"
        | "type_spec"
        | "class"
        | "object_declaration"
        | "message"
        | "enum" => Kind::Type,
        "mod_item"
        | "module"
        | "namespace_definition"
        | "namespace_declaration"
        | "internal_module" => Kind::Module,
        _ => return None,
    })
}

/// The name of a definition node, or `None` when it has none worth showing
/// (an anonymous function, a forward declaration).
fn name_of(node: tree_sitter::Node, text: &str) -> Option<String> {
    let slice = |n: tree_sitter::Node| text.get(n.byte_range()).map(str::to_string);
    if node.kind() == "impl_item" {
        // `impl Trait for Type` reads as the type, then the trait.
        let ty = node.child_by_field_name("type").and_then(slice)?;
        return Some(match node.child_by_field_name("trait").and_then(slice) {
            Some(trait_name) => format!("impl {trait_name} for {ty}"),
            None => format!("impl {ty}"),
        });
    }
    if let Some(name) = node.child_by_field_name("name") {
        return slice(name);
    }
    // C, C++, Objective-C: `int main(void)` keeps the name at the end of a
    // chain of declarators (pointer, function, parenthesized…).
    let mut declarator = node.child_by_field_name("declarator")?;
    for _ in 0..8 {
        match declarator.child_by_field_name("declarator") {
            Some(inner) => declarator = inner,
            None => break,
        }
    }
    let name = slice(declarator)?;
    (!name.is_empty() && name.len() < 120).then_some(name)
}

/// The outline of `text` as `language` parses it, in source order.
pub fn outline(language: &tree_sitter::Language, text: &str) -> Vec<Symbol> {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    let mut symbols = Vec::new();
    // Depth is counted in symbols, not syntax nodes: node id → symbol depth.
    let mut depth_of: HashMap<usize, usize> = HashMap::new();
    let mut cursor = tree.walk();
    let mut stack: Vec<usize> = Vec::new();
    'walk: loop {
        let node = cursor.node();
        let parent_depth = stack.last().map(|id| depth_of[id]).unwrap_or(0);
        let mut depth = parent_depth;
        if let Some(kind) = kind_of(node.kind())
            && let Some(name) = name_of(node, text)
        {
            symbols.push(Symbol {
                name: name.split_whitespace().collect::<Vec<_>>().join(" "),
                kind,
                line: node.start_position().row,
                depth: parent_depth,
            });
            depth += 1;
        }
        depth_of.insert(node.id(), depth);
        if cursor.goto_first_child() {
            stack.push(node.id());
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                continue 'walk;
            }
            if !cursor.goto_parent() {
                break 'walk;
            }
            stack.pop();
        }
    }
    symbols
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::highlighter::LanguageRegistry;

    fn outline_of(language: &str, text: &str) -> Vec<(String, Kind, usize, usize)> {
        crate::syntax::register_extra_languages();
        let config = LanguageRegistry::singleton()
            .language(language)
            .unwrap_or_else(|| panic!("{language} is not registered"));
        outline(config.language.as_ref().expect("a grammar"), text)
            .into_iter()
            .map(|s| (s.name, s.kind, s.line, s.depth))
            .collect()
    }

    #[test]
    fn rust_items_nest_under_their_impl_and_module() {
        let text = "struct Point { x: i32 }\n\nimpl Display for Point {\n    fn fmt(&self) {}\n}\n\nmod tests {\n    fn adds() {}\n}\n";
        assert_eq!(
            outline_of("rust", text),
            vec![
                ("Point".into(), Kind::Type, 0, 0),
                ("impl Display for Point".into(), Kind::Type, 2, 0),
                ("fmt".into(), Kind::Function, 3, 1),
                ("tests".into(), Kind::Module, 6, 0),
                ("adds".into(), Kind::Function, 7, 1),
            ]
        );
    }

    #[test]
    fn python_typescript_and_c_find_their_definitions() {
        let python = outline_of(
            "python",
            "class Box:\n    def open(self):\n        pass\n\ndef main():\n    pass\n",
        );
        assert_eq!(
            python,
            vec![
                ("Box".into(), Kind::Type, 0, 0),
                ("open".into(), Kind::Function, 1, 1),
                ("main".into(), Kind::Function, 4, 0),
            ]
        );
        let ts = outline_of(
            "typescript",
            "interface Shape { area(): number }\nclass Circle {\n  area() { return 1 }\n}\nfunction draw() {}\n",
        );
        let names: Vec<_> = ts.iter().map(|s| s.0.as_str()).collect();
        assert!(
            names.contains(&"Shape") && names.contains(&"Circle"),
            "{names:?}"
        );
        assert!(
            names.contains(&"area") && names.contains(&"draw"),
            "{names:?}"
        );
        let c = outline_of(
            "c",
            "struct node { int v; };\nstatic int *find(int key) { return 0; }\nint main(void) { return 0; }\n",
        );
        let names: Vec<_> = c.iter().map(|s| s.0.as_str()).collect();
        assert_eq!(names, vec!["node", "find", "main"]);
    }
}
