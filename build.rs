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

extern crate cc;

fn main() {
    cc::Build::new()
        .include("third_party/grammars/")
        .file("third_party/grammars/weggli-c/src/parser.c")
        .flag("-std=c99")
        .flag("-w")
        .compile("tree-sitter-c");

    cc::Build::new()
        .cpp(true)
        .include("third_party/grammars/")
        .file("third_party/grammars/weggli-cpp/src/scanner.cc")
        // keep all symbols until linking (https://doc.rust-lang.org/rustc/command-line-arguments.html#linking-modifiers-whole-archive)
        .link_lib_modifier("+whole-archive")
        // don't bundle into rlib, but link to the object files instead (+bundle is incompatible with +whole-archive) (https://doc.rust-lang.org/rustc/command-line-arguments.html#linking-modifiers-bundle)
        .link_lib_modifier("-bundle")
        .flag("-w")
        .compile("tree-sitter-cpp-scanner");

    cc::Build::new()
        .include("third_party/grammars/")
        .file("third_party/grammars/weggli-cpp/src/parser.c")
        .flag("-w")
        .compile("tree-sitter-cpp-parser");
}
