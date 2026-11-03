//! The cross-language documentation tables (#1699): every language's
//! `DocTarget` owners outside the space tree, named with why, and the
//! gated-out shapes. Since #1700 the relations themselves —
//! `DocTarget ⊆ FuncOpener ∪ Container ∪ Member ∪ named` on the lists
//! and on every corpus node, `DocComment` within `is_comment`'s union,
//! `Docstring` outside it — are asked by `roles/derived_parity_tests.rs`
//! of every table and candidate; the three hand-written tests this
//! file carried were retired under that issue's protocol, each after a
//! perturbation of the mechanism it guarded failed the derived test
//! (recorded on #1700). What stays here is the data and the pin that
//! every language with a `DocTarget` row has a table.

use super::*;

/// One language's `DocTarget` row beside its three space-tree rows,
/// with every target outside the space tree named: the exception table
/// of the `DocTarget ⊆ space tree ∪ named` relation, read by the
/// derived relation in `derived_parity_tests.rs` (#1700) so the two
/// cannot drift.
pub(in crate::roles) struct Table {
    pub(in crate::roles) lang: &'static str,
    pub(in crate::roles) doc_target: &'static [u16],
    pub(in crate::roles) space: [&'static [u16]; 3],
    /// `(kind id, reason)` for each target the space tree does not own.
    pub(in crate::roles) beyond: Vec<(u16, &'static str)>,
}

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
#[allow(
    clippy::too_many_lines,
    clippy::vec_init_then_push,
    unused_macros,
    unused_variables,
    unused_mut
)]
pub(in crate::roles) fn doc_target_tables() -> Vec<Table> {
    use crate::roles::*;
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
    tables
}

/// The ids of every `DocTarget` owner of `lang` the space tree does
/// not own, by the table above; empty for a language with no table.
pub(in crate::roles) fn targets_beyond_the_space_tree(lang: &str) -> Vec<u16> {
    doc_target_tables()
        .into_iter()
        .find(|t| t.lang == lang)
        .map(|t| t.beyond.into_iter().map(|(k, _)| k).collect())
        .unwrap_or_default()
}

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
fn every_language_with_a_doc_target_row_has_a_table() {
    let tables = doc_target_tables();
    assert!(
        !tables.is_empty(),
        "no language enabled for the parity table"
    );
    // Every enabled language with a `DocTarget` owner has a table here —
    // a 26th language cannot fall outside the space tree silently. The
    // list of rows is the one `roles::tests` holds to `LANG`; the
    // tables are feature-gated, so the expectation is too. The two
    // directions of the relation itself — every target outside the
    // space tree named, every name still such a target — are the
    // derived module's (`every_row_relation_holds_on_the_lists_and_the_corpus`,
    // `every_named_target_beyond_the_space_tree_is_still_one`), retired
    // from here under #1700's protocol with the perturbations recorded
    // on the issue.
    let listed: BTreeSet<&str> = tables.iter().map(|t| t.lang).collect();
    let enabled: BTreeSet<String> = crate::LANG::into_enum_iter()
        .filter(crate::LANG::is_enabled)
        .map(|lang| format!("{lang:?}"))
        .collect();
    let with_targets: BTreeSet<&str> = crate::roles::tables()
        .iter()
        .filter(|t| enabled.contains(t.name))
        .filter(|t| {
            (t.rows)().iter().any(|row| {
                row.role() == "DocTarget" && row.declared().is_some_and(|r| !r.owners.is_empty())
            })
        })
        .map(|t| t.name)
        .collect();
    pretty_assertions::assert_eq!(
        listed,
        with_targets,
        "every language with a `DocTarget` row"
    );
}

/// The `DocTarget` owners of `lang` the space tree lists but rejects
/// by gate, as `(kind, first line)` on the documentation fixtures: C#'s
/// bodied property and indexer, which `Member` lists and whose accessor
/// gate rejects, beside the expression-bodied `int Q => 1;` the gate
/// admits. Read by the derived relation in `derived_parity_tests.rs`,
/// which collects the exact gated-out shapes of every corpus candidate
/// and compares them to this set in both directions (#1700).
pub(in crate::roles) fn gated_out_of_the_space_tree(
    lang: &str,
) -> &'static [(&'static str, &'static str)] {
    match lang {
        "Csharp" => &[
            ("property_declaration", "int P { /// in accessors"),
            ("indexer_declaration", "int this[int i] { get => i; }"),
        ],
        _ => &[],
    }
}
