#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }

    let is_cpp = data[0] == 0;

    if let Ok(query) = std::str::from_utf8(&data[1..]) {
        let tree = weggli::parse(query, is_cpp);
        if tree.root_node().has_error() {
            return;
        }

        let c = tree.root_node().child(0);
        if let Some(n) = c {
            if !VALID_NODE_KINDS.contains(&n.kind()) {
                return;
            }
        } else {
            return;
        }

        let mut cursor = c.unwrap().walk();

        let _ = weggli::builder::build_query_tree(query, &mut cursor, is_cpp, None);
    }
});

/// Supported root node types.
const VALID_NODE_KINDS: &[&str] = &[
    "compound_statement",
    "function_definition",
    "struct_specifier",
    "enum_specifier",
    "union_specifier",
    "class_specifier",
];
