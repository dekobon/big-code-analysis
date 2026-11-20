//! Direct-AST source documentation workloads; parsing is outside timed regions.

use std::hint::black_box;

use big_code_analysis_ast::documentation::{DocumentationContext, DocumentationOptions};
use big_code_analysis_ast::{AnyParser, LANG};
use criterion::{BenchmarkId, Criterion};

#[path = "documentation/validation.rs"]
mod validation;

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
    for size in [1024, 2048, 4096] {
        remediation(&mut criterion, size, &context, options);
        multiline_ranges(&mut criterion, size, &context, options);
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
            start_byte: at * 19,
            end_byte: (at + 1) * 19,
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
    assert_eq!(tree.included_ranges().len(), size);
    let checked = big_code_analysis_ast::documentation::collect(
        LANG::Rust,
        &tree,
        &source,
        context,
        &options,
    )
    .unwrap_or_else(|error| panic!("collect: {error:?}"));
    validation::documents(&checked, size, 1, 1, "audit");
    drop(checked);
    let mut group = criterion.benchmark_group("documentation/ranges");
    group.bench_function(BenchmarkId::new("cold", size), |bencher| {
        bencher.iter(|| {
            black_box(
                big_code_analysis_ast::documentation::collect(
                    LANG::Rust,
                    black_box(&tree),
                    black_box(&source),
                    black_box(context),
                    black_box(&options),
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
        validation::fixture(name, size, &parser, context, options);
        group.bench_with_input(BenchmarkId::new(name, size), &parser, |bencher, parser| {
            bencher.iter(|| {
                black_box(
                    black_box(parser)
                        .documentation(black_box(context), black_box(&options))
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
            "visible-declarator-depth",
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
        validation::fixture(name, size, &parser, context, options);
        group.bench_with_input(BenchmarkId::new(name, size), &parser, |bencher, parser| {
            bencher.iter(|| {
                black_box(
                    black_box(parser)
                        .documentation(black_box(context), black_box(&options))
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
    validation::documents(&index, size, 1, 1, "audit");
    assert_eq!(node.kind(), "function_item");
    assert_eq!(node.start_byte(), (size - 1) * 19 + 9);
    assert_eq!(
        index
            .node_associations(node.as_tree_sitter())
            .unwrap_or_else(|error| panic!("owner: {error:?}"))
            .associations()
            .len(),
        1
    );
    let mut group = criterion.benchmark_group("documentation/prepared");
    group.bench_function(BenchmarkId::new("late-owner", size), |bencher| {
        bencher.iter(|| {
            black_box(
                black_box(&index)
                    .node_associations(black_box(node.as_tree_sitter()))
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
    validation::documents(
        &index,
        1,
        size,
        1,
        &std::iter::repeat_n("a", size)
            .collect::<Vec<_>>()
            .join("\n"),
    );
    assert_eq!(owner.kind(), "function_item");
    assert_eq!(owner.start_byte(), 5 * size);
    assert_eq!(text.mappings().len(), size * 2 - 1);
    let portions: Vec<_> = text
        .origins(size * 2 - 2..size * 2 - 1)
        .unwrap_or_else(|error| panic!("map: {error:?}"))
        .collect();
    assert_eq!(portions.len(), 1);
    let big_code_analysis_ast::documentation::OriginView::Copied(piece) = &portions[0].origin
    else {
        panic!("copied late origin")
    };
    assert_eq!(piece.range(), (size - 1) * 5 + 3..(size - 1) * 5 + 4);
    let mut group = criterion.benchmark_group("documentation/prepared");
    group.bench_function(BenchmarkId::new("owner", size), |bencher| {
        bencher.iter(|| {
            black_box(
                black_box(&index)
                    .node_associations(black_box(owner.as_tree_sitter()))
                    .unwrap_or_else(|error| panic!("owner: {error:?}"))
                    .associations()
                    .count(),
            )
        });
    });
    group.bench_function(BenchmarkId::new("late-map", size), |bencher| {
        bencher.iter(|| {
            for portion in black_box(text)
                .origins(black_box(text.text().len() - 1..text.text().len()))
                .unwrap_or_else(|error| panic!("map: {error:?}"))
            {
                black_box(portion);
            }
        });
    });
    group.bench_function(BenchmarkId::new("retained", size), |bencher| {
        bencher.iter(|| {
            black_box(
                black_box(&index)
                    .complete_text(black_box(document))
                    .unwrap_or_else(|error| panic!("text: {error:?}")),
            )
        });
    });
    group.finish();
}

#[allow(clippy::too_many_lines)] // Each workload keeps its untimed semantic control next to its fixture.
fn remediation(
    criterion: &mut Criterion,
    size: usize,
    context: &DocumentationContext,
    options: DocumentationOptions,
) {
    let depth = size / 16;
    let workloads = [
        (
            "inline-spans",
            LANG::Cpp,
            format!("/**{}*/\nvoid f() {{}}", "`x` ".repeat(size)),
        ),
        (
            "gathered-disposition",
            LANG::Cpp,
            format!("{}/**{}@file*/", "/**a*/\n".repeat(size), "x ".repeat(size)),
        ),
        (
            "complex-signatures",
            LANG::Cpp,
            format!(
                "{}void leaf();{}",
                "void f(int x = [] { ".repeat(depth),
                " return 1; }());".repeat(depth)
            ),
        ),
        (
            "order-multiple-chunks",
            LANG::Rust,
            "///audit\nfn f() {}\n".repeat(size),
        ),
        (
            "text-budget-boundary",
            LANG::Rust,
            format!("///{}\nfn f() {{}}", "abcd".repeat(size * 2)),
        ),
        (
            "go-fold",
            LANG::Go,
            format!(
                "package p\ntype S struct {{\n{}X int\n{} }}\n",
                "X struct {\n".repeat(depth),
                "}\n".repeat(depth)
            ),
        ),
        (
            "ruby-fold",
            LANG::Ruby,
            format!(
                "{}# audit\ndef f; end\n{}",
                "class C\n".repeat(depth),
                "end\n".repeat(depth)
            ),
        ),
    ];
    let mut group = criterion.benchmark_group("documentation/remediation");
    for (name, language, source) in workloads {
        let parser = AnyParser::parse(language, source.into_bytes(), None, None)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(!parser.root_node().has_error(), "{name}");
        let checked = parser
            .documentation(context, &options)
            .unwrap_or_else(|error| panic!("{error:?}"));
        match name {
            "inline-spans" => {
                validation::documents(&checked, 1, 1, 1, &"`x` ".repeat(size)[..size * 4 - 1]);
            }
            "gathered-disposition" => {
                assert_eq!(checked.documents().len(), 1);
                assert_eq!(
                    checked
                        .documents()
                        .next()
                        .unwrap_or_else(|| panic!("doc"))
                        .get()
                        .fragment_count(),
                    size + 1
                );
            }
            "complex-signatures" => {
                assert_eq!(checked.documents().len(), 0);
                assert_eq!(
                    parser
                        .root_node()
                        .preorder()
                        .filter(|node| node.kind() == "declaration")
                        .count(),
                    depth + 1
                );
                assert_eq!(
                    checked
                        .assessments()
                        .filter(
                            |a| a.get().reason == "Complex signature awaits exact named matching"
                        )
                        .count(),
                    depth
                );
            }
            "go-fold" => {
                assert_eq!(checked.documents().len(), 0);
                assert_eq!(
                    parser
                        .root_node()
                        .preorder()
                        .filter(|node| node.kind() == "field_declaration")
                        .count(),
                    depth + 1
                );
            }
            "order-multiple-chunks" => validation::documents(&checked, size, 1, 1, "audit"),
            "text-budget-boundary" => {
                validation::documents(&checked, 1, 1, 1, &"abcd".repeat(size * 2));
            }
            "ruby-fold" => validation::documents(&checked, 1, 1, 1, "audit"),
            _ => unreachable!(),
        }
        drop(checked);
        group.bench_with_input(BenchmarkId::new(name, size), &parser, |bencher, parser| {
            bencher.iter(|| {
                black_box(
                    black_box(parser)
                        .documentation(black_box(context), black_box(&options))
                        .unwrap_or_else(|error| panic!("{error:?}")),
                )
            });
        });
    }
    group.finish();
}

fn multiline_ranges(
    criterion: &mut Criterion,
    size: usize,
    context: &DocumentationContext,
    options: DocumentationOptions,
) {
    use big_code_analysis_ast::tree_sitter;
    let source = format!("/**\n{}*/\nclass C {{}}", " * audit\n".repeat(size));
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(
            &LANG::Java
                .tree_sitter_language()
                .unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let mut point = tree_sitter::Point::new(0, 0);
    let ranges: Vec<_> = source
        .bytes()
        .enumerate()
        .map(|(at, byte)| {
            let start_point = point;
            if byte == b'\n' {
                point.row += 1;
                point.column = 0;
            } else {
                point.column += 1;
            }
            tree_sitter::Range {
                start_byte: at,
                end_byte: at + 1,
                start_point,
                end_point: point,
            }
        })
        .collect();
    parser
        .set_included_ranges(&ranges)
        .unwrap_or_else(|error| panic!("{error}"));
    let tree = parser
        .parse(&source, None)
        .unwrap_or_else(|| panic!("parse"));
    assert!(!tree.root_node().has_error());
    let checked = big_code_analysis_ast::documentation::collect(
        LANG::Java,
        &tree,
        source.as_bytes(),
        context,
        &options,
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    validation::documents(
        &checked,
        1,
        1,
        1,
        &std::iter::repeat_n("audit", size)
            .collect::<Vec<_>>()
            .join("\n"),
    );
    drop(checked);
    let mut group = criterion.benchmark_group("documentation/remediation");
    group.bench_function(
        BenchmarkId::new("multiline-adjacent-ranges", size),
        |bencher| {
            bencher.iter(|| {
                black_box(
                    big_code_analysis_ast::documentation::collect(
                        LANG::Java,
                        black_box(&tree),
                        black_box(source.as_bytes()),
                        black_box(context),
                        black_box(&options),
                    )
                    .unwrap_or_else(|error| panic!("{error:?}")),
                )
            });
        },
    );
    group.finish();
}
