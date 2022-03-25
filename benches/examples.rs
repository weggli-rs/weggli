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

use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};

fn run_query(s: &(tree_sitter::Tree, String), pattern: &str) {
    let tree = weggli::parse(pattern, false);

    let mut c = tree.walk();
    c.goto_first_child();
    let qt = weggli::builder::build_query_tree(pattern, &mut c, false, None);

    let matches = qt.matches(s.0.root_node(), &s.1);
    for m in matches {
        m.display(&s.1, 500, 500);
    }
}

fn read_file(path: &str) -> String {
    let c = std::fs::read(path).unwrap();
    std::str::from_utf8(&c).unwrap().to_string()
}

fn bench(c: &mut Criterion) {
    let p = |path| {
        let source = read_file(path);
        (weggli::parse(&source, false), source)
    };

    let cluster = p("./third_party/examples/cluster.c");
    c.bench_function("cluster.c ", |b| {
        b.iter(|| run_query(&cluster, "{zmalloc($x);}"))
    });

    c.bench_function("cluster.c - 2", |b| {
        b.iter(|| run_query(&cluster, "{$t $x; $y = _($x); return $y;"))
    });

    let malloc = p("./third_party/examples/malloc.c");
    c.bench_function("malloc.c", |b| {
        b.iter(|| run_query(&malloc, "{$t $x; $x=_+_;}"))
    });

    let parser = p("./third_party/examples/parser.c");
    c.bench_function("parser.c", |b| {
        b.iter(|| run_query(&parser, "{$func($x); $func2($x);"))
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().significance_level(0.05).measurement_time(Duration::from_millis(20000)).sample_size(100);
    targets = bench
}
criterion_main!(benches);
