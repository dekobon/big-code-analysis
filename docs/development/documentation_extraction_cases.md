# Documentation extraction form cases

This is the independently specified form inventory for contributors implementing
the [documentation extraction contract](documentation_extraction.md#acceptance-cases).
It supplements the primary language roster with inputs that distinguish attachment,
composition, text availability and profile choices. These are required outcomes,
not a claim that each adapter currently implements every case. The direct-AST
collector and a hidden root facade are available with partial adapter support.
Profile definitions and references
are in the contract.

Each source cell expands `\n`, `\r`, `\t` and `\\` only. Inputs are standalone
and have no unshown terminal newline unless an enclosing template is specified.
An owner such as `f` or `C.x` denotes the declaration occurrence or name slot in
that input, not a global name. Its range is the exact declaration/name spelling
in the input; repeated clauses are numbered in source order. Attribute fragments
span the whole attribute, literal fragments include quotes, and comment fragments
include markers but exclude a terminating LF/CRLF. Those spelling-based ranges
are independent expectations and must not be taken from extractor output.

Unless a row says otherwise, a single selected block or literal expression
produces one document with complete marker-cleaned text `audit`, resolved to
the named owner. Members of a selected run compose into that document in written
source order, using the contract's join rule; superseded members are evidence,
not extra selected prose. Publication is unknown unless a row supplies enough
information to classify it. The semantic role follows the contract; a syntactic
attribute form does not mean Python's attribute-binding role.

## Implementation ownership {#ownership}

| Case prefix | Owning issue |
| --- | --- |
| `rust-` | [#1664](https://github.com/dekobon/big-code-analysis/issues/1664) |
| `python-` | [#1665](https://github.com/dekobon/big-code-analysis/issues/1665) |
| `java-`, `kotlin-`, `groovy-` | [#1666](https://github.com/dekobon/big-code-analysis/issues/1666) |
| `csharp-` | [#1667](https://github.com/dekobon/big-code-analysis/issues/1667) |
| `php-` and PHP matrix | [#1668](https://github.com/dekobon/big-code-analysis/issues/1668) |
| `js-` | [#1669](https://github.com/dekobon/big-code-analysis/issues/1669) |
| `ts-` | [#1670](https://github.com/dekobon/big-code-analysis/issues/1670) |
| `doxygen-`, `objc-` | [#1671](https://github.com/dekobon/big-code-analysis/issues/1671) |
| `go-` | [#1672](https://github.com/dekobon/big-code-analysis/issues/1672) |
| `ruby-` | [#1673](https://github.com/dekobon/big-code-analysis/issues/1673) |
| `lua-` | [#1674](https://github.com/dekobon/big-code-analysis/issues/1674) |
| `perl-` | [#1675](https://github.com/dekobon/big-code-analysis/issues/1675) |
| `elixir-` and metadata matrix | [#1676](https://github.com/dekobon/big-code-analysis/issues/1676) |
| `bash-` | [#1677](https://github.com/dekobon/big-code-analysis/issues/1677) |
| `tcl-` | [#1678](https://github.com/dekobon/big-code-analysis/issues/1678) |
| `irules-` | [#1679](https://github.com/dekobon/big-code-analysis/issues/1679) |

## Comment and preamble cleanup {#comment-cleanup}

| Case | Source | Exact text/origins |
| --- | --- | --- |
| python-sphinx | `#: audit\nx = 1` | attribute x; one comment fragment 0..8; `audit` output 0..5 copies source 3..8 |
| ruby-block | `=begin\naudit\n=end\ndef f\nend` | Both YARD and RDoc: method f; fragment 0..17; `audit` output 0..5 copies 7..12; boundary lines no output |
| go-block | `package p\n/* audit */\nfunc F() {}` | function F; fragment 10..21; `audit` output 0..5 copies 13..18 |
| tcl-quoted-preamble | `namespace eval n { variable _ruff_preamble "# Heading\\n  detail" }` | namespace n; text `# Heading\n  detail`, escape maps to complete backslash-n token; # preserved |
| tcl-braced-preamble | `namespace eval n {\nvariable _ruff_preamble {\n# Heading\\n  detail\n}\n}` | namespace n; text `# Heading\\n  detail`, literal backslash-n copied; # preserved |
| tcl-braced-substitutions | `namespace eval n {\nvariable _ruff_preamble {\n# $name [call]\n}\n}` | namespace n; literal `# $name [call]`; no variable/command evaluation |
| tcl-braced-same-line | `namespace eval n { variable _ruff_preamble {# Heading\\n  detail} }` | grammar-boundary uncertainty; no confirmed namespace owner or complete preamble text |
| tcl-dynamic-preamble | `namespace eval n { variable _ruff_preamble "# $name" }` | namespace owner resolved; text unavailable(dynamic substitution), raw retained |

## Rust source forms

| Case | Source | Exact selected outcome |
| --- | --- | --- |
| rust-inner-module | `mod m { //!audit\n}` | 1 inner comment document -> module m, text audit |
| rust-outer-block | `/**audit*/\nfn f() {}` | 1 document -> f, text audit |
| rust-raw-attribute | `#[doc = r"audit"]\nfn f() {}` | 1 primary document in attribute form -> f, text audit; attribute remains code for legacy LOC |
| rust-mixed | `/// first\n#[doc = "second"]\nfn f() {}` | 1 document -> f, 2 contributing fragments in order, text `first\nsecond` |
| rust-field-variant | `enum E { ///audit\nV }` | 1 document -> variant E.V, text audit |
| rust-metadata-only | `#[doc(hidden)]\nfn f() {}` | 1 metadata-only record -> f; no prose; eligibility ineligible |
| rust-computed | `#[doc = include_str!("doc.md")]\nfn f() {}` | owner f resolved; raw attribute retained; complete text unavailable(external/computed) |
| rust-macro | `m! { ///audit\nfn f() {} }` | raw comment recognized; owner unresolved(macro expansion); text audit; publication unknown; no concrete f association |

## Elixir groups and states

Use module wrapper `defmodule M do\n` + body + `\nend` for every row.

| Case | Body | Exact selected outcome |
| --- | --- | --- |
| elixir-head | `@doc "audit"\ndef f(x)\ndef f(0), do: 0\ndef f(x), do: x` | 1 document audit -> M/def/f/1; owner group has head and two explicit clauses in source order |
| elixir-clause-replace | `@doc "old"\ndef f(0), do: 0\n@doc "new"\ndef f(x), do: x` | 1 group, text new; old prose superseded evidence; two clause pieces |
| elixir-arity | `@doc "one"\ndef f(x), do: x\n@doc "two"\ndef f(x,y), do: x+y` | 2 docs one/two -> distinct f/1, f/2 |
| elixir-default-arity | `@doc "audit"\ndef f(x \\\\ 0), do: x` | one source owner f/1; no synthetic f/0 |
| elixir-private | `@doc "audit"\ndefp f(), do: :ok` | source doc retained -> defp/f/0; compiler publication ineligible |
| elixir-moduledoc | `@moduledoc "audit"` | 1 module document audit -> M |
| elixir-typedoc | `@typedoc "audit"\n@type t :: integer()` | 1 type document audit -> M/type/t/0 |
| elixir-callback | `@doc "audit"\n@callback f() :: atom()` | 1 doc audit -> M/callback/f/0 |
| elixir-meta-text | `@doc since: "1"\n@doc "audit"\ndef f(), do: :ok` | one doc audit with metadata since=1 |
| elixir-false-then-text | `@doc false\n@doc "audit"\ndef f(), do: :ok` | one selected prose doc audit; false superseded evidence |
| elixir-text-then-false | `@doc "audit"\n@doc false\ndef f(), do: :ok` | one selected disabled record; text superseded evidence |
| elixir-raw-sigil | `@doc ~S(a#{x}b)\ndef f(), do: :ok` | one doc with literal `a#{x}b`, no interpolation |
| elixir-quote | `quote do\n@doc "audit"\ndef f(), do: :ok\nend` | template evidence only; no concrete M/f owner |

## TclOO and iRules

| Case | Source | Exact selected outcome |
| --- | --- | --- |
| tcl-method | `oo::class create C {\nmethod f {} {\n# audit\nreturn\n}\n}` | one doc audit -> direct method command C.f; no metric function space promotion |
| tcl-constructor | `oo::class create C {\nconstructor {} {\n# audit\n}\n}` | one doc audit -> constructor command C; not class prose |
| tcl-destructor | `oo::define C {\ndestructor {\n# audit\n}\n}` | one doc audit -> destructor occurrence under definition C |
| tcl-outside-method | `method f {} {\n# audit\n}` | no native concrete method owner; owner unsupported outside specified class context |
| tcl-dynamic-class | `oo::class create $name {\nmethod f {} {\n# audit\n}\n}` | raw source candidate; class/method association unresolved(dynamic name) |
| irules-two-handlers | `# first\nwhen HTTP_REQUEST {}\n# second\nwhen HTTP_REQUEST priority 200 {}` | two docs -> separate handler occurrences, source order; priority not identity substitution |
| irules-body | `when HTTP_REQUEST {\n# audit\n}` | one body-role doc audit -> handler occurrence |
| irules-separated | `# audit\n\nwhen HTTP_REQUEST {}` | no selected leading doc; candidate unattached under evidence option |

## Lua source contexts

| Case | Source | Exact selected outcome |
| --- | --- | --- |
| lua-first-file | `---audit\nfunction f() end` | module/file doc audit, no f association |
| lua-after-module | `module("m")\n---audit\nfunction f() end` | function f doc audit; module() itself creates no documentation prose occurrence |
| lua-ordinary-leading | `module("m")\n-- audit\nfunction f() end` | default no selected doc; explicit leading-inference doc audit -> f |
| lua-table-field | `module("m")\n--- settings\n-- @table options\noptions = {\nalpha = 1, -- correction\n}` | table doc `settings\n@table options` -> options; native field doc correction -> alpha slot |
| lua-parameter | `module("m")\n---audit\nfunction f(\nx -- input\n) end` | function doc audit; native parameter doc input -> x slot |
| lua-explicit-virtual | `module("m")\n---audit\n-- @function ghost\n` | explicit virtual function ghost with local text `audit\n@function ghost`; no fabricated concrete node |

## JVM, C# and PHP owner controls

| Case | Source | Exact selected outcome |
| --- | --- | --- |
| java-annotation | `/**audit*/\n@Deprecated class C {}` | one doc audit -> C, wrapper retained |
| java-explicit-constructor | `class C { /**audit*/ C() {} }` | one doc audit -> constructor C(), not class C |
| java-compact-constructor | `record R(int x) { /**audit*/ R {} }` | one doc audit -> compact constructor R, not synthesized header |
| java-markdown-separated | `///old\n\n///new\nclass C {}` | new -> C; old superseded/dangling evidence, never joined |
| java-markdown-slash | `////\nclass C {}` | one doc / -> C |
| java-unicode | `/** \\u0040audit */\nclass C {}` | owner C source candidate; full semantic text unavailable(prelex); raw/source-cleaned spelling retained |
| kotlin-annotation | `/**audit*/\n@Deprecated("x") fun f() {}` | one doc audit -> f |
| kotlin-secondary | `class C { /**audit*/ constructor(x: Int) {} }` | one doc audit -> secondary constructor, not C |
| kotlin-parameter | `class C(/**audit*/ val x: Int)` | one doc audit -> x parameter/property slot |
| groovy-constructor | `class C { /**audit*/ C() {} }` | one doc audit -> explicit constructor C |
| groovy-field | `class C { /**audit*/ int x }` | one doc audit -> field x |
| csharp-attribute | `///<summary>audit</summary>\n[System.Obsolete] class C {}` | one XML doc -> C; text `<summary>audit</summary>` |
| csharp-property | `class C {\n///<summary>audit</summary>\nint P => 1;\n}` | one doc, text `<summary>audit</summary>` -> P, not generated getter |
| csharp-indexer | `class C {\n///<summary>audit</summary>\nint this[int x] => x;\n}` | one doc, text `<summary>audit</summary>` -> indexer occurrence |
| csharp-inheritdoc | `class C {\n/// <inheritdoc/>\nvoid F() {}\n}` | one doc -> F, text `<inheritdoc/>`, no expansion |
| csharp-accessor | `class C { int P { /**audit*/ get; } }` | no selected accessor doc; candidate unattached evidence; no implicit P attachment |
| php-one | `<?php\n/** one */\nclass C {}` | both profiles one -> C; no file doc |
| php-two | `<?php\n/** file */\n/** item */\nclass C {}` | both profiles file -> root, item -> C; two docs |
| php-three | `<?php\n/** a */\n/** b */\n/** c */\nclass C {}` | no file doc; selected c -> C; a/b superseded evidence |
| php-enum | `<?php\n/** audit */\nenum E { case A; }` | default audit -> E; alternate audit -> file, exclusive |
| php-define | `<?php\n/** audit */\ndefine('X', 1);` | default audit -> constant slot X with qualified provenance; alternate -> file |
| php-namespace | `<?php\n/** audit */\nnamespace N;` | one file doc audit, no namespace declaration edge under file rule |
| php-promoted | `<?php class C { function __construct(/** audit */ public int $x) {} }` | one doc audit -> promoted property/parameter slot x, not constructor |

## JS, TS and C-family controls

| Case | Source | Exact selected outcome |
| --- | --- | --- |
| js-variable | `/**audit*/\nconst f = () => 1;` | one doc audit -> binding f with declarator/arrow evidence |
| js-multi-declarator | `/**audit*/\nconst a = 1, b = 2;` | one doc -> first eligible binding a, not automatically b |
| js-export | `/**audit*/\nexport function f() {}` | one doc audit -> f, export binding wrapper retained |
| js-inner-declaration | `function outer() { /**audit*/ function inner() {} }` | one doc audit -> inner, not outer |
| js-file | `/**@file audit*/\nfunction f() {}` | one file doc `@file audit`; no f edge |
| js-virtual | `/**@name ghost\n * @function\n * audit*/` | one virtual ghost doc, text `@name ghost\n@function\naudit`; no concrete node fabricated |
| ts-ambient | `/**audit*/\ndeclare function f(): void;` | one doc audit -> signature f with ambient wrapper |
| ts-default-anonymous | `/**audit*/\nexport default function() {}` | one doc audit -> anonymous default-export declaration occurrence |
| ts-overloads | `/**one*/\nfunction f(x: string): string;\n/**two*/\nfunction f(x: number): number;` | two docs one/two -> distinct overload signature occurrences |
| ts-file | `/** @packageDocumentation */\nexport const x = 1;` | one file doc @packageDocumentation, no x edge |
| ts-parameter | `function f(/**audit*/ x: number) {}` | one doc audit -> parameter x |
| ts-destructure | `/**audit*/\nexport const {x} = value;` | candidate unattached under default; no supported declaration doc edge to x |
| doxygen-trailing | `struct S { int x; ///<audit\n};` | one doc audit -> x member slot |
| doxygen-multi | `/**audit*/\nint a, b;` | one doc audit -> both qualifying a and b slots |
| doxygen-file | `/**@file*/\nint x;` | one file doc @file, no fabricated x edge |
| doxygen-forward | `/**@fn void f()\n * audit*/\nvoid f();` | explicit signature resolves same-file f; text `@fn void f()\naudit` |
| doxygen-missing | `/**@fn void missing()\n * audit*/\nvoid f();` | text `@fn void missing()\naudit`; unresolved missing-name association, no f edge |
| doxygen-ambiguous | `/**@fn f\n * audit*/\nvoid f(int);\nvoid f(double);` | text `@fn f\naudit`; unresolved alternatives, no first-wins overload |
| doxygen-quoted | `/** text "@file" */\nvoid f();` | declaration doc -> f, text `text "@file"`; quoted @file does not reassign to file |
| objc-method | `@interface C\n/**audit*/\n- (void)f;\n@end` | one doc audit -> Objective-C method f |

JS controls run separately for Javascript and Mozjs; TS controls for Typescript
and Tsx where syntax is common. C-family controls run independently for each
grammar that accepts the spelling; ObjC-specific method case is separate.

## Go, Ruby, Perl, Bash and Python controls

| Case | Source | Exact selected outcome |
| --- | --- | --- |
| go-package | `// audit\npackage p` | one package doc audit -> package p |
| go-group | `package p\n// group\nvar (\n// member\nX int\n)` | group -> var group; member -> X spec, two documents |
| go-multi-field | `package p\ntype S struct {\n// both\nX, Y int\n}` | one doc both -> X and Y field slots |
| go-directive | `package p\n// audit\n//go:generate tool\nfunc F() {}` | one doc -> F with text audit; directive retained as non-prose evidence |
| ruby-attribute | `# audit\nattr_reader :x` | YARD one source doc -> x attribute slot |
| ruby-visibility | `class C\n# audit\nprivate def f; end\nend` | source doc -> f; private publication disposition retained |
| ruby-virtual | `# @!method ghost\n#   audit\n` | YARD virtual ghost, text `@!method ghost\n  audit`; RDoc no YARD virtual owner |
| perl-default-pod | `=head2 f\n\nAudit\n\n=cut\n\nsub f {}` | default file doc, text `=head2 f\n\nAudit`, no f edge; unique-heading option yields inferred f edge |
| perl-comment-only | `=begin comment\n\nnot prose\n\n=end comment\n\n=cut\n` | comment region, no selected prose document |
| perl-opaque | `__END__\n=head1 NAME\n\nAudit\n\n=cut\n` | file POD doc, text `=head1 NAME\n\nAudit`, source subrange inside opaque data node |
| perl-collision | `=head2 f\n\nAudit\n\n=cut\n\nsub f {}\nsub f {}` | text `=head2 f\n\nAudit`; heading inference unresolved duplicate local definitions |
| bash-function-keyword | `# @description audit\nfunction f { :; }` | one shdoc doc @description audit -> f |
| bash-file | `# @file sample\n# @description audit\necho ok` | one file doc, text `@file sample\n@description audit`; no echo owner |
| bash-unmarked | `# audit\nf() { :; }` | default no selected doc; inference option doc audit -> f |
| bash-heredoc | `cat <<'EOF'\n# @description audit\nEOF\nf() { :; }` | no documentation from heredoc text |
| python-module | `"audit"\nx = 1` | primary module doc audit |
| python-class | `class C:\n    "audit"\n    pass` | primary doc audit -> C |
| python-attribute | `x = 1\n"audit"` | attribute doc audit -> binding x |
| python-chained | `x = y = 1\n"audit"` | one attribute-binding doc -> x only under Sphinx's first-target rule; no y association |
| python-chained-comment | `#: audit\nx = y = 1` | one Sphinx comment doc -> x and y under the comment path's all-target rule |
| python-nondoc | `x = 1\nprint(x)\n"audit"` | discarded string, no selected primary/additional/attribute document |
| python-prefix | `def f():\n    b"audit"\n    pass` | bytes statement not primary doc; same exclusion for f/t strings |

## PHP callee and placement matrix

Use `<?php\n$x = 0;\n/** audit */\n` plus the statement to isolate source
declaration attachment from the first-file rule. Both BCA profiles retain the
stated callee result below, including the explicit static-profile limitations;
this matrix does not claim exact phpDocumentor name-resolution parity.
A constant name expression is only resolved when it is a static supported
string; no evaluator is added.

| Statement / case | Source association | Text and publication |
| --- | --- | --- |
| `define('X', 1);` | constant source slot X | audit; runtime existence unknown |
| `\\define('X', 1);` | constant source slot X | audit; runtime existence unknown |
| `((define('X', 1)));` | same X slot, grouping wrapper retained | audit |
| `DEFINE('X', 1);` | no declared constant under case-sensitive phpDocumentor profile; unattached doc evidence | audit still available through evidence; no publication assertion |
| `Vendor\\define('X', 1);` | no supported builtin constant association; unattached evidence | audit |
| `namespace\\define('X', 1);` | initial static-profile limitation: no builtin association, unattached evidence | audit; phpDocumentor resolves this global-namespace spelling to builtin define |
| `define($name, 1);` | unresolved constant owner name, call/argument source spans retained | audit; dynamic name |
| import `use function Vendor\\make as define;` then documented `define('X', 1);` | unresolved imported-callee association, not fabricated builtin X | audit |
| import `use function define as make;` then documented `make('X', 1);` | unresolved imported-callee association in initial static profile | audit |
| inside `function f() { /** audit */ define('X', 1); }` | qualified source constant slot X, lexical scope f retained | audit; publication/runtime execution unknown; legacy DLOC zero for the block |

The relative-name exclusion and imported-callee uncertainty are initial profile
limitations. The legacy row may classify the written spelling differently and stays
independently characterized. With no preceding neutral statement, the file
selector can consume a doc block before an unsupported call; that is a file
association, not evidence the callee became a documentation target.

## Elixir effective metadata filtering

| Body inside M | Exact effective result |
| --- | --- |
| `@doc defaults: 99\ndef f(), do: :ok` | metadata-only raw occurrence retained; user-supplied reserved defaults omitted from effective metadata |
| `@doc opaque: true\ndef f(), do: :ok` | raw reserved-key occurrence retained; no effective opaque override |
| `@doc since: "1"\n@doc since: "2"\ndef f(), do: :ok` | effective since=2, source occurrences retained in order |
| `@doc since: 1\ndef f(), do: :ok` | invalid metadata evidence with no valid effective since value; no fabricated complete metadata |

Effective metadata here means only the compiler-recognized supported keys
needed for provenance/eligibility, not a promised general annotation map.
Unknown arbitrary keys remain source metadata without evaluation. Compiler
generated defaults/opaque values are not synthesized from invalid overrides.
