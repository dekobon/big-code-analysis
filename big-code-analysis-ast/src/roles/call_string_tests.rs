//! The `Call` and `StringLit` rows, pinned independently of the
//! classifiers that read them (#1698): per language, a fixture spelling
//! every call and string-literal kind, the expected `(is_call,
//! is_string)` of each such node by its text, as the two `_with_code`
//! predicates answered before the rows, and the byte-less twins beside
//! them where a gate makes the two spellings differ. Every declared
//! owner must be met by the fixture, or be one of the aliases the
//! pinned grammar never emits, which are required *not* to appear so a
//! bump that starts emitting one fails here rather than changing a
//! count unseen (grammar-dispatch §1 / §2).

use crate::test_support::{for_each_node_with_chain, owners_covered};
use crate::{Ancestors, Checker, LanguageInfo};

/// The `(is_call_with_code, is_string_with_code)` answers per matched
/// node, in document order, and the kinds the fixture emitted.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
fn verdicts<L: LanguageInfo + Checker>(
    source: &[u8],
    select: impl Fn(&str) -> bool,
) -> (Vec<(String, bool, bool)>, std::collections::BTreeSet<u16>) {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for_each_node_with_chain::<L>(source, |node, chain| {
        seen.insert(node.kind_id());
        let ancestors = Ancestors::known(chain);
        let call = L::is_call_with_code(node, source, ancestors);
        let string = L::is_string_with_code(node, source, ancestors);
        if select(node.kind()) && node.is_named() {
            let text = node.utf8_text(source).unwrap_or("<non-utf8>");
            let head: String = text.lines().next().unwrap_or("").chars().take(24).collect();
            out.push((head, call, string));
        } else {
            assert!(
                !call && !string,
                "{} `{}` is neither a call nor a string",
                node.kind(),
                node.utf8_text(source).unwrap_or("?")
            );
        }
    });
    (out, seen)
}

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
const C: (bool, bool) = (true, false);
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
const S: (bool, bool) = (false, true);

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
fn expect(got: &[(String, bool, bool)], want: &[(&str, (bool, bool))]) {
    let got: Vec<(&str, (bool, bool))> =
        got.iter().map(|(t, c, s)| (t.as_str(), (*c, *s))).collect();
    assert_eq!(got, want);
}

#[cfg(feature = "c")]
#[test]
fn c_call_and_string_rows() {
    use crate::roles::c::{call, string_lit};
    let src = b"int f(int x) { char c = 'c'; char *s = \"a\" \"b\"; return g(x) + (*fp)(1); }\n";
    let (got, seen) = verdicts::<crate::CCode>(src, |k| {
        matches!(
            k,
            "call_expression" | "string_literal" | "concatenated_string" | "char_literal"
        )
    });
    // The concatenation and both of its literals are strings; a char is not.
    expect(
        &got,
        &[
            ("'c'", (false, false)),
            ("\"a\" \"b\"", S),
            ("\"a\"", S),
            ("\"b\"", S),
            ("g(x)", C),
            ("(*fp)(1)", C),
        ],
    );
    // `CallExpression` is the never-emitted `preproc_call_expression`
    // alias (#1254).
    owners_covered(
        &seen,
        &[call::OWNERS, string_lit::OWNERS],
        &[crate::C::CallExpression as u16],
    );
}

#[cfg(any(feature = "cpp", feature = "mozcpp"))]
const CPP_SRC: &[u8] = b"int f() { auto s = \"a\" \"b\"; auto r = R\"(raw)\"; char c = 'c'; o.m(); p->n(); ns::g(); T t(1); return T(4); }\n";

#[cfg(feature = "cpp")]
#[test]
fn cpp_call_and_string_rows() {
    use crate::roles::cpp::{call, string_lit};
    let (got, seen) = verdicts::<crate::CppCode>(CPP_SRC, |k| {
        matches!(
            k,
            "call_expression"
                | "string_literal"
                | "concatenated_string"
                | "raw_string_literal"
                | "char_literal"
                | "init_declarator"
        )
    });
    // `T t(1)` is a declaration, `T(4)` an ordinary `call_expression`;
    // a char is no string. The declarators are rows so that the fixture
    // cannot lose either negative unseen.
    expect(
        &got,
        &[
            ("s = \"a\" \"b\"", (false, false)),
            ("\"a\" \"b\"", S),
            ("\"a\"", S),
            ("\"b\"", S),
            ("r = R\"(raw)\"", (false, false)),
            ("R\"(raw)\"", S),
            ("c = 'c'", (false, false)),
            ("'c'", (false, false)),
            ("o.m()", C),
            ("p->n()", C),
            ("ns::g()", C),
            ("t(1)", (false, false)),
            ("T(4)", C),
        ],
    );
    owners_covered(
        &seen,
        &[call::OWNERS, string_lit::OWNERS],
        &[crate::Cpp::CallExpression as u16],
    );
}

#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_call_and_string_rows() {
    use crate::roles::mozcpp::{call, string_lit};
    let (got, seen) = verdicts::<crate::MozcppCode>(CPP_SRC, |k| {
        matches!(
            k,
            "call_expression"
                | "string_literal"
                | "concatenated_string"
                | "raw_string_literal"
                | "char_literal"
                | "init_declarator"
        )
    });
    expect(
        &got,
        &[
            ("s = \"a\" \"b\"", (false, false)),
            ("\"a\" \"b\"", S),
            ("\"a\"", S),
            ("\"b\"", S),
            ("r = R\"(raw)\"", (false, false)),
            ("R\"(raw)\"", S),
            ("c = 'c'", (false, false)),
            ("'c'", (false, false)),
            ("o.m()", C),
            ("p->n()", C),
            ("ns::g()", C),
            ("t(1)", (false, false)),
            ("T(4)", C),
        ],
    );
    owners_covered(
        &seen,
        &[call::OWNERS, string_lit::OWNERS],
        &[crate::Mozcpp::CallExpression as u16],
    );
}

#[cfg(feature = "objc")]
#[test]
fn objc_call_and_string_rows() {
    use crate::roles::objc::{call, string_lit};
    let src = b"@implementation Foo\n- (void)m { [self n]; [[Foo alloc] init]; f(1); NSString *s = @\"s\"; char *c = \"a\" \"b\"; }\n@end\n";
    let (got, seen) = verdicts::<crate::ObjcCode>(src, |k| {
        matches!(
            k,
            "call_expression" | "message_expression" | "string_literal" | "concatenated_string"
        )
    });
    expect(
        &got,
        &[
            ("[self n]", C),
            ("[[Foo alloc] init]", C),
            ("[Foo alloc]", C),
            ("f(1)", C),
            ("@\"s\"", S),
            ("\"a\" \"b\"", S),
            ("\"a\"", S),
            ("\"b\"", S),
        ],
    );
    owners_covered(
        &seen,
        &[call::OWNERS, string_lit::OWNERS],
        &[crate::Objc::CallExpression as u16],
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_call_and_string_rows() {
    use crate::roles::csharp::{call, string_lit};
    let src = b"class C { C() : this(1) { } C(int x) { } void M() { M(); this.M(); new C(); var s = $\"a {x}\"; var v = @\"v\"; var u = \"u\"; var r = \"\"\"\n r\n \"\"\"; char c = 'c'; } }\n";
    let (got, seen) = verdicts::<crate::CsharpCode>(src, |k| {
        matches!(
            k,
            "invocation_expression"
                | "string_literal"
                | "verbatim_string_literal"
                | "raw_string_literal"
                | "interpolated_string_expression"
                | "constructor_initializer"
                | "object_creation_expression"
                | "character_literal"
        )
    });
    // `: this(1)` and `new C()` are construction, not calls; a char is
    // no string.
    expect(
        &got,
        &[
            (": this(1)", (false, false)),
            ("M()", C),
            ("this.M()", C),
            ("new C()", (false, false)),
            ("$\"a {x}\"", S),
            ("@\"v\"", S),
            ("\"u\"", S),
            ("\"\"\"", S),
            ("'c'", (false, false)),
        ],
    );
    // Every invocation the pinned grammar emits — a bare, a member, a
    // generic, a nullsafe call, in a class, struct, record, interface
    // and field initialiser — carries the third alias (probed with `bca
    // dump`); the first two are defensive (lesson 2).
    owners_covered(
        &seen,
        &[call::OWNERS, string_lit::OWNERS],
        &[
            crate::Csharp::InvocationExpression as u16,
            crate::Csharp::InvocationExpression2 as u16,
        ],
    );
}

#[cfg(feature = "go")]
#[test]
fn go_call_and_string_rows() {
    use crate::roles::go::{call, string_lit};
    let src = b"package p\nfunc f() { g(1); o.m(2); s := \"s\"; r := `r`; c := 'c'; t := T{X: 1}; _ = t }\n";
    let (got, seen) = verdicts::<crate::GoCode>(src, |k| {
        matches!(
            k,
            "call_expression"
                | "interpreted_string_literal"
                | "raw_string_literal"
                | "rune_literal"
                | "composite_literal"
        )
    });
    // A rune is no string; a composite literal `T{…}` is no call.
    expect(
        &got,
        &[
            ("g(1)", C),
            ("o.m(2)", C),
            ("\"s\"", S),
            ("`r`", S),
            ("'c'", (false, false)),
            ("T{X: 1}", (false, false)),
        ],
    );
    owners_covered(&seen, &[call::OWNERS, string_lit::OWNERS], &[]);
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_call_and_string_rows() {
    use crate::roles::groovy::{call, string_lit};
    // `foo bar baz` is three expression statements at the pinned grammar,
    // not a chain; `println "cmd"` is the `command_chain`.
    let src = b"class C { void m() { f(1); this.m(); new C(); println \"cmd\"; def s = 'x'; def g = \"a ${x} b\"; def t = \"\"\"tri\"\"\" } }\n";
    let (got, seen) = verdicts::<crate::GroovyCode>(src, |k| {
        matches!(
            k,
            "method_invocation" | "command_chain" | "string_literal" | "object_creation_expression"
        )
    });
    // `new C()` is construction, not a call.
    expect(
        &got,
        &[
            ("f(1)", C),
            ("this.m()", C),
            ("new C()", (false, false)),
            ("println \"cmd\"", C),
            ("\"cmd\"", S),
            ("'x'", S),
            ("\"a ${x} b\"", S),
            ("\"\"\"tri\"\"\"", S),
        ],
    );
    owners_covered(&seen, &[call::OWNERS, string_lit::OWNERS], &[]);
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_call_and_string_rows() {
    use crate::roles::kotlin::{call, string_lit};
    let src = b"fun f() {\n  g(1)\n  o.m(2)\n  val r = ::g\n  val s = \"t $f ${f + 1}\"\n  val u = \"\"\"raw\"\"\"\n}\n";
    let (got, seen) = verdicts::<crate::KotlinCode>(src, |k| {
        matches!(
            k,
            "call_expression"
                | "string_literal"
                | "multiline_string_literal"
                | "callable_reference"
        )
    });
    // A callable reference `::g` names a function without calling it.
    expect(
        &got,
        &[
            ("g(1)", C),
            ("o.m(2)", C),
            ("::g", (false, false)),
            ("\"t $f ${f + 1}\"", S),
            ("\"\"\"raw\"\"\"", S),
        ],
    );
    owners_covered(&seen, &[call::OWNERS, string_lit::OWNERS], &[]);
}

#[cfg(feature = "lua")]
#[test]
fn lua_call_and_string_rows() {
    use crate::roles::lua::{call, string_lit};
    let src = b"f(1); o:m(2); o.n(3); print(\"s\", 'd', [[long]])\n";
    let (got, seen) = verdicts::<crate::LuaCode>(src, |k| matches!(k, "function_call" | "string"));
    expect(
        &got,
        &[
            ("f(1)", C),
            ("o:m(2)", C),
            ("o.n(3)", C),
            ("print(\"s\", 'd', [[long]]", C),
            ("\"s\"", S),
            ("'d'", S),
            ("[[long]]", S),
        ],
    );
    owners_covered(&seen, &[call::OWNERS, string_lit::OWNERS], &[]);
}

#[cfg(feature = "rust")]
#[test]
fn rust_call_and_string_rows() {
    use crate::roles::rust::{call, string_lit};
    let src = b"fn f() { g(1); S::new().m(); (|| 1)(); println!(\"m\"); let s = \"s\"; let r = r#\"r\"#; let c = 'c'; let b = b\"b\"; }\n";
    let (got, seen) = verdicts::<crate::RustCode>(src, |k| {
        matches!(
            k,
            "call_expression"
                | "string_literal"
                | "raw_string_literal"
                | "macro_invocation"
                | "char_literal"
        )
    });
    // A `macro_invocation` is not a call; its argument string is a
    // string. A char is no string.
    expect(
        &got,
        &[
            ("g(1)", C),
            ("S::new().m()", C),
            ("S::new()", C),
            ("(|| 1)()", C),
            ("println!(\"m\")", (false, false)),
            ("\"m\"", S),
            ("\"s\"", S),
            ("r#\"r\"#", S),
            ("'c'", (false, false)),
            ("b\"b\"", S),
        ],
    );
    owners_covered(&seen, &[call::OWNERS, string_lit::OWNERS], &[]);
}

#[cfg(feature = "bash")]
#[test]
fn bash_call_and_string_rows() {
    use crate::roles::bash::{call, string_lit};
    // In argument position `$"t"` is a `$` token and a `string` (#1358);
    // only the assignment spelling `a=$"x"` is a `translated_string`,
    // and both it and its inner `string` are strings.
    let src = b"f() { echo \"s $1\" 'd' $'a' $\"t\"; }\nf 1\ncat <<EOT\nhi $z\nEOT\na=$\"x\"\n";
    let (got, seen) = verdicts::<crate::BashCode>(src, |k| {
        matches!(
            k,
            "command"
                | "string"
                | "raw_string"
                | "ansi_c_string"
                | "translated_string"
                | "heredoc_body"
        )
    });
    // The translated string `$"t"` and its inner `string` are both
    // strings; the heredoc redirect hangs beside the `cat` command, not
    // inside it.
    expect(
        &got,
        &[
            ("echo \"s $1\" 'd' $'a' $\"t", C),
            ("\"s $1\"", S),
            ("'d'", S),
            ("$'a'", S),
            ("\"t\"", S),
            ("f 1", C),
            ("cat", C),
            ("hi $z", S),
            ("$\"x\"", S),
            ("\"x\"", S),
        ],
    );
    owners_covered(&seen, &[call::OWNERS, string_lit::OWNERS], &[]);
    // The three heredoc aliases the pinned grammar never emits (#1412).
    for k in [
        crate::Bash::HeredocBody as u16,
        crate::Bash::HeredocBody3 as u16,
        crate::Bash::SimpleHeredocBody as u16,
    ] {
        assert!(!seen.contains(&k), "heredoc alias {k} is now emitted");
    }
}

#[cfg(feature = "perl")]
#[test]
fn perl_call_and_string_rows() {
    use crate::roles::perl::{call, string_lit};
    // `method m { 1 }` is the `call_expression_with_sub` spelling and
    // `&$g(6)` the `call_expression_with_variable` one (`$f->(1)` is a
    // method invocation).
    let src = b"foo(1); bar; baz 1, 2; $o->m(3); &q(4); __SUB__->(5); my %h = (name => 1, -key => 2); my $s = 'x' . \"y $z\" . q(p) . qq(q $z) . `cmd` . qx(ls);\nmy $h = <<\"HD\";\nheredoc $z\nHD\nmethod m { 1 }\n&$g(6);\n";
    let (got, seen) = verdicts::<crate::PerlCode>(src, |k| {
        matches!(
            k,
            "call_expression_with_spaced_args"
                | "call_expression_with_sub"
                | "call_expression_with_args_with_brackets"
                | "call_expression_with_variable"
                | "call_expression_with_bareword"
                | "call_expression_recursive"
                | "method_invocation"
                | "string_single_quoted"
                | "string_double_quoted"
                | "string_q_quoted"
                | "string_qq_quoted"
                | "backtick_quoted"
                | "command_qx_quoted"
                | "heredoc_body_statement"
        )
    });
    // The bareword callee of `foo(1)` / `baz 1, 2` and the hash keys
    // `name` / `-key` are barewords that are not calls (#1580, #1545);
    // `bar` alone is.
    expect(
        &got,
        &[
            ("foo(1)", C),
            ("foo", (false, false)),
            ("bar", C),
            ("baz 1, 2", C),
            ("baz", (false, false)),
            ("$o->m(3)", C),
            ("&q(4)", C),
            ("&q", (false, false)),
            ("__SUB__->(5)", C),
            ("name", (false, false)),
            // `-key` parses as a file test `-k` on a bareword `ey`
            // (#1545, #1572 — FIXME(#1571 upstream)); the word is a key.
            ("ey", (false, false)),
            ("'x'", S),
            ("\"y $z\"", S),
            ("q(p)", S),
            ("qq(q $z)", S),
            ("`cmd`", S),
            ("qx(ls)", S),
            ("", S),
            ("method m { 1 }", C),
            ("&$g(6)", C),
        ],
    );
    owners_covered(&seen, &[call::OWNERS, string_lit::OWNERS], &[]);
    // The byte-less spelling is the kind table: every bareword call
    // kind is a call to it.
    let mut byteless = 0;
    for_each_node_with_chain::<crate::PerlCode>(src, |node, _| {
        if node.kind() == "call_expression_with_bareword" {
            assert!(crate::PerlCode::is_call(node));
            byteless += 1;
        }
    });
    assert_eq!(byteless, 6);
}

#[cfg(feature = "php")]
#[test]
fn php_call_and_string_rows() {
    use crate::roles::php::{call, string_lit};
    let src = b"<?php\nclass C { public string $p; function m(): string { f(1); $this->m(); self::n(); $o?->q(); new C(); $a = 'x'; $b = \"y $z\"; $c = `ls`; $d = <<<HD\nh $z\nHD; $e = <<<'ND'\nn\nND; return (string) $a; } }\n";
    let (got, seen) = verdicts::<crate::PhpCode>(src, |k| {
        matches!(
            k,
            "function_call_expression"
                | "member_call_expression"
                | "scoped_call_expression"
                | "nullsafe_member_call_expression"
                | "string"
                | "encapsed_string"
                | "heredoc"
                | "nowdoc"
                | "shell_command_expression"
                | "primitive_type"
                | "object_creation_expression"
                | "cast_expression"
        )
    });
    // The `string` type keyword (the `String2` leaf under each
    // `primitive_type`) and the `(string)` cast are not literals; `new
    // C()` is not a call. The keyword leaf is anonymous, so the wrapper
    // is the row that pins it.
    expect(
        &got,
        &[
            ("string", (false, false)),
            ("string", (false, false)),
            ("f(1)", C),
            ("$this->m()", C),
            ("self::n()", C),
            ("$o?->q()", C),
            ("new C()", (false, false)),
            ("'x'", S),
            ("\"y $z\"", S),
            ("`ls`", S),
            ("<<<HD", S),
            ("<<<'ND'", S),
            ("(string) $a", (false, false)),
        ],
    );
    // `String3` is the hidden `_string` supertype (lesson 34).
    owners_covered(
        &seen,
        &[call::OWNERS, string_lit::OWNERS],
        &[crate::Php::String3 as u16],
    );
    assert!(
        seen.contains(&(crate::Php::String2 as u16)),
        "the `string` keyword leaf is what the two `primitive_type` rows wrap"
    );
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_call_and_string_rows() {
    use crate::roles::ruby::{call, string_lit};
    let src = b"f(1); o.m; o&.n(2); s = \"a #{x} b\"; t = 'c'; u = \"d\" \"e\"; v = `cmd`; w = /re/; x = <<~HD\n  h #{x}\nHD\ny = %w[a b]; z = %i[c d]; q = :sym; r = :\"ds\"; c = ?c\n";
    let (got, seen) = verdicts::<crate::RubyCode>(src, |k| {
        matches!(
            k,
            "call"
                | "string"
                | "chained_string"
                | "bare_string"
                | "subshell"
                | "regex"
                | "heredoc_body"
                | "delimited_symbol"
                | "simple_symbol"
                | "string_array"
                | "symbol_array"
                | "character"
        )
    });
    expect(
        &got,
        &[
            ("f(1)", C),
            ("o.m", C),
            ("o&.n(2)", C),
            ("\"a #{x} b\"", S),
            ("'c'", S),
            ("\"d\" \"e\"", S),
            ("\"d\"", S),
            ("\"e\"", S),
            ("`cmd`", S),
            ("/re/", S),
            ("", S),
            ("%w[a b]", S),
            ("a", S),
            ("b", S),
            ("%i[c d]", S),
            (":sym", S),
            (":\"ds\"", S),
            ("?c", S),
        ],
    );
    // Every call spelling the pinned grammar emits — parenthesised,
    // command, safe-navigation, with a block, as an assignment target,
    // `f.()` — carries `Call3` (probed with `bca dump`); `Call`, `Call2`
    // and `Call4` are the other visible aliases and are defensive, and
    // `Call5` is the hidden `_call` (lesson 34).
    owners_covered(
        &seen,
        &[call::OWNERS, string_lit::OWNERS],
        &[
            crate::Ruby::Call as u16,
            crate::Ruby::Call2 as u16,
            crate::Ruby::Call4 as u16,
        ],
    );
    assert!(!seen.contains(&(crate::Ruby::Call5 as u16)));
}

#[cfg(any(feature = "tcl", feature = "irules"))]
fn tcl_family_expect(got: &[(String, bool, bool)]) {
    // The braced *values* are strings, the `proc` body and the `if`
    // body are not; a quoted word is one either way (#1381). `proc`,
    // `set` and `if` are their own kinds, not `command`s, and the
    // grammar parses the words inside a braced value as a command too.
    expect(
        got,
        &[
            ("{ return $a }", (false, false)),
            ("return $a", C),
            ("lappend l {a b}", C),
            ("{a b}", S),
            ("a b", C),
            ("\"q $x\"", S),
            ("{ f 1 }", (false, false)),
            ("f 1", C),
            // `set`'s braced value is the grammar's `braced_word_simple`.
            ("{c d}", S),
        ],
    );
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_call_and_string_rows() {
    use crate::roles::tcl::{call, string_lit};
    let src = b"proc f {a} { return $a }\nlappend l {a b}\nset s \"q $x\"\nif {$x} { f 1 }\nset x {c d}\n";
    let (got, seen) = verdicts::<crate::TclCode>(src, |k| {
        matches!(
            k,
            "command" | "quoted_word" | "braced_word" | "braced_word_simple"
        )
    });
    tcl_family_expect(&got);
    owners_covered(&seen, &[call::OWNERS, string_lit::OWNERS], &[]);
}

#[cfg(feature = "irules")]
#[test]
fn irules_call_and_string_rows() {
    use crate::roles::irules::{call, string_lit};
    let src = b"proc f {a} { return $a }\nlappend l {a b}\nset s \"q $x\"\nif {$x} { f 1 }\nset x {c d}\nwhen HTTP_REQUEST { f 2 }\n";
    let (got, seen) = verdicts::<crate::IrulesCode>(src, |k| {
        matches!(
            k,
            "command" | "quoted_word" | "braced_word" | "braced_word_simple"
        )
    });
    let (head, tail) = got.split_at(9);
    tcl_family_expect(head);
    // The handler body is a script, not a string; the handler is no call.
    expect(tail, &[("{ f 2 }", (false, false)), ("f 2", C)]);
    owners_covered(&seen, &[call::OWNERS, string_lit::OWNERS], &[]);
}

#[cfg(any(feature = "javascript", feature = "mozjs"))]
const JS_SRC: &[u8] = b"import \"m\";\nf(1); a.b(2); a?.c(3); new F(4); f`x`; f?.(5); (() => 1)();\nconst s = 's', d = \"d\", t = `t ${i} u`;\n@dec(6) class K {}\n";

#[cfg(feature = "javascript")]
#[test]
fn javascript_call_and_string_rows() {
    use crate::roles::javascript::{call, string_lit};
    let (got, seen) = verdicts::<crate::JavascriptCode>(JS_SRC, |k| {
        matches!(k, "call_expression" | "string" | "template_string")
    });
    expect(
        &got,
        &[
            ("\"m\"", S),
            ("f(1)", C),
            ("a.b(2)", C),
            ("a?.c(3)", C),
            ("f`x`", C),
            ("`x`", S),
            ("f?.(5)", C),
            ("(() => 1)()", C),
            ("'s'", S),
            ("\"d\"", S),
            ("`t ${i} u`", S),
            ("dec(6)", C),
        ],
    );
    // The named `String` kind is never emitted at the pin: every literal
    // — a module string, a property key, an alias — is the `String2`
    // alias (probed with `bca dump`), so `String` is defensive.
    owners_covered(
        &seen,
        &[call::OWNERS, string_lit::OWNERS],
        &[
            crate::Javascript::CallExpression2 as u16,
            crate::Javascript::String as u16,
        ],
    );
}

#[cfg(feature = "mozjs")]
#[test]
fn mozjs_call_and_string_rows() {
    use crate::roles::mozjs::{call, string_lit};
    let (got, seen) = verdicts::<crate::MozjsCode>(JS_SRC, |k| {
        matches!(k, "call_expression" | "string" | "template_string")
    });
    expect(
        &got,
        &[
            ("\"m\"", S),
            ("f(1)", C),
            ("a.b(2)", C),
            ("a?.c(3)", C),
            ("f`x`", C),
            ("`x`", S),
            ("f?.(5)", C),
            ("(() => 1)()", C),
            ("'s'", S),
            ("\"d\"", S),
            ("`t ${i} u`", S),
            ("dec(6)", C),
        ],
    );
    owners_covered(
        &seen,
        &[call::OWNERS, string_lit::OWNERS],
        &[
            crate::Mozjs::CallExpression2 as u16,
            crate::Mozjs::String as u16,
        ],
    );
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_call_and_string_rows() {
    use crate::roles::typescript::{call, string_lit};
    let src = b"import \"m\";\nlet s: string = 's', t = `t ${i}`;\nf(1); a?.c(3); new F(4); f<T>(5); f!(6); type Q = typeof g(7);\n@dec(8) class K { @m(9) x = 1; }\n";
    let (got, seen) = verdicts::<crate::TypescriptCode>(src, |k| {
        matches!(
            k,
            "call_expression" | "string" | "template_string" | "predefined_type"
        )
    });
    // `: string` is the `String2` keyword, not a literal (#1261): an
    // anonymous leaf, so its `predefined_type` wrapper is the row that
    // pins it, and the keyword's presence is asserted below.
    expect(
        &got,
        &[
            ("\"m\"", S),
            ("string", (false, false)),
            ("'s'", S),
            ("`t ${i}`", S),
            ("f(1)", C),
            ("a?.c(3)", C),
            ("f<T>(5)", C),
            ("f!(6)", C),
            ("g(7)", C),
            ("dec(8)", C),
            ("m(9)", C),
        ],
    );
    owners_covered(
        &seen,
        &[call::OWNERS, string_lit::OWNERS],
        &[
            crate::Typescript::CallExpression2 as u16,
            crate::Typescript::CallExpression3 as u16,
            crate::Typescript::CallExpression4 as u16,
        ],
    );
    assert!(
        seen.contains(&(crate::Typescript::String2 as u16)),
        "the `string` keyword leaf is what the `predefined_type` row wraps"
    );
}

#[cfg(feature = "typescript")]
#[test]
fn tsx_call_and_string_rows() {
    use crate::roles::tsx::{call, string_lit};
    let src = b"import \"m\";\nlet s: string = 's', t = `t ${i}`;\nf(1); a?.c(3); new F(4); f!(6); type Q = typeof g(7);\n@dec(8) class K { @m(9) x = 1; }\nconst J = () => <div className=\"x\">{f(10)}</div>;\n";
    let (got, seen) = verdicts::<crate::TsxCode>(src, |k| {
        matches!(
            k,
            "call_expression" | "string" | "template_string" | "predefined_type"
        )
    });
    // A JSX attribute literal is the `String2` alias, a string (#283);
    // `: string` is the `String3` keyword, an anonymous leaf pinned
    // through its `predefined_type` wrapper as in TypeScript.
    expect(
        &got,
        &[
            ("\"m\"", S),
            ("string", (false, false)),
            ("'s'", S),
            ("`t ${i}`", S),
            ("f(1)", C),
            ("a?.c(3)", C),
            ("f!(6)", C),
            ("g(7)", C),
            ("dec(8)", C),
            ("m(9)", C),
            ("\"x\"", S),
            ("f(10)", C),
        ],
    );
    owners_covered(
        &seen,
        &[call::OWNERS, string_lit::OWNERS],
        &[
            crate::Tsx::CallExpression2 as u16,
            crate::Tsx::CallExpression3 as u16,
            crate::Tsx::CallExpression4 as u16,
            // As in JavaScript: every TSX literal is the `String2` alias.
            crate::Tsx::String as u16,
        ],
    );
    assert!(
        seen.contains(&(crate::Tsx::String3 as u16)),
        "the `string` keyword leaf is what the `predefined_type` row wraps"
    );
}

/// `is_primitive` equals `lists` on every node of `source`, and every
/// owner but `unmet` is met.
#[cfg(all(
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "mozcpp",
    feature = "objc",
    feature = "rust",
    feature = "typescript",
))]
fn probe<L: LanguageInfo + Checker>(
    lang: &str,
    source: &[u8],
    owners: &[u16],
    lists: fn(u16) -> bool,
    unmet: &[u16],
) {
    let mut met = std::collections::BTreeSet::new();
    let nodes = for_each_node_with_chain::<L>(source, |node, _| {
        let kind = node.kind_id();
        assert_eq!(
            L::is_primitive(node),
            lists(kind),
            "{lang} {} `{}`: `is_primitive` is the row's kind table",
            node.kind(),
            node.utf8_text(source).unwrap_or("?")
        );
        if lists(kind) {
            met.insert(kind);
        }
    });
    assert!(nodes > 20, "{lang}: the fixture parsed to {nodes} nodes");
    for &k in owners {
        assert_eq!(
            met.contains(&k),
            !unmet.contains(&k),
            "{lang}: owner kind {k} met"
        );
    }
    for &k in unmet {
        assert!(owners.contains(&k), "{lang}: unmet kind {k} is declared");
    }
}

/// The byte-less `Checker::is_primitive` is the `Primitive` row's kind
/// table, gates aside, in every language that overrides it: it answers
/// true on exactly the nodes `lists_kind` claims and nothing else, on a
/// fixture per language spelling every owner the pinned grammar emits.
/// The C family's `sized_type_specifier` holds its `signed` /
/// `unsigned` / `long` / `short` leaves beside the `primitive_type`;
/// Objective-C adds `_Complex`; TypeScript's `predefined_type` wraps
/// `void`, which the row's gate declines in the `_with_code` spelling
/// and the table claims here. Every owner is met except the ones named
/// `unmet` — C's hidden `type_specifier` supertype, ObjC's second
/// `_Complex` alias, and the sixteen Rust `primitive_type` aliases a
/// fixture of six spells one of (#1693) — so a row that grows a kind
/// fails here rather than passing on the nodes the fixture has.
#[cfg(all(
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "mozcpp",
    feature = "objc",
    feature = "rust",
    feature = "typescript",
))]
#[test]
fn byte_less_is_primitive_is_the_rows_kind_table() {
    let c = b"int f(unsigned long x) { short s; signed char c; double d; return 0; }\n";
    let objc = b"@interface A @end\nint f(_Complex double z, unsigned long x, short s, signed char c) { return 0; }\n";
    let cs = b"class C { int f(string s, bool b, object o, var v) { var q = 1; return 0; } }\n";
    let ts = b"let a: number = 1; let b: string = \"s\"; let c: void = undefined; let d: any;\n";
    let rs = b"fn f(a: i32, b: u8, c: f64, d: bool, e: char, f: usize) {}\n";
    {
        use crate::roles::c::primitive;
        probe::<crate::CCode>(
            "c",
            c,
            primitive::OWNERS,
            primitive::lists_kind,
            &[crate::C::TypeSpecifier as u16],
        );
    }
    {
        use crate::roles::cpp::primitive;
        probe::<crate::CppCode>(
            "cpp",
            c,
            primitive::OWNERS,
            primitive::lists_kind,
            &[crate::Cpp::TypeSpecifier as u16],
        );
    }
    {
        use crate::roles::mozcpp::primitive;
        probe::<crate::MozcppCode>(
            "mozcpp",
            c,
            primitive::OWNERS,
            primitive::lists_kind,
            &[crate::Mozcpp::TypeSpecifier as u16],
        );
    }
    {
        use crate::roles::objc::primitive;
        probe::<crate::ObjcCode>(
            "objc",
            objc,
            primitive::OWNERS,
            primitive::lists_kind,
            &[
                crate::Objc::TypeSpecifier as u16,
                crate::Objc::Complex2 as u16,
            ],
        );
    }
    {
        use crate::roles::csharp::primitive;
        probe::<crate::CsharpCode>("csharp", cs, primitive::OWNERS, primitive::lists_kind, &[]);
    }
    {
        use crate::roles::typescript::primitive;
        probe::<crate::TypescriptCode>(
            "typescript",
            ts,
            primitive::OWNERS,
            primitive::lists_kind,
            &[],
        );
    }
    {
        use crate::roles::tsx::primitive;
        probe::<crate::TsxCode>("tsx", ts, primitive::OWNERS, primitive::lists_kind, &[]);
    }
    {
        use crate::roles::rust::primitive;
        let spelled = crate::Rust::PrimitiveType as u16;
        let unmet: Vec<u16> = primitive::OWNERS
            .iter()
            .copied()
            .filter(|&k| k != spelled)
            .collect();
        probe::<crate::RustCode>("rust", rs, primitive::OWNERS, primitive::lists_kind, &unmet);
    }
}
