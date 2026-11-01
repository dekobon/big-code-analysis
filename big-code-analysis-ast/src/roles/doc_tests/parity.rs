//! The cross-language pins (#1699): the derived parity
//! `owns_DocTarget ⊆ owns_FuncOpener ∪ owns_Container ∪ owns_Member`,
//! on the declared lists and on gated nodes, with every exception
//! named; and `is_doc_comment ⇒ is_comment` as the Checker answers it,
//! with `Docstring` reaching neither predicate.

use super::*;

/// Every language's `DocTarget` row against its three space-tree rows,
/// by declared kind id: a target outside the space tree must be named
/// here with the reason, and a name here must still be a target outside
/// the space tree, so a stale entry fails in either direction. The
/// reasons live here rather than as `excludes` on a space-tree row: an
/// `excludes` records a kind that row's author *considered* for the
/// role and rejected, and no space-tree row considered an enum constant
/// or a macro a member or a container — it never was one, which is a
/// fact about the target, not about the row (D2 of the plan, #1699). No
/// gate is applied: this is the declared tables, so a kind owned under
/// a gate counts as owned.
#[cfg(any(
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "go",
    feature = "groovy",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "php",
    feature = "ruby",
    feature = "rust",
    feature = "typescript",
))]
#[test]
#[allow(clippy::too_many_lines, clippy::vec_init_then_push)]
fn doc_targets_outside_the_space_tree_are_each_named() {
    use crate::roles::*;
    use std::collections::BTreeMap;

    struct Table {
        lang: &'static str,
        doc_target: &'static [u16],
        space: [&'static [u16]; 3],
        /// `(kind id, reason)` for each target the space tree does not own.
        beyond: Vec<(u16, &'static str)>,
    }
    macro_rules! table {
        ($lang:ident, $roles:ident, [$($kind:ident: $why:literal),* $(,)?]) => {
            Table {
                lang: stringify!($lang),
                doc_target: $roles::doc_target::OWNERS,
                space: [
                    $roles::func_opener::OWNERS,
                    $roles::container::OWNERS,
                    $roles::member::OWNERS,
                ],
                beyond: vec![$((crate::$lang::$kind as u16, $why)),*],
            }
        };
    }
    // The C-family targets beyond the space tree, by kind name, so the
    // four dialects share one list resolved per enum; a kind the dialect's
    // own space tree owns (a C++ namespace, struct or field) is no
    // exception there and gets no name, so the stale check stays exact.
    #[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
    let c_entries = |l: &'static str, space: &[&[u16]]| -> Vec<(u16, &'static str)> {
        let owned: BTreeSet<u16> = space.iter().copied().flatten().copied().collect();
        let names: &[(&str, &str)] = &[
            (
                "declaration",
                "a declaration Doxygen documents; not a container or member",
            ),
            ("type_definition", "a typedef"),
            ("preproc_def", "a macro"),
            ("preproc_function_def", "a function-like macro"),
            ("linkage_specification", "an `extern \"C\"` block"),
            ("enumerator", "an enumerator"),
            (
                "struct_specifier",
                "a struct with a body; `Container` opens it in C++, where it is no exception",
            ),
            ("enum_specifier", "an enum with a body; no space of its own"),
            (
                "union_specifier",
                "a union with a body; no space of its own",
            ),
            (
                "field_declaration",
                "a member Doxygen documents; a `Member` in C++, where it is no exception",
            ),
            (
                "expression_statement",
                "a macro call at file scope, which Doxygen reads as a function",
            ),
            (
                "namespace_definition",
                "a namespace; `Container` opens it in C++, where it is no exception",
            ),
            ("using_declaration", "a `using`"),
            ("alias_declaration", "a type alias"),
            ("friend_declaration", "a `friend`"),
            (
                "template_declaration",
                "a template, forward-declared included",
            ),
            (
                "qualified_protocol_interface_declaration",
                "a protocol's `@optional` / `@required` section",
            ),
            (
                "method_declaration",
                "an Objective-C method declaration, bodiless",
            ),
            (
                "implementation_definition",
                "the wrapper of an Objective-C method definition",
            ),
            ("preproc_if", "a preprocessor conditional holding an entry"),
            (
                "preproc_ifdef",
                "a preprocessor conditional holding an entry",
            ),
            (
                "preproc_else",
                "a preprocessor conditional holding an entry",
            ),
            (
                "preproc_elif",
                "a preprocessor conditional holding an entry",
            ),
            (
                "preproc_elifdef",
                "a preprocessor conditional holding an entry",
            ),
        ];
        let flags: &[(u16, bool, bool)] = match l {
            #[cfg(feature = "c")]
            "C" => crate::C_KIND_FLAGS,
            #[cfg(feature = "cpp")]
            "Cpp" => crate::CPP_KIND_FLAGS,
            #[cfg(feature = "mozcpp")]
            "Mozcpp" => crate::MOZCPP_KIND_FLAGS,
            #[cfg(feature = "objc")]
            "Objc" => crate::OBJC_KIND_FLAGS,
            _ => &[],
        };
        let kind_name = |id: u16| -> &'static str {
            match l {
                #[cfg(feature = "c")]
                "C" => crate::C::from(id).into(),
                #[cfg(feature = "cpp")]
                "Cpp" => crate::Cpp::from(id).into(),
                #[cfg(feature = "mozcpp")]
                "Mozcpp" => crate::Mozcpp::from(id).into(),
                #[cfg(feature = "objc")]
                "Objc" => crate::Objc::from(id).into(),
                _ => "",
            }
        };
        flags
            .iter()
            .map(|(id, ..)| *id)
            .filter(|id| !owned.contains(id))
            .filter_map(|id| {
                names
                    .iter()
                    .find(|(name, _)| *name == kind_name(id))
                    .map(|(_, why)| (id, *why))
            })
            .collect()
    };
    let mut tables: Vec<Table> = Vec::new();
    #[cfg(feature = "c")]
    tables.push({
        let t = table!(C, c, []);
        Table {
            beyond: c_entries("C", &t.space),
            ..t
        }
    });
    #[cfg(feature = "cpp")]
    tables.push({
        let t = table!(Cpp, cpp, []);
        Table {
            beyond: c_entries("Cpp", &t.space),
            ..t
        }
    });
    #[cfg(feature = "mozcpp")]
    tables.push({
        let t = table!(Mozcpp, mozcpp, []);
        Table {
            beyond: c_entries("Mozcpp", &t.space),
            ..t
        }
    });
    #[cfg(feature = "objc")]
    tables.push({
        let t = table!(Objc, objc, []);
        Table {
            beyond: c_entries("Objc", &t.space),
            ..t
        }
    });
    #[cfg(feature = "csharp")]
    tables.push(table!(Csharp, csharp, [
        DelegateDeclaration: "a delegate: a type with no body to open a space",
        EnumMemberDeclaration: "an enum member",
        EventFieldDeclaration: "an event field, which the attribute count does not read",
        EventDeclaration: "an event with accessors, which the attribute count does not read",
    ]));
    #[cfg(feature = "go")]
    tables.push(table!(Go, go, [
        PackageClause: "the package clause",
        TypeDeclaration: "a `type` declaration; the struct it declares opens no space",
        VarDeclaration: "a `var` declaration",
        ConstDeclaration: "a `const` declaration",
        TypeSpec: "a spec of a grouped `type ( … )`",
        TypeAlias: "a grouped alias",
        VarSpec: "a spec of a grouped `var ( … )`",
        ConstSpec: "a spec of a grouped `const ( … )`",
        MethodElem: "an interface's method element",
        TypeElem: "an interface's embedded element",
    ]));
    #[cfg(feature = "groovy")]
    tables.push(table!(Groovy, groovy, [EnumConstant: "an enum constant"]));
    #[cfg(feature = "java")]
    tables.push(table!(Java, java, [
        PackageDeclaration: "the package declaration, in `package-info.java`'s shape",
        ModuleDeclaration: "a module",
        AnnotationTypeElementDeclaration: "an annotation type's element, an abstract method at the bytecode level",
        EnumConstant: "an enum constant",
    ]));
    #[cfg(feature = "javascript")]
    tables.push(table!(Javascript, javascript, [
        Pair: "an object member",
        ShorthandPropertyIdentifier: "an object member",
        VariableDeclaration: "a `var` declaration whose first declarator binds a name",
        LexicalDeclaration: "a `let` / `const` declaration whose first declarator binds a name",
        UsingDeclaration: "a `using` declaration",
        ExportStatement: "an export of a declaration",
        ExpressionStatement: "an assignment to a name or a member",
    ]));
    #[cfg(feature = "mozjs")]
    tables.push(table!(Mozjs, mozjs, [
        Pair: "as JavaScript",
        ShorthandPropertyIdentifier: "as JavaScript",
        VariableDeclaration: "as JavaScript",
        LexicalDeclaration: "as JavaScript",
        UsingDeclaration: "as JavaScript",
        ExportStatement: "as JavaScript",
        ExpressionStatement: "as JavaScript",
    ]));
    #[cfg(feature = "kotlin")]
    tables.push(table!(Kotlin, kotlin, [
        TypeAlias: "a type alias",
        EnumEntry: "an enum entry",
        Parameter: "a parameter, which Dokka documents",
        ClassParameter: "a primary constructor's parameter, a property when `val` / `var`",
    ]));
    #[cfg(feature = "lua")]
    tables.push(table!(Lua, lua, [
        AssignmentStatement: "an assignment to one name, an LDoc item",
        AssignmentStatement2: "an assignment to one name, an LDoc item (alias)",
        VariableDeclaration: "a `local` assigning one name, an LDoc item",
        VariableDeclaration2: "a `local` assigning one name, an LDoc item (alias)",
        Field: "a named table field, an LDoc item",
        ReturnStatement: "a `return` of a function or table, an LDoc item",
    ]));
    #[cfg(feature = "php")]
    tables.push(table!(Php, php, [EnumCase: "an enum case, a tag rather than a member"]));
    #[cfg(feature = "ruby")]
    tables.push(table!(Ruby, ruby, [
        Alias: "a method alias",
        Assignment: "a constant or class-variable assignment",
        Assignment2: "a constant or class-variable assignment (alias)",
        Call: "an `attr*`, `alias_method` or visibility-wrapped `def` call",
        Call2: "as `Call` (alias)",
        Call3: "as `Call` (alias)",
        Call4: "as `Call` (alias)",
    ]));
    #[cfg(feature = "rust")]
    tables.push(table!(Rust, rust, [
        ModItem: "a module",
        StructItem: "a struct; its fields are the members, the struct opens no space",
        UnionItem: "a union",
        EnumItem: "an enum",
        EnumVariant: "an enum variant",
        TypeItem: "a type alias",
        FunctionSignatureItem: "a trait's bodiless function signature",
        UseDeclaration: "a `use`",
        ExternCrateDeclaration: "an `extern crate`",
        MacroDefinition: "a `macro_rules!` definition",
    ]));
    #[cfg(feature = "typescript")]
    tables.push(table!(Typescript, typescript, [
        Pair: "as JavaScript",
        ShorthandPropertyIdentifier: "as JavaScript",
        VariableDeclaration: "as JavaScript",
        LexicalDeclaration: "as JavaScript",
        ExportStatement: "an export of a declaration or a list",
        ExpressionStatement: "a `using` declaration or an `internal_module`",
        TypeAliasDeclaration: "a type alias",
        EnumDeclaration: "an enum",
        Module: "a module",
        AmbientDeclaration: "an ambient declaration",
        FunctionSignature: "an overload signature",
        MethodSignature: "an interface's method signature",
        AbstractMethodSignature: "an abstract method signature",
        IndexSignature: "an index signature",
        CallSignature: "a call signature",
        ConstructSignature: "a construct signature",
        EnumAssignment: "an enum member with an initializer",
        RequiredParameter: "a parameter, which tsc documents",
        RequiredParameter2: "a parameter (alias)",
        OptionalParameter: "an optional parameter",
        OptionalParameter2: "an optional parameter (alias)",
        PropertyIdentifier: "an enum member without an initializer, under the enum body",
        ComputedPropertyName: "an enum member with a computed name, under the enum body",
        String: "an enum member spelled as a string, under the enum body",
    ]));
    #[cfg(feature = "typescript")]
    tables.push(table!(Tsx, tsx, [
        Pair: "as TypeScript",
        ShorthandPropertyIdentifier: "as TypeScript",
        VariableDeclaration: "as TypeScript",
        LexicalDeclaration: "as TypeScript",
        ExportStatement: "as TypeScript",
        ExpressionStatement: "as TypeScript",
        TypeAliasDeclaration: "as TypeScript",
        EnumDeclaration: "as TypeScript",
        Module: "as TypeScript",
        AmbientDeclaration: "as TypeScript",
        FunctionSignature: "as TypeScript",
        MethodSignature: "as TypeScript",
        AbstractMethodSignature: "as TypeScript",
        IndexSignature: "as TypeScript",
        CallSignature: "as TypeScript",
        ConstructSignature: "as TypeScript",
        EnumAssignment: "as TypeScript",
        RequiredParameter: "as TypeScript",
        RequiredParameter2: "as TypeScript",
        OptionalParameter: "as TypeScript",
        OptionalParameter2: "as TypeScript",
        PropertyIdentifier: "as TypeScript",
        ComputedPropertyName: "as TypeScript",
        String2: "an enum member spelled as a string — TSX's id for the `string` kind",
    ]));
    assert!(
        !tables.is_empty(),
        "no language enabled for the parity table"
    );
    // Every enabled language with a `DocTarget` owner has a table here —
    // a 26th language cannot fall outside the space tree silently. The
    // list of rows is the one `roles::tests` holds to `LANG`; the
    // tables are feature-gated, so the expectation is too.
    let listed: BTreeSet<&str> = tables.iter().map(|t| t.lang).collect();
    let enabled: BTreeSet<String> = crate::LANG::into_enum_iter()
        .filter(crate::LANG::is_enabled)
        .map(|lang| format!("{lang:?}"))
        .collect();
    let with_targets: BTreeSet<&str> = crate::roles::doc_target_rows()
        .iter()
        .filter(|(name, owners)| enabled.contains(*name) && !owners.is_empty())
        .map(|(name, _)| *name)
        .collect();
    pretty_assertions::assert_eq!(
        listed,
        with_targets,
        "every language with a `DocTarget` row"
    );

    let mut failures = Vec::new();
    for t in &tables {
        let owned: BTreeSet<u16> = t.space.iter().copied().flatten().copied().collect();
        let named: BTreeMap<u16, &str> = t.beyond.iter().copied().collect();
        for &k in t.doc_target {
            if !owned.contains(&k) && !named.contains_key(&k) {
                failures.push(format!(
                    "{}: target kind {k} is outside the space tree and unnamed",
                    t.lang
                ));
            }
        }
        for (&k, why) in &named {
            if !t.doc_target.contains(&k) {
                failures.push(format!(
                    "{}: named kind {k} ({why}) is no longer a target",
                    t.lang
                ));
            }
            if owned.contains(&k) {
                failures.push(format!(
                    "{}: named kind {k} ({why}) is in the space tree; the name is stale",
                    t.lang
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `is_doc_comment ⇒ is_comment` as the *Checker* answers it, on every
/// node of a fixture per language that spells the language's doc
/// comment, a plain comment and — for Python — a docstring: a node
/// `is_doc_comment` accepts is one `is_comment` accepts, and a
/// `Docstring` owner is accepted by neither. Asked of the Checker, not
/// the tables, so a forward that drops the `DocComment` half of
/// `is_comment`'s union (Groovy's `groovydoc_comment`, Perl's
/// `pod_statement` sit on that row alone) fails here. The non-vacuity
/// assertions: every language's fixture yields at least one comment
/// and, where the language has a `DocComment` row, at least one doc
/// comment; Python's at least one docstring.
#[cfg(any(
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "groovy",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "rust",
    feature = "typescript",
))]
#[test]
#[allow(clippy::too_many_lines)]
fn doc_comment_implies_comment_and_docstring_implies_neither() {
    use crate::roles::*;
    /// `(comments, doc comments, docstrings, violations)` of one walk.
    type Counts = (usize, usize, usize, Vec<String>);
    struct Case {
        lang: &'static str,
        source: &'static [u8],
        has_doc_row: bool,
        walk: fn(&[u8]) -> Counts,
    }
    macro_rules! case {
        ($lang:ident, $code:ident, $roles:ident, $source:literal) => {
            Case {
                lang: stringify!($lang),
                source: $source,
                has_doc_row: !$roles::doc_comment::OWNERS.is_empty(),
                walk: |source| {
                    let (mut comments, mut docs, mut docstrings) = (0, 0, 0);
                    let mut violations = Vec::new();
                    for_each_node_with_chain::<crate::$code>(source, |node, chain| {
                        let ancestors = Ancestors::known(chain);
                        let comment = crate::$code::is_comment(node);
                        let doc = crate::$code::is_doc_comment(node, source);
                        let docstring = $roles::docstring::owns(node, source, ancestors);
                        comments += usize::from(comment);
                        docs += usize::from(doc);
                        docstrings += usize::from(docstring);
                        if doc && !comment {
                            violations.push(format!(
                                "{}: doc comment that is no comment: {}",
                                stringify!($lang),
                                node.kind()
                            ));
                        }
                        if docstring && (comment || doc) {
                            violations.push(format!(
                                "{}: docstring reaching a comment predicate: {}",
                                stringify!($lang),
                                node.kind()
                            ));
                        }
                    });
                    (comments, docs, docstrings, violations)
                },
            }
        };
    }
    let cases: Vec<Case> = vec![
        #[cfg(feature = "c")]
        case!(C, CCode, c, b"/** d */\nint x;\n// p\n"),
        #[cfg(feature = "cpp")]
        case!(Cpp, CppCode, cpp, b"/** d */\nint x;\n// p\n"),
        #[cfg(feature = "mozcpp")]
        case!(Mozcpp, MozcppCode, mozcpp, b"/** d */\nint x;\n// p\n"),
        #[cfg(feature = "objc")]
        case!(Objc, ObjcCode, objc, b"/** d */\nint x;\n// p\n"),
        #[cfg(feature = "csharp")]
        case!(Csharp, CsharpCode, csharp, b"/// d\nclass C { }\n// p\n"),
        #[cfg(feature = "groovy")]
        case!(
            Groovy,
            GroovyCode,
            groovy,
            b"/** d */\nclass C { }\n// p\n/**/\n"
        ),
        #[cfg(feature = "java")]
        case!(Java, JavaCode, java, b"/** d */\nclass C { }\n// p\n"),
        #[cfg(feature = "javascript")]
        case!(
            Javascript,
            JavascriptCode,
            javascript,
            b"/** d */\nfunction f() { }\n// p\n"
        ),
        #[cfg(feature = "mozjs")]
        case!(
            Mozjs,
            MozjsCode,
            mozjs,
            b"/** d */\nfunction f() { }\n// p\n"
        ),
        #[cfg(feature = "kotlin")]
        case!(Kotlin, KotlinCode, kotlin, b"/** d */\nclass C\n// p\n"),
        #[cfg(feature = "perl")]
        case!(
            Perl,
            PerlCode,
            perl,
            b"=pod\n\nd\n\n=cut\n# p\n\n=begin comment\n\nc\n\n=end comment\n\n=cut\n"
        ),
        #[cfg(feature = "php")]
        case!(Php, PhpCode, php, b"<?php\n/** d */\nclass C { }\n// p\n"),
        #[cfg(feature = "python")]
        case!(Python, PythonCode, python, b"\"\"\"d\"\"\"\n# p\n"),
        #[cfg(feature = "rust")]
        case!(Rust, RustCode, rust, b"/// d\nfn f() { }\n// p\n"),
        #[cfg(feature = "typescript")]
        case!(
            Typescript,
            TypescriptCode,
            typescript,
            b"/** d */\nfunction f() { }\n// p\n"
        ),
        #[cfg(feature = "typescript")]
        case!(Tsx, TsxCode, tsx, b"/** d */\nfunction f() { }\n// p\n"),
    ];
    assert!(!cases.is_empty());
    for case in &cases {
        let (comments, docs, docstrings, violations) = (case.walk)(case.source);
        assert!(violations.is_empty(), "{}", violations.join("\n"));
        assert!(comments > 0, "{}: the fixture yields no comment", case.lang);
        assert_eq!(
            docs > 0,
            case.has_doc_row,
            "{}: doc comments vs the row",
            case.lang
        );
        assert_eq!(
            docstrings > 0,
            case.lang == "Python",
            "{}: docstrings",
            case.lang
        );
    }
}

/// The parity on *nodes*: for every node a language's `DocTarget` row
/// owns on its own fixture — with the node's real chain, gates applied
/// — one of the three space-tree rows owns the same node, or the kind
/// is one `doc_targets_outside_the_space_tree_are_each_named` names, or
/// the shape is one named here: a kind the space tree lists but owns
/// only under a gate the shape fails (C#'s bodied property, which is a
/// `FuncOpener` only in its accessor-less form). The table test above
/// compares declared lists; this one compares answers, which is what
/// the issue asks for, so a gate on a space-tree row that rejects a
/// target cannot hide behind the kind's presence in the list.
#[cfg(any(
    feature = "csharp",
    feature = "go",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "ruby",
    feature = "rust",
    feature = "typescript",
))]
#[test]
fn doc_targets_the_space_tree_rejects_by_gate_are_each_named() {
    use crate::roles::*;
    /// `(targets the row owns on the fixture, the gated-out ones)`: the
    /// first is the non-vacuity count, so a fixture that stops yielding
    /// any target cannot pass with an empty `gated_out`.
    type Walked = (usize, Vec<(String, String)>);
    struct Case {
        lang: &'static str,
        source: &'static [u8],
        /// `(kind, text)` of every target the space tree owns by list but
        /// not by gate on this fixture.
        gated_out: &'static [(&'static str, &'static str)],
        walk: fn(&[u8]) -> Walked,
    }
    macro_rules! case {
        ($lang:ident, $code:ident, $roles:ident, $source:literal, $gated_out:expr) => {
            Case {
                lang: stringify!($lang),
                source: $source,
                gated_out: $gated_out,
                walk: |source| {
                    let listed: std::collections::BTreeSet<u16> = [
                        $roles::func_opener::OWNERS,
                        $roles::container::OWNERS,
                        $roles::member::OWNERS,
                    ]
                    .into_iter()
                    .flatten()
                    .copied()
                    .collect();
                    let mut seen = Vec::new();
                    let mut targets = 0;
                    for_each_node_with_chain::<crate::$code>(source, |node, chain| {
                        let ancestors = Ancestors::known(chain);
                        if !$roles::doc_target::owns(node, source, ancestors) {
                            return;
                        }
                        targets += 1;
                        let owned = $roles::func_opener::owns(node, source, ancestors)
                            || $roles::container::owns(node, source, ancestors)
                            || $roles::member::owns(node, source, ancestors);
                        if !owned && listed.contains(&node.kind_id()) {
                            let text = node
                                .utf8_text(source)
                                .unwrap_or("")
                                .lines()
                                .next()
                                .unwrap_or("");
                            seen.push((node.kind().to_owned(), text.to_owned()));
                        }
                    });
                    (targets, seen)
                },
            }
        };
    }
    let cases: Vec<Case> = vec![
        #[cfg(feature = "csharp")]
        case!(Csharp, CsharpCode, csharp,
            b"class C { int P { get; set; } int Q => 1; int this[int i] { get => i; } int this[string s] => 1; void M() { } }\n",
            &[("property_declaration", "int P { get; set; }"), ("indexer_declaration", "int this[int i] { get => i; }")]),
        #[cfg(feature = "go")]
        case!(Go, GoCode, go, b"package p\ntype T struct { F int }\nfunc F() { }\nfunc (t T) M() { }\n", &[]),
        #[cfg(feature = "java")]
        case!(Java, JavaCode, java, b"class C { int f; void m() { } C() { } }\nenum E { K }\n", &[]),
        #[cfg(feature = "javascript")]
        case!(Javascript, JavascriptCode, javascript,
            b"function f() { }\nclass C { x = 1; m() { } }\nconst o = { p: 1 };\nvar v = 1;\n", &[]),
        #[cfg(feature = "kotlin")]
        case!(Kotlin, KotlinCode, kotlin, b"class C(val p: Int) {\n    val a = 1\n    fun f() { }\n}\nobject O { }\n", &[]),
        #[cfg(feature = "ruby")]
        case!(Ruby, RubyCode, ruby, b"class C\n  def m; end\n  def self.s; end\n  K = 1\nend\nmodule M; end\n", &[]),
        #[cfg(feature = "rust")]
        case!(Rust, RustCode, rust, b"struct S { f: u8 }\nimpl S { fn m() { } const C: u8 = 1; }\ntrait T { type A; fn g(); }\nfn f() { }\n", &[]),
        #[cfg(feature = "typescript")]
        case!(Typescript, TypescriptCode, typescript,
            b"function f() { }\nclass C { x = 1; m() { } }\ninterface I { p: number; m(): void; }\nconst v = 1;\n", &[]),
    ];
    assert!(!cases.is_empty());
    for case in &cases {
        let (targets, seen) = (case.walk)(case.source);
        assert!(targets > 0, "{}: the fixture yields no target", case.lang);
        let want: Vec<(String, String)> = case
            .gated_out
            .iter()
            .map(|(k, t)| ((*k).to_owned(), (*t).to_owned()))
            .collect();
        assert_eq!(
            seen, want,
            "{}: targets the space tree lists but rejects by gate",
            case.lang
        );
    }
}
