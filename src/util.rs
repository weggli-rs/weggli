/*
Copyright 2021 Google LLC

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

     https://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/


// Try to parse a tree-sitter number literal into a constant value.
// This function assumes that tree-sitter already parsed the input string
// as a valid literal so we don't need to do much validation.
// The function should work for all integer literals, but will fail for
// floats. 
pub fn parse_number_literal(input: &str) -> Option<i128> {
    // remove suffixes and '
    let mut input: String = input
        .chars()
        .filter(|c| !['\'', 'u', 'U', 'l', 'L', 'z', 'Z'].contains(c))
        .collect();

    let negative = if input.starts_with('-'){
        input.remove(0);
        true
    } else {
        false
    };

    let (offset, radix) = match input.get(0..2) {
        Some("0x") | Some("0X") => (2, 16),
        Some("0b") | Some("0B") => (2, 2),
        Some(s) if s.starts_with('0') => (1, 8),
        None | Some(_) => (0, 10),
    };

    let value = i128::from_str_radix(&input[offset..], radix);

    if let Ok(v) = value {
        if negative {
            Some(-v)
        } else {
            Some(v)
        }
    } else {
        None
    }
}

#[test]
fn test_parse_number_literal() {
    assert_eq!(parse_number_literal("10"), Some(10));
    assert_eq!(parse_number_literal("0x10"), Some(0x10));
    assert_eq!(parse_number_literal("-0x10"), Some(-0x10));
    assert_eq!(parse_number_literal("0b11"), Some(3));
    assert_eq!(parse_number_literal("0"), Some(0));
    assert_eq!(parse_number_literal(""), None);
    assert_eq!(parse_number_literal("0xbeef"), Some(0xbeef));
    assert_eq!(parse_number_literal("0xbeef"), Some(0xbeef));
    assert_eq!(parse_number_literal("010"), Some(8));
    assert_eq!(parse_number_literal("abcdef"), None);
    assert_eq!(parse_number_literal("-0xbeef"), Some(-0xbeef));
    assert_eq!(parse_number_literal("0x1ull"), Some(1));
    assert_eq!(parse_number_literal("0x100ULL"), Some(0x100));
    assert_eq!(parse_number_literal("0x100z"), Some(0x100));
    assert_eq!(parse_number_literal("100'000"), Some(100000));
    assert_eq!(parse_number_literal("0.0"), None);
    assert_eq!(parse_number_literal("not-a-literal"), None);
    assert_eq!(parse_number_literal("-"), None);
}
