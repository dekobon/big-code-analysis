//! Direct-AST source documentation workloads; parsing is outside timed regions.

use std::hint::black_box;

use big_code_analysis_ast::documentation::{DocumentationContext, DocumentationOptions};
use big_code_analysis_ast::{AnyParser, LANG};
use criterion::{BenchmarkId, Criterion};

fn main() {
    let mut criterion = Criterion::default().configure_from_args();
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    for size in [256, 512, 1024] {
        cold(&mut criterion, size, &context, options);
        secondary(&mut criterion, size, &context, options);
        prepared(&mut criterion, size, &context, options);
        late_owner(&mut criterion, size, &context, options);
        included_ranges(&mut criterion, size, &context, options);
    }
    criterion.final_summary();
}

fn included_ranges(
    criterion: &mut Criterion,
    size: usize,
    context: &DocumentationContext,
    options: DocumentationOptions,
) {
    use big_code_analysis_ast::tree_sitter;
    let source = "///audit\nfn f() {}\n".repeat(size).into_bytes();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(
            &LANG::Rust
                .tree_sitter_language()
                .unwrap_or_else(|error| panic!("grammar: {error}")),
        )
        .unwrap_or_else(|error| panic!("grammar: {error}"));
    let ranges: Vec<_> = (0..size)
        .map(|at| tree_sitter::Range {
            start_byte: at * 18,
            end_byte: (at + 1) * 18,
            start_point: tree_sitter::Point::new(at * 2, 0),
            end_point: tree_sitter::Point::new((at + 1) * 2, 0),
        })
        .collect();
    parser
        .set_included_ranges(&ranges)
        .unwrap_or_else(|error| panic!("ranges: {error}"));
    let tree = parser
        .parse(&source, None)
        .unwrap_or_else(|| panic!("complete parse"));
    let mut group = criterion.benchmark_group("documentation/ranges");
    group.bench_function(BenchmarkId::new("cold", size), |bencher| {
        bencher.iter(|| {
            black_box(
                big_code_analysis_ast::documentation::collect(
                    LANG::Rust,
                    &tree,
                    &source,
                    context,
                    &options,
                )
                .unwrap_or_else(|error| panic!("collect: {error:?}")),
            )
        });
    });
    group.finish();
}

fn cold(
    criterion: &mut Criterion,
    size: usize,
    context: &DocumentationContext,
    options: DocumentationOptions,
) {
    let workloads = [
        (
            "depth",
            LANG::Rust,
            format!(
                "{}///audit\nfn leaf() {{}}{}",
                "mod m {".repeat(size),
                "}".repeat(size)
            ),
        ),
        ("width", LANG::Rust, "///audit\nfn f() {}\n".repeat(size)),
        (
            "fragments",
            LANG::Go,
            format!("package p\n{}func F() {{}}", "// audit\n".repeat(size)),
        ),
        (
            "single-line-lua",
            LANG::Lua,
            format!(
                "module(\"m\")\n---{}\nfunction f() end",
                "audit ".repeat(size)
            ),
        ),
        (
            "single-line-jsdoc",
            LANG::Javascript,
            format!("/**{}*/\nfunction f() {{}}", "audit ".repeat(size)),
        ),
        (
            "c-slots",
            LANG::C,
            format!(
                "/**audit*/\nint {};",
                (0..size)
                    .map(|at| format!("v{at}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ),
        (
            "wide-error",
            LANG::C,
            format!("/**audit*/\nstruct {{\n{}", "int x;\n".repeat(size)),
        ),
        (
            "missing",
            LANG::Rust,
            format!(
                "fn f() {{ {}0{}; }}",
                "(".repeat(size),
                ")".repeat(size - 1)
            ),
        ),
        ("names", LANG::Cpp, names_source(size)),
    ];
    let mut group = criterion.benchmark_group("documentation/cold");
    for (name, language, source) in workloads {
        let parser = AnyParser::parse(language, source.into_bytes(), None, None)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        group.bench_with_input(BenchmarkId::new(name, size), &parser, |bencher, parser| {
            bencher.iter(|| {
                black_box(
                    parser
                        .documentation(context, &options)
                        .unwrap_or_else(|error| panic!("collection: {error:?}")),
                )
            });
        });
    }
    group.finish();
}

fn secondary(
    criterion: &mut Criterion,
    size: usize,
    context: &DocumentationContext,
    options: DocumentationOptions,
) {
    let slots = (0..size)
        .map(|at| format!("v{at}"))
        .collect::<Vec<_>>()
        .join(",");
    let workloads = [
        (
            "single-line-php",
            LANG::Php,
            format!("<?php\n/** {}*/\nfunction f() {{}}", "audit ".repeat(size)),
        ),
        (
            "single-line-ruby",
            LANG::Ruby,
            format!("# {}\ndef f\nend", "audit ".repeat(size)),
        ),
        (
            "single-line-doxygen",
            LANG::Cpp,
            format!("/**{}*/\nvoid f() {{}}", "audit ".repeat(size)),
        ),
        (
            "text-fixed-fanout",
            LANG::C,
            format!("/**{}*/\nint a,b;", "audit ".repeat(size)),
        ),
        (
            "fanout-fixed-text",
            LANG::C,
            format!("/**{}*/\nint {slots};", "audit ".repeat(1024)),
        ),
        (
            "hidden-fields",
            LANG::C,
            format!("/**audit*/\nint a{};", "[1]".repeat(size)),
        ),
        (
            "ruby-modifiers",
            LANG::Ruby,
            format!("# audit\ndef f; end{}", " if true".repeat(size)),
        ),
        (
            "java-undocumented",
            LANG::Java,
            format!(
                "{}void f() {{}}{}",
                "class C {".repeat(size),
                "}".repeat(size)
            ),
        ),
        (
            "pending-scope",
            LANG::Rust,
            format!("#![doc(hidden)]\n{}", "fn f() {}\n".repeat(size)),
        ),
    ];
    let mut group = criterion.benchmark_group("documentation/secondary");
    for (name, language, source) in workloads {
        let parser = AnyParser::parse(language, source.into_bytes(), None, None)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        group.bench_with_input(BenchmarkId::new(name, size), &parser, |bencher, parser| {
            bencher.iter(|| {
                black_box(
                    parser
                        .documentation(context, &options)
                        .unwrap_or_else(|error| panic!("{name}: {error:?}")),
                )
            });
        });
    }
    group.finish();
}

fn late_owner(
    criterion: &mut Criterion,
    size: usize,
    context: &DocumentationContext,
    options: DocumentationOptions,
) {
    let parser = AnyParser::parse(
        LANG::Rust,
        "///audit\nfn f() {}\n".repeat(size).into_bytes(),
        None,
        None,
    )
    .unwrap_or_else(|error| panic!("parse: {error}"));
    let index = parser
        .documentation(context, &options)
        .unwrap_or_else(|error| panic!("collect: {error:?}"));
    let node = parser
        .root_node()
        .children()
        .last()
        .unwrap_or_else(|| panic!("last function"));
    let mut group = criterion.benchmark_group("documentation/prepared");
    group.bench_function(BenchmarkId::new("late-owner", size), |bencher| {
        bencher.iter(|| {
            black_box(
                index
                    .node_associations(node.as_tree_sitter())
                    .unwrap_or_else(|error| panic!("owner: {error:?}"))
                    .associations()
                    .count(),
            )
        });
    });
    group.finish();
}

fn names_source(size: usize) -> String {
    use std::fmt::Write;
    let mut source = String::new();
    for at in 0..size {
        let _ = writeln!(source, "/** @fn void f{at}() */\nvoid f{at}() {{}}");
    }
    source
}

fn prepared(
    criterion: &mut Criterion,
    size: usize,
    context: &DocumentationContext,
    options: DocumentationOptions,
) {
    let parser = AnyParser::parse(
        LANG::Rust,
        "///a\n"
            .repeat(size)
            .into_bytes()
            .into_iter()
            .chain(b"fn f() {}".iter().copied())
            .collect(),
        None,
        None,
    )
    .unwrap_or_else(|error| panic!("parse: {error}"));
    let index = parser
        .documentation(context, &options)
        .unwrap_or_else(|error| panic!("collection: {error:?}"));
    let document = index
        .documents()
        .next()
        .unwrap_or_else(|| panic!("fixture has a document"));
    let text = index
        .complete_text(document)
        .unwrap_or_else(|error| panic!("text: {error:?}"))
        .unwrap_or_else(|| panic!("complete text"));
    let owner = parser
        .root_node()
        .children()
        .last()
        .unwrap_or_else(|| panic!("fixture has a function"));
    let mut group = criterion.benchmark_group("documentation/prepared");
    group.bench_function(BenchmarkId::new("owner", size), |bencher| {
        bencher.iter(|| {
            black_box(
                index
                    .node_associations(owner.as_tree_sitter())
                    .unwrap_or_else(|error| panic!("owner: {error:?}"))
                    .associations()
                    .count(),
            )
        });
    });
    group.bench_function(BenchmarkId::new("late-map", size), |bencher| {
        bencher.iter(|| {
            black_box(
                text.origins(text.text().len() - 1..text.text().len())
                    .unwrap_or_else(|error| panic!("map: {error:?}"))
                    .count(),
            )
        });
    });
    group.bench_function(BenchmarkId::new("retained", size), |bencher| {
        bencher.iter(|| {
            black_box(
                index
                    .complete_text(document)
                    .unwrap_or_else(|error| panic!("text: {error:?}")),
            )
        });
    });
    group.finish();
}
