# Source-documentation extraction contract

This is the contributor implementation contract for declaration-to-documentation
queries in `big-code-analysis-ast` and the root library's `Ast`. It specifies what
the documentation index must preserve, how it differs from documentation line
counting, and which outcomes its tests must establish. It assumes familiarity
with the project's [grammar dispatch](grammar_dispatch.md) and
[tree-sitter](https://tree-sitter.github.io/tree-sitter/) syntax trees.

The direct-AST collector and a hidden root `Ast::documentation` method are available;
adapter support remains partial. This contract describes required outcomes, not a
claim that every form is implemented. The current stable public surface is listed in
[STABILITY.md](../../STABILITY.md). Track per-adapter status and review evidence in
[#1654](https://github.com/dekobon/big-code-analysis/issues/1654), not in this contract.

## Recognition serves two consumers {#recognition}

A documentation line count answers which physical source rows the legacy
convention bills as documentation. Extraction answers which source fragments
form a document and which declaration or other owner that document describes.
An owner is the source entity being documented. It can differ from the metric
space containing the comment.

Documentation lines of code (DLOC) are part of the
[lines-of-code (LOC) metrics](../../big-code-analysis-book/src/metrics.md#lines-of-code).
They also affect the documentation-aware
[maintainability index (MI)](../../big-code-analysis-book/src/metrics.md#maintainability-index-mi).

```text
AST role/context recognition and shared binding
    +-- legacy candidate view -> exact streaming reading -> root LOC/MI
    `-- source candidate view -> documents/associations -> local text/mappings
```

The source and legacy views may select different candidates before applying
the same binding algorithm. For example, Java source attachment recognizes
`/**/`, while the current legacy marker excludes it. An older block followed
by `/**/` selects the newer block for extraction and the older block for DLOC.
Filtering source-selected documents afterwards cannot recover the old reading.

Keep these decisions separate:

| Decision | Question |
| --- | --- |
| Recognition | Does the syntax contain a documentation candidate? |
| Profile selection | Which candidates does this named convention retain? |
| Source attachment | Which source owner does the selected document describe? |
| Publication eligibility | Would the named tool/configuration include that owner? |
| Legacy reading | Which original rows does the existing metric path bill, and when? |

Use the existing documentation roles and their named context functions in
[`roles/mod.rs`](../../big-code-analysis-ast/src/roles/mod.rs). `Comment` and
`DocComment` partition comment kinds; `is_comment` reads their kind union and
`is_doc_comment` applies documentation gates. A `Docstring` is a positional
string, not a comment. `DocTarget` is a candidate, not a resolved owner.
`DocBodyResidual` supplements callable/closure recognition and the language's
body-child test; it is not the complete body predicate.

Attributes recognized by extraction do not automatically enter the legacy
`DocComment` or `Docstring` rows. This applies to Rust `#[doc = ...]` and Elixir
`@doc`, `@moduledoc` and `@typedoc`. Do not put metric flags on public facts.
The AST supplies the exact legacy reading through internal intercrate plumbing;
the root retains physical row sets, metric-space attribution and aggregation.

### Streaming and evidence requests

The legacy-only path does not construct a documentation index or render text.
A private request or sink controls additional source evidence:

| Existing situation | Legacy-only work | Source-evidence request |
| --- | --- | --- |
| `StretchEnd::Undecided` | Defer until the final member | Also defer; never scan every prefix |
| Rejected placement or `Elsewhere` without a gatherer | Keep the current early exit | Inspect only when retaining rejected/unbound evidence was requested |
| Existing in-body documentation | Preserve the current reading | Resolve source attachment separately |
| Doxygen gathering | Preserve gatherer and coverage order | Retain contributing occurrences once |
| Python additional-string run | Preserve one backward run scan | Retain primary/additional owners and fragments in that scan |
| Logical continuation rows | Retain the root's existing `BareRows` prepass | Reuse only structural source facts, not metric policy |

Count additional-evidence materialization in tests. Metrics-only requests must
perform zero new materialization. Preserve all existing row corrections and
emission timing, including Python parentheses and Rust comment newline handling.
Do not count rows from cleaned text or semantic owner spans.

## The source-bound index {#index}

Collection borrows the language, tree and parsed source with explicit immutable
context and options. It returns a caller-owned immutable index. Typed
`Parser<T>`/`AnyParser` convenience is the preferred direct-AST entry point and
checks retained parse completion provenance.

The direct AST crate also exposes collection over borrowed language/tree/source.
Its preconditions are a matching grammar, tree and buffer, and a complete parse.
This permits early benchmark and fuzz callers to reuse `Ast`'s existing tree
and source accessors. No reparse, clone-and-adopt workaround or early root API
addition is required. The later stable `Ast::documentation` delegates through
its typed parser. `Ast` stores no persistent or self-referential documentation
cache.

### Fragments, documents and associations

| Record | Identity and contents |
| --- | --- |
| Physical fragment | One source occurrence; ordered included byte ranges; optional exact syntax node |
| Logical document | Ordered fragment composition, role and text state; shared by its associations |
| Association | One document, its convention/provenance, binding wrapper and resolved owners or unresolved candidates |
| Owner | Exact node, location-backed slot, file/root, logical declaration group or virtual target |

A line-comment fragment excludes its terminating LF/CRLF, even when the grammar
includes that terminator in the optional syntax node. Its range need not equal
that node's range. Block comments and literal fragments include their outer
delimiters; attribute-form fragments include the entire attribute spelling,
not just its value literal. A Tcl preamble fragment is its value word, including
braces or quotes. Included-range intersection applies to each of these spellings.

An occurrence's identity is not its text. Equal strings at different offsets
remain different fragments. A document describing several qualifying fields
is stored once; one resolved association lists them. A conditional declaration
with alternative branches has unresolved alternatives, not several claims that
all branches are active.

Preserve the lexical containing scope and binding wrapper separately from
normalized owners. A slot contains its enclosing owner, role, ordinal and source
span. It does not pretend to be a tree-sitter node. Recovered declarations can
retain several ordered source pieces; insufficient evidence yields an unresolved
candidate, not a fabricated function inferred from a declarator's shape.

Type-leading documentation belongs to the type even when metrics synthesize a
primary-constructor or record-header function space. Explicit Java and compact
constructors, Kotlin secondary constructors and other declared-constructor targets
remain owners. A parameter-list node can contain documentation without owning it;
the profile selects the declaration or parameter/property slot. A parameter-local
document belongs to its parameter slot; `@param` text in a type document stays
markup rather than creating another document.

### Outcomes

| Situation | Required outcome |
| --- | --- |
| Supported owner with no selected documentation | Successful empty owner query |
| Unsupported owner kind | Unsupported-owner outcome, distinguishable from an empty query |
| Foreign tree or noncanonical node | Distinguishable identity errors |
| Handle from another index | Foreign-index error, or API shape preventing transfer |
| Parser feature disabled | Language-disabled error |
| Requested convention not supported for this language | Unsupported-profile error |
| Valid empty prose | Present document, complete text `""`, empty mapping |
| Explicitly disabled documentation | Retained disabled state; not empty prose |
| Explicitly absent value such as Elixir `nil` | Retained explicit-absence state |
| Metadata-only attribute | Retained metadata-only state; no prose value |
| Dynamic, undecodable or incomplete local content | Retained raw fragments and explicit text-unavailable reason |
| Missing, duplicate or conditional named target | Unresolved association with reason and available candidates |
| Ccomment or Preproc | `NoDeclarationModel`; empty documents/associations; declaration lookup unsupported |

Default enumeration retains selected documents, including disabled/metadata
states. An explicit evidence option also exposes superseded and dangling
candidates; ordinary owner queries do not return them as attached documents.

### Identity and ordering

Prefer opaque references borrowing their index. Node convenience checks the
held tree and an indexed canonical representation of the node. The pinned
tree-sitter binding provides safe `Node::into_raw()`: compare its tree pointer
without dereferencing it. Also compare the canonical context, node identity,
kind and spans recorded during collection. Node ID or byte range alone is not
provenance. The returned index, handles and result types retain `Send + Sync`.
Keep safe borrowed tree/node values rather than a stored raw pointer that
changes those auto-traits.

Reject nodes from another parse, cloned or incrementally edited trees, shifted
roots, and individually edited node values. If two indices borrow one tree,
their handles remain distinct. Moving an index before obtaining its borrowed
references is valid. No identifier is promised to persist across indices.

Document order is `(first_fragment.start, first_fragment.end, role_rank,
creation_ordinal)`. Creation ordinals are unique within the index and assigned
by deterministic root-down/source-order collection. Role ranks are explicit
contract values, not incidental enum discriminants. Associations follow document
order, then owner source anchor and slot ordinal. Fragments retain composition
order. File and virtual documents use their own first fragment as anchor.
Hash iteration and pointer order never reach output. Initial role order is
primary, additional, attribute-binding, Sphinx, leading comment, body comment,
file/package, virtual, then inferred. Here attribute-binding means a Python
PEP 258 string describing a binding. Rust `#[doc]` and Elixir `@doc` are syntax
forms, not that role: item documentation uses primary, module documentation
uses file/package. Mixed Rust comments/attributes retain primary. A metadata-only
attribute keeps its owner's documentation role. Future roles append; content
states such as disabled text do not change an existing role's rank.

Every document has a physical occurrence. Empty prose still has nonempty raw
syntax, such as `/*!*/` or `""`; do not invent zero-width text fragments.

## Profiles and source forms {#profiles}

An index selects one attachment profile for its language. A BCA composition
states precedence explicitly. Private or nested source-attached docs remain
discoverable even when a documentation tool omits them. Publication means known
eligibility under the specified configuration, or unknown; it is not a claim
that a generator was run.

| Language | Default and required alternatives | Attachment scope |
| --- | --- | --- |
| Rust | [Compiler documentation attributes](https://doc.rust-lang.org/reference/comments.html#doc-comments) | Outer/inner comments and literal attributes; private/nested items remain source-visible |
| Python | [PEP 257](https://peps.python.org/pep-0257/), [PEP 258](https://peps.python.org/pep-0258/) and named [Sphinx attribute comments](https://www.sphinx-doc.org/en/master/usage/extensions/autodoc.html#directive-autoattribute) | Primary, additional, attribute and Sphinx roles remain distinct |
| Java | [JDK 25 source attachment](https://docs.oracle.com/en/java/javase/25/docs/specs/javadoc/doc-comment-spec.html), BCA local cleanup | Traditional and Markdown docs; explicit logical filename for filename-dependent eligibility |
| Kotlin | [KDoc](https://kotlinlang.org/docs/kotlin-doc.html) | Declarations and supported parameter slots through annotation wrappers |
| Groovy | [Groovydoc](https://groovy-lang.org/groovydoc.html) | Types, members, constructors and current grammar shapes |
| Csharp | [C# XML documentation](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/xmldoc/) | Documented declarations through leading trivia, attributes and directives |
| Php | `BcaPhpSource`; required selectable `PhpDocumentor` | Declaration-first default; file-first alternate, detailed below |
| Javascript, Mozjs | [JSDoc](https://jsdoc.app/) | Declarations, supported assignments/members/exports, file and virtual docs |
| Typescript, Tsx | `BcaTypescriptSource`: [TypeScript](https://www.typescriptlang.org/docs/handbook/jsdoc-supported-types.html) attachment and [TypeDoc](https://typedoc.org/documents/Doc_Comments.html) structural file rules | File structural blocks take precedence over tsc declaration attachment; source overloads stay distinct |
| C, Cpp, Mozcpp, Objc | [Doxygen](https://www.doxygen.nl/manual/docblocks.html) | Leading, trailing, in-body, signature, gathered and bounded structural-name forms |
| Go | [go/doc](https://pkg.go.dev/go/doc) source groups | Package, declaration group, spec, field and interface docs; directive evidence separate |
| Ruby | [YARD](https://yardoc.org/); required selectable [RDoc](https://ruby.github.io/rdoc/) | Separate scope, virtual and visibility rules; no union of incompatible results |
| Lua | [LDoc](https://lunarmodules.github.io/ldoc/) | Marked runs/long comments, module, supported assignment/function/table targets |
| Perl | [POD](https://perldoc.perl.org/perlpod), optional unique-heading inference | File/package docs by default; unique same-file subroutine association only when selected |
| Elixir | [Compiler source documentation](https://hexdocs.pm/elixir/writing-documentation.html) | Module/function/macro/type/callback attributes and explicit heads/clauses |
| Bash | [shdoc](https://github.com/reconquest/shdoc) | File/function marked comments; generic adjacency opt-in |
| Tcl | [Static Ruff! 3.0.4](https://ruff.magicsplat.com/v3.0/ruff-ruff.html) | Procedure/method/constructor body docs and literal namespace preambles |
| Irules | BCA leading/body convention | Separate same-scope leading and initial body docs for each proc/when occurrence |
| Ccomment, Preproc | Helper capability | No declaration model |

### Conflicting conventions and static ownership

For PHP, the default first selects
[Zend/reflection declaration documentation](https://www.php.net/manual/en/reflectionfunctionabstract.getdoccomment.php).
It then considers phpDocumentor-qualified `define` forms and file docs, with
declaration-before-file precedence. The file selector reads the original
candidate sequence, not a sequence with consumed declaration blocks removed.
An already-consumed block cannot receive a second file association.

The file rule is the pinned
[phpDocumentor Reflection 7.0.0 selector](https://github.com/phpDocumentor/Reflection/blob/70254fa0b3058b16908a3a9d95f7dca904e03396/src/phpDocumentor/Reflection/Php/Factory/File.php).
Before class/function/constant/interface/trait, exactly two blocks permit the
first to be file documentation. Other first-node kinds use the first block.
There is no `@package` or license-tag override. The alternate selects file docs
first, then remaining declaration docs using
[PHP-Parser](https://github.com/nikic/PHP-Parser) attachment. This
one-owner precedence is explicit even where an upstream implementation retains
duplicate associations.

Thus one block before a class documents the class in both profiles; two blocks
document the file then class; three do not satisfy the exactly-two file rule.
A block before an enum or supported `define` is the conflict control: the
default associates the declaration, the alternate the file. Native attributes,
promoted properties and signature-local comments retain their own source owner
evidence. An imported callee name is not proof that a call is builtin `define`.

YARD and RDoc both read `=begin`/`=end` blocks. YARD handles `@!` virtual
directives and its own scope/call rules; RDoc has separate `##` and visibility
directive rules. Hidden source documents retain the appropriate publication
state. An explicit
virtual method does not become a concrete declaration merely because a same-name
method exists later.

Python attribute strings follow the Sphinx target subset where it extends
PEP 258: a string after `x = y = 1` describes x only, while Sphinx `#:` comments
can describe both assignment targets. A tuple within the first assignment target
still enumerates its qualifying leaves. Do not turn the legacy existence test
into an association for every syntactic target.

Elixir documentation groups use enclosing module, definition kind, name and
source-written arity. Preserve explicit heads and clauses in the group. Default
arguments do not create extra synthetic arity owners. `@spec` is signature
metadata; `@typedoc` associates types and `@doc` can describe supported callbacks.
Private source docs remain inspectable even where the compiler discards them.
`false`, `nil`, empty strings and keyword-only metadata are distinct states.
Repeated attributes follow the inspected
[Elixir 1.20.1 module implementation](https://github.com/elixir-lang/elixir/blob/v1.20.1/lib/elixir/lib/module.ex):
within an attribute run, the last non-keyword value replaces the prior value;
keyword attributes merge metadata and later keys replace earlier keys. Thus
`@doc false; @doc "text"` selects text, while the reversed order disables it.
`false` is not sticky. Across explicit clauses of one owner group, a new nil
value leaves the current effective value unchanged; a new non-nil value replaces
it. Retain superseded value occurrences as evidence, not extra selected prose.
Explicit nil evidence remains distinguishable even where group composition
retains an earlier effective value.

Separate raw metadata occurrences from effective metadata. Elixir filters
user-supplied `opaque` and `defaults` keys before merging; retain those raw
occurrences without applying their values. Validate supported built-in keys
such as string-valued `since` and `deprecated`; invalid values retain explicit
invalid-metadata evidence rather than a valid effective value. Arbitrary keys
remain source metadata without evaluation. This is not a general annotation map.

With no `@doc` occurrence, `@impl true` supplies owner evidence and ineligible
publication, not a fabricated documentation occurrence. For a documented group,
the compiler's `@impl` fallback disables a nil effective value; it does not
override explicit string or false values. Retain an explicit nil occurrence
even when that fallback disables the effective value. `@deprecated "reason"`
is separate owner evidence and contributes `deprecated` to an existing
document's effective metadata; it does not create prose. Explicit documentation
metadata overrides the same compiler-provided key, following `compile_doc_meta`.

Tcl's first body comment block needs no `#ruff` marker. Leading blank source
lines are allowed; a blank source line or code ends the block. Later `#ruff`
blocks append fragments to that procedure/method document. A bare `#ruff` line
contributes a blank line; text following it contributes that text. Preserve
`#ruffopt` as local source evidence without formatter-dependent filtering.
Literal `_ruff_preamble` describes its namespace. Dynamic preambles and runtime
`_ruffClassHook` evaluation have explicit unavailable content.

Static TclOO method, constructor and destructor owners are commands named
`method`, `constructor` or `destructor` directly in a literal
`oo::class create NAME { ... }` or `oo::define NAME { ... }` body. Recognize
them through named context over the `Call` row. They do not acquire metric
function spaces. Dynamic class names/bodies do not yield guessed concrete owners.
For literal `variable _ruff_preamble ...` inside `namespace eval NAME { ... }`,
the owner is that enclosing namespace occurrence. A qualified top-level
`set ::ns::_ruff_preamble ...` is an unresolved name-keyed candidate initially.

For iRules, leading comment siblings occupy consecutive rows and end directly
above the proc/when's first row. Initial body docs use the same blank-line
boundary as Tcl's lead block. Leading and body docs remain two documents when
both exist. Priority/modifiers do not change owner or body selection.

Generic leading-comment inference is opt-in only for Bash, Tcl, Lua and Perl.
A contiguous ordinary run must immediately precede a statically named supported
same-scope declaration, with no blank source row, command or excluded-range gap.
It cannot override native attachment and carries BCA-inferred provenance.
This does not suppress native unmarked Ruff, Go or YARD documentation.

## Local text and source mappings {#text}

Raw syntax, decoded static value and cleaned local content are separate views.
Raw included bytes remain available when decoding fails. Complete local text is
a UTF-8 string plus forward mappings, or an explicit unavailable result. It
contains local markup and reference directives, including XML, Markdown, POD,
`inheritdoc`, `copydoc` and `include`; no generator, inheritance or macro
expansion runs.

An optional segmented view contains known text and explicit opaque gaps. It
cannot silently join known prefixes/suffixes across a dynamic expression into
an apparent annotation. A pure implicit Python literal concatenation has no
separator; separate additional strings remain separate logical documents.

### Decoding and cleanup boundaries

- Python supports ordinary/raw static strings, standard control/punctuation,
  octal, hex and Unicode-scalar escapes, and escaped line endings. Unknown
  escapes preserve the backslash according to the pinned Python semantics.
  Named `\N{...}` escapes are explicitly unsupported in this delivery; lone
  surrogates and out-of-range scalars are unrepresentable. `f`, `b` and `t`
  prefixed strings are not docstrings. Whole-document cleanup follows
  Python 3.12 [`inspect.cleandoc`](https://docs.python.org/3.12/library/inspect.html#inspect.cleandoc):
  expand tabs, remove first-line indentation and common later-line margin,
  then remove surrounding empty lines. Preserve trailing content spaces.
- Rust supports static cooked/raw doc attribute strings and documentation
  comments. Mixed cleanup follows the pinned Rust 1.99.0
  [`beautify_doc_string`](https://github.com/rust-lang/rust/blob/1.99.0/compiler/rustc_ast/src/util/comments.rs#L37),
  [`unindent_doc_fragments` and `add_doc_fragment`](https://github.com/rust-lang/rust/blob/1.99.0/compiler/rustc_resolve/src/rustdoc.rs#L107-L198),
  then removes the final assembly newline once, as
  [`opt_doc_value`](https://github.com/rust-lang/rust/blob/1.99.0/src/librustdoc/clean/types.rs#L1130-L1137) does.
  This is not universal trimming or tab expansion. Computed values and
  `include_str!` retain source evidence with unavailable local content.
- Java source tokens containing raw backslash-u have explicit unsupported
  semantic decoding. Preserve separately labeled source-cleaned spelling.
  The trigger deliberately over-approximates JLS eligibility, including an
  ineligible spelling whose Unicode-escape backslash has an odd number of
  preceding backslashes, such as two backslashes followed by `u0041`.
  [JLS Unicode prelexing](https://docs.oracle.com/javase/specs/jls/se25/html/jls-3.html#jls-3.3)
  can also alter delimiters or line endings; affected associations remain
  uncertain even when tree-sitter reports no ERROR node. Ordinary docs remain
  supported. For `/**/`, BCA delimiter-cleaned text is empty by design; observed
  JDK 25.0.5 `DocTrees` returns `/`. Java `////` is different: removing `///`
  leaves the real content `/`, which BCA retains.
- Elixir supports ordinary strings, heredocs, and `~s`/`~S` string sigils with
  static delimiters and no modifiers. Decode cooked escapes and language-defined
  heredoc indentation. Raw `~S` preserves interpolation-looking bytes and escapes
  except delimiter syntax. Cooked interpolation, computed expressions,
  charlist/non-string values, other sigils and modifiers have unavailable content.
  Keep the raw attribute and explicit reason; do not evaluate expressions.
- Comment adapters remove only their specified delimiter/line prefixes and
  structural margins. A profile's exact LF/CRLF, blank-line, trailing-space and
  join behavior belongs in its fixtures. Keep Markdown hard-break spaces and
  code indentation. General annotation interpretation is consumer work.

The local comment-cleanup policy is a BCA source view, not formatted generator
output. Freeze these transformations separately from attachment:

| Forms | Local cleanup |
| --- | --- |
| Java Markdown | Remove each `///` and the minimum leading space/tab margin of nonblank lines across the selected run; preserve trailing spaces and remaining indentation |
| C# XML lines, Doxygen lines, Go, Ruby, Bash, Tcl and iRules hash/line forms | Remove the exact recognized marker and at most one following ASCII space from each member; preserve the remaining whitespace |
| LDoc lines | Remove the recognized `---` opener; continuation `--` lines lose their own marker; remove at most one following ASCII space |
| Python Sphinx comments | Remove `#:` and at most one following ASCII space; do not apply string-literal decoding |
| Go block comments | Remove `/*` and `*/` and at most one immediate interior ASCII space on either boundary; keep internal indentation and stars literally |
| Ruby YARD/RDoc blocks | Remove the `=begin` opener line and `=end` terminator line; retain interior text and indentation |
| Java traditional, KDoc, Groovydoc, PHPDoc, JSDoc, TypeScript, Doxygen and C# block forms | Remove the outer recognized delimiters; remove one immediate ASCII space inside either delimiter on its boundary line; on later lines remove indentation plus one decoration `*` and at most one following space when present |
| Lua long documentation comments | Remove the matching long-comment opener/closer, including the matching equals depth; no star decoration rule |
| Perl POD | Preserve POD markup and internal spacing; omit the terminal `=cut` line and surrounding empty boundary lines; comment-only POD has no prose |

For comment forms in this table, normalize CRLF to LF with the complete CRLF
token as its source origin, discard only empty boundary lines, and insert one
synthetic LF between selected physical line-comment members or separately
gathered blocks. Keep interior blank comment lines. Do not append a final LF
just because the source file has one. Block-internal LF bytes retain copied
origins; joins across removed delimiters are synthetic. This differs deliberately
from Go's `CommentGroup.Text` final newline and renderer-specific whitespace.
Rust/Python/Elixir literals and Tcl preamble values keep their separately
specified language cleanup. Sphinx, Go block and Ruby block forms follow the
table's CRLF, empty-boundary-line and join rules.

Examples: `/** audit */` becomes `audit`; `/**\n * a  \n *   b\n */` becomes
`a  \n  b`; `/// a  \n///   b` in Java becomes `a  \n  b`; `# a\n#\n# b` in
an attached hash-comment run becomes `a\n\nb`. A source blank line between
separate runs still participates in attachment rules; cleanup never joins runs
that the profile did not select together.

Tcl preambles accept static braced or quoted words under the
[Tcl 8.6 substitution rules](https://www.tcl-lang.org/man/tcl8.6/TclCmd/Tcl.htm).
Braced words preserve ordinary escapes, dollar signs and brackets. Quoted words
decode Tcl backslash escapes; variable or command substitution makes complete
text unavailable. Non-scalar decoded values are unavailable. Backslash-newline
and following indentation decode to one space only where the supplied parse
has reliable boundaries; continuation-affected parses retain the #1802 limitation.
Never execute Tcl to obtain a value.

The pinned Tcl grammar can misparse a braced preamble when its `#` content and
closing brace share a source line. That form retains explicit grammar-boundary
uncertainty and cannot produce a confirmed namespace association or complete
preamble text. Multiline controls put the closing brace on its own line so the
parser supplies the namespace and value word. This is a grammar limitation, not
a Tcl string-substitution rule.

After value decoding, use Ruff's namespace preamble margin: take the leading
space/tab prefix of the first nonblank content line and remove that prefix from
each line that has it. Remove empty boundary lines, preserve internal whitespace
and `#` headings, and normalize CRLF to LF. Do not use the hash-comment cleaner
or append a terminal newline. In literal Tcl source notation, quoted
`"# Heading\n  detail"` produces a line break, whereas braced
`{# Heading\n  detail}` retains the backslash and `n`.
Both preserve the heading marker. Decoded escapes map to complete source tokens;
unchanged braced content has copied origins.

Required Rust cooked escapes are escaped quote/backslash, newline/carriage
return/tab/NUL, valid ASCII hex and Unicode-scalar escapes, and Rust's escaped
line-continuation removal. Raw strings preserve interior bytes. Invalid escape
spelling, non-scalar values and non-UTF-8 source make complete content unavailable.
Nested block-comment delimiters inside an outer doc block remain content; only
the outer documentation delimiters are removed.

### Forward mapping rules

Mappings partition the output UTF-8 byte range into ordered runs:

| Origin | Query behavior |
| --- | --- |
| Copied source | Clip source bytes exactly with the requested output subrange |
| Decoded token | Return the complete responsible source token for any intersecting output scalar |
| Expanded token | Return the complete token, such as a tab expanded into several spaces |
| Synthetic | Retain the output portion with no source range, such as an inserted fragment separator |

A query returns ordered output subranges with origin information. Disjoint
origins stay disjoint; never use an envelope containing intervening code or
comments. Removed prefixes, delimiters and continuations emit no mapping run.
If a token itself crosses included ranges, preserve its included pieces rather
than return an envelope spanning excluded bytes.

Reject reversed/out-of-bounds ranges and endpoints inside a UTF-8 scalar.
A valid empty range returns an empty origin sequence, including at EOF and for
empty text. Synthetic-only queries return synthetic output, not an empty answer.
No reverse source-to-text mapping is promised.

## Parse context and cancellation {#parse-context}

Offsets refer to the actual parsed buffer, including file-loading normalization
and C-family masking. `Ast::source()` is that buffer. Masking preserves comment
bytes but does not justify recovering an original identifier from dollar runs.

Logical filename is explicit caller metadata. `Ast` holds a private
path-provenance field populated by `Ast::from_path`: the final component of the
path. The hidden `Ast::documentation` supplies it when the context names no
logical filename; an explicit one always wins.
`Source::with_name` and adopted-tree display labels remain display labels. A
logical filename is context for documentation rules, not a filesystem credential.

Read included ranges from the supplied tree. Excluded gaps are positional
attachment barriers. A node envelope may contain excluded bytes; intersect it
with the included ranges before returning fragments. Required gap-spanning
content has unavailable text. Preserve unrelated valid docs and do not reparse
the source to manufacture a continuous tree.

Retain actual parse completion provenance at parse time in internal parser
state. The new typed documentation query rejects a cancelled parse even after
the cancellation flag's scope ends. Existing metric behavior remains unchanged.
Adopted parses carry the caller's completeness precondition, not fabricated
internally verified provenance. Empty roots and restricted parses do not imply
cancellation.

The raw-triple seam cannot observe past cancellation. Supplying a cancelled
parse through raw accessors violates its precondition. Ordinary early benchmark
and fuzz paths do not install `ParseCancel`; cancellation probes use the typed
parser entry point. Active collection cancellation applies to both entry points.

Collection, named finalization and text construction observe cancellation at
bounded work intervals, including within a large single fragment. Return an
explicit cancelled outcome rather than a partial successful index/text result.
Tests cover pre-cancelled parses, mid-work cancellation, nested flag restoration,
unwinding, valid empty/restricted parses and the next successful parse.

## Traversal and implementation boundaries {#traversal}

Primary visits use `Ancestors::for_walk(chain, node, &folds, &siblings)` with
the walk's sibling records. Use the without-siblings constructor only when no
sibling query can occur. A derived secondary walk maintains one borrowed
`ChildAncestors` holder through descent/ascent rather than copying derived tails.
That holder drops sibling-cache access: broad secondary sibling queries need a
recorded batch or a justified cursor, not repeated uncached seeks.
`nearest_level_answer` requires complete ancestry. Preserve source bytes and
the current typed gate signatures through all callbacks.

The collector performs one coordinated structural pass, permitting existing
amortized sibling scans and indexed finalization. Source/legacy/profile queries
use different keys or caches when their questions can disagree. Drop build-only
`RefCell` caches before returning an index. Query validation uses indexed
canonical context, never `Node::parent` on deep zero-width nodes.

Same-file structural name references finalize through adapter-defined
scope/kind/name/signature indices. Missing or duplicate names stay unresolved.
No all-pairs matching, cross-file symbol resolver or macro evaluator is required.
Build fragment text once per document, not once per association.

State costs in source bytes/nodes, fragments, associations, rendered bytes and
map segments. Separate sorting cost, field lookup and cached-state search from
the structural walk. Cold collection, prepared-index lookup, text/map creation
and retained-result reads have separate probes. Exact counters measure both
state computation and slot search. Deep construction, owner extraction, assembly
and teardown are iterative and tested on a small stack.

The following bounds exclude parsing. Let N/B be input nodes/bytes, F physical
fragments, E associations, R included ranges, T rendered bytes, S map segments,
I emitted source intersections, and K query-result records or pieces.

| Operation | Required bound |
| --- | --- |
| Collection before sorting | Amortized `O(N + B + F + E + R + I)` structural work; indexed intersections may add `O(F log(R + 1))` instead of a monotone sweep |
| Prepared owner/node query | Expected `O(1 + K)` indexed lookup, or `O(log(N + F + E + 1) + K)` ordered lookup; no full association/file rescan |
| Mapping range query | `O(log(S + 1) + K)` including returned source pieces; valid empty-range handling is `O(1)` |
| Text/map construction | Linear in contributing input bytes, T, S and I; no repeated copying of a growing prefix |
| Retained text/map access | Borrow a handle or slice in `O(1)` without reconstruction |
| Retained storage | Borrow source bytes; index `O(N + F + E + R + I)`; rendered T/S storage once per document, not multiplied by owner fanout |

State index-building and deterministic sorting terms separately, including name
bytes compared and queued references. A full included-range scan per fragment
is not permitted. Probe small queries near the last owner/map segment, many
included ranges, a many-fragment document, and independently growing text size
and owner fanout. Count index comparisons, range intersections and copied bytes
where output cannot reveal a quadratic regression. Retained-result access must
not silently rebuild a document on every call.

The Tcl/iRules continuation limitation in
[#1802](https://github.com/dekobon/big-code-analysis/issues/1802) follows its
approved upstream grammar repair and adoption. Until then, mark affected
comment boundaries and apparent proc/when targets uncertain. A bounded source
check can identify uncertainty; it cannot become a replacement command parser.
Ordinary unrelated docs remain available.

The Rust attribute-value pruning behavior in
[#1786](https://github.com/dekobon/big-code-analysis/issues/1786) is a documented
retained limitation. Do not treat its closed issue as a code fix or erase the
corrected outer-attribute pruning baseline.

## Worked acceptance cases {#acceptance-cases}

These are required extraction outcomes, not results from an implementation.
In escaped source examples, `\n`, `\r`, `\t` and `\\` denote LF, CR, tab and
one literal backslash. Other characters are literal UTF-8. Each primary source
cell ends exactly where shown, without a final newline. Ranges are half-open
bytes. The primary roster has one document and one resolved declaration
association per row; Perl selects its explicit unique-heading profile.
`audit` is arbitrary prose, not a structural tag. The source spelling
independently fixes the ranges and content.

| LANG | Exact source | Fragment range | Owner and range | Local text |
| --- | --- | --- | --- | --- |
| Rust | `///audit\nfn f() {}` | `0..8` | f, `9..18` | `audit` |
| Python | `def f():\n    "audit"\n    pass` | `13..20` | f, `0..29`, primary role | `audit` |
| Java | `///audit\nclass C {}` | `0..8` | C, `9..19` | `audit` |
| Kotlin | `/**audit*/\nfun f() {}` | `0..10` | f, `11..21` | `audit` |
| Groovy | `/**audit*/\nclass C {}` | `0..10` | C, `11..21` | `audit` |
| Csharp | `///<summary>audit</summary>\nclass C {}` | `0..27` | C, `28..38` | `<summary>audit</summary>` |
| Php | `<?php\n/** audit */\nfunction f() {}` | `6..18` | f, `19..34` | `audit` |
| Javascript | `/**audit*/\nfunction f() {}` | `0..10` | f, `11..26` | `audit` |
| Mozjs | `/**audit*/\nfunction f() {}` | `0..10` | f, `11..26` | `audit` |
| Typescript | `/**audit*/\nfunction f(): void {}` | `0..10` | f, `11..32` | `audit` |
| Tsx | `/**audit*/\nfunction f() { return <div/>; }` | `0..10` | f, `11..42` | `audit` |
| C | `/**audit*/\nvoid f(void) {}` | `0..10` | f, `11..26` | `audit` |
| Cpp | `/**audit*/\nvoid f() {}` | `0..10` | f, `11..22` | `audit` |
| Mozcpp | `/**audit*/\nvoid f() {}` | `0..10` | f, `11..22` | `audit` |
| Objc | `/**audit*/\nvoid f(void) {}` | `0..10` | f, `11..26` | `audit` |
| Go | `package p\n// audit\nfunc F() {}` | `10..18` | F, `19..30` | `audit` |
| Ruby | `# audit\ndef f\nend` | `0..7` | f, `8..17` | `audit` |
| Lua | `module("m")\n---audit\nfunction f() end` | `12..20` | f, `21..37` | `audit` |
| Perl | `=head2 f\n\nAudit\n\n=cut\n\nsub f {}` | `0..21` | f, `23..31`, inferred role | `=head2 f\n\nAudit` |
| Elixir | `defmodule M do\n  @doc "audit"\n  def f(), do: :ok\nend` | `17..29` | M/def/f/0, `32..48` | `audit` |
| Bash | `# @description audit\nf() { :; }` | `0..20` | f, `21..31` | `@description audit` |
| Tcl | `proc f {} {\n# audit\nreturn\n}` | `12..19` | f, `0..28` | `audit` |
| Irules | `# audit\nwhen HTTP_REQUEST {\n}` | `0..7` | HTTP_REQUEST occurrence, `8..29` | `audit` |

For Ccomment input `/**audit*/` and Preproc input `#define X 1\n`, collection
reports `NoDeclarationModel` and no documents/associations. A disabled feature
instead returns language-disabled before collection. Neither helper case can
satisfy a source language's positive requirement.

Lua's original first-file input `---audit\nfunction f() end` is a separate
module-document control, with no f association. A preceding `module("m")` makes
the roster's later block declaration documentation. An undocumented `module()`
call does not create a synthetic prose document.

### Independent source and legacy readings

For Java, use these exact bytes, including the final LF:

```java
/** a
 * b
 * c */
/**/
class C {}
```

The old block occupies `0..18`, the new block `19..23`, and C `24..34`.
Extraction selects the new block, with complete BCA-local text `""`; the old
block is superseded evidence. Legacy DLOC bills the first three rows, not the
fourth. At the pre-implementation base, `bca metrics --no-config --no-ignore
--no-skip-generated --language java --format json --metrics loc <file>` reports
unit `(sloc, ploc, lloc, cloc, blank, dloc) = (5, 1, 0, 4, 0, 3)` and C's
`dloc = 0`. The empty-block-only twin has unit `dloc = 0` and `cloc = 1`.
The independently specified row set discriminates a wrong three-row selection
that would have the same count.

For `function f() {\n    /** note */\n    return 1;\n}\n`, legacy DLOC is one
in f and the unit. The comment at `19..30` does not describe a declaration
under the source JSDoc profile; retain it only as unattached evidence when
requested. Legacy coverage is not evidence of a semantic owner.

For `/*!*/\nfn f() {}\n`, the fragment `0..5` describes the file/root and has
empty local text. Legacy unit DLOC is one, while f's DLOC is zero. For
`/// outer\n#[test]\nfn test_f() {}\n\n/// kept\nfn f() {}\n`, source docs describe
both functions. Legacy default DLOC is two; `--exclude-tests` leaves only f
and DLOC one. Source collection does not inherit the metric pruning option.

For exact Rust input `m! { ///audit\nfn f() {} }\n`, the comment fragment is
`5..13`, with text `audit` copied from `8..13`. Retain an unresolved macro-expansion
association and unknown publication; the token-tree spelling does not establish
a concrete f owner. On the baseline, unit
`(sloc, ploc, lloc, cloc, blank, dloc) = (2, 2, 0, 1, 0, 1)` and there are no
nested metric spaces. This source uncertainty does not erase its legacy DLOC.

For `/** Record doc */\nrecord R(int a) {}\n`, the doc belongs to type R,
not its synthesized header function space. Adding `R {}` inside the record
replaces that synthesized metric space with the explicit compact constructor;
the doc owner remains R. Both baseline unit DLOC values are one, and both
type/function DLOC values are zero. Check the constructor node span, not just
the unchanged number of spaces.

### Composition, slots and mapping

Python `def f():\n    ""\n    "extra"\n    return 1\n` has primary fragment
`13..15` and additional fragment `20..27`. They form two documents with texts
`""` and `"extra"`, both associated with f. Legacy f/unit DLOC is two; runtime
`__doc__` remains empty. A b/f/t-prefixed second expression does not become
an additional docstring.

Python `def f():\n    ("@au" # ordinary\n     "dit")\n` has fragments `14..19`
and `36..41`. Complete text is `@audit`. Output `0..3` maps to `15..18`, and
`3..6` to `37..40`. The full `0..6` query returns both copied ranges and no
separator; the intervening ordinary comment is not text. This lets a consumer
find `@audit` while preserving its two physical origins.

For an escaped source example `"a\\x40b"`, the decoded value is `a@b`:
output `1..2` maps to complete escape token `2..6`. For `"é"`, output `0..2`
maps to source `1..3`; output query `0..1` fails because it splits the UTF-8
scalar. For `"\\N{COMMERCIAL AT}audit"`, raw bytes survive and complete text is
unsupported; an opaque escape followed by known `audit` is not a complete tag.
For `"\\ud800"`, the state is non-scalar/unrepresentable rather than replacement
character text.

For `///a\n///b\nfn f() {}`, the logical text `a\nb` maps `0..1` to source
`3..4`, `1..2` to a synthetic separator, and `2..3` to `8..9`. Removed `///`
prefixes emit nothing. An empty query at `0`, `1`, `2` or `3` returns no origins;
`3..4`, `2..1` and an out-of-bounds empty query fail. A query of only `1..2`
still returns its synthetic origin.

In `struct S(///first\n u32, ///second\n u32);`, each doc belongs to a tuple
field slot identified by parent S and its ordinal, even when no dedicated
field node exists. For `/**both*/\nint a, b;`, one document associates with
both qualifying declarators. Python receiver assignments enumerate only
qualifying receiver targets: in an `__init__` body, docs after
`self.x, other.y = values` associate with self.x, not other.y.

For Doxygen docs before `#if A\nvoid f();\n#else\nvoid g();\n#endif`, preserve
the conditional alternatives and an unresolved association. Do not claim that
the single document resolves simultaneously to f and g. A forward explicit
name resolves only under the adapter's same-file key; two matching overloads
without a disambiguating signature remain ambiguous.

### Query, context and state controls

| Input or operation | Required result |
| --- | --- |
| Rust `fn f() {}` queried for f | Supported owner, successful empty document list |
| Its `fn` keyword token queried as owner | Unsupported owner kind, not a successful empty list |
| Another parse of the same bytes, or `tree.clone()` | Foreign-tree error for its nodes |
| Two indices over the same tree | Equal source nodes may be queried in both; a handle from one index cannot be used in the other |
| Offset root or edited node value from the held tree | Noncanonical-node error when context no longer matches the indexed node |
| Rust profile requested for Python | Unsupported-profile error |
| Python feature disabled, even with empty input | Language-disabled error |
| Typed parse of a long input whose progress callback demonstrably aborts | Retained cancelled provenance; documentation query returns Cancelled outside the cancellation scope too |
| Complete parse of empty Rust input | Successful empty index |
| Elixir `@doc false` before f | Retained disabled state for f, no successful empty prose |
| Elixir `@doc nil` before f | Retained explicit-absence occurrence for f |
| Elixir `@doc ""` before f | Present-empty prose for f |
| Elixir `@doc since: "1"` before f | Metadata-only document for f |
| Elixir `@doc "text"` and `@doc since: "1"` before f | One document with text `text` and metadata, in either attribute order |
| Elixir `@doc "old"` then `@doc "new"` before f | Selected text `new`; old occurrence available as superseded evidence |
| Elixir `@doc "a#{x}b"` before f | Full text unavailable; known `a` and `b` separated by opaque interpolation |
| Elixir `@impl true` before f without `@doc` | No document; retain owner evidence with ineligible publication |
| Elixir `@impl true` and explicit `@doc "text"` before f | Present text `text`; impl fallback does not replace it |
| Elixir `@deprecated "old"` before f without `@doc` | Separate owner metadata evidence, no prose document |

For retained-abort tests, use the existing long-input cancellation fixture or a
deterministic progress hook and assert that parsing actually aborted. A short
parse can finish before tree-sitter calls its progress callback. An installed
cancelled flag alone is not retained-abort provenance. Collection invoked while
that flag is active returns Cancelled on either entry point; a genuinely
completed parse queried after the flag is removed remains complete.

For Java `/**pkg*/\npackage p;\nclass C {}`, the comment `0..8` has a source
association to package p. Explicit logical filename `package-info.java` permits
package-doc eligibility under the profile; another filename is ineligible and
missing filename is unknown. Merely setting the display label to
`package-info.java` does not change unknown into eligible. Attachment and raw
text do not disappear because publication eligibility is unavailable.

For the exact Rust source `///x\n\nfn f() {}`, supply included ranges `0..4`
and `6..15` with their corresponding tree-sitter points. The excluded `4..6`
gap prevents attaching the first range's comment to f. Preserve the comment as
unattached/uncertain evidence when requested, and never reparse to bridge the
gap. For a token envelope spanning such a gap, return only included source
pieces and unavailable complete text. An ordinary document wholly in a later
included range must still work.

For `# before\nwhen HTTP_REQUEST {\n# inside\n}\n`, iRules returns a leading
document `before` and a body document `inside`, in that order, both associated
with the same handler occurrence. Their fragments and roles remain distinct.
For `proc f {} {\n# first\nset x 1\n#ruff\n# later\n}`, Tcl returns one logical
body document `first\n\nlater`, composed from the lead block and marked block.
Replacing `#ruff` with an ordinary comment does not append the later block.

For Ruby `# @!method generated\n#   audit\n`, the YARD profile retains a
virtual method named generated. RDoc does not synthesize that YARD virtual
method. For `# audit\ndef f; end # :nodoc:\n`, RDoc retains the source document
for f with ineligible publication. A virtual document and a later concrete
method of the same name remain separate unless a profile explicitly defines
that named resolution.

### Required additional forms

The [case-keyed form inventory](documentation_extraction_cases.md#ownership)
supplies concrete inputs and outcomes for these acceptance families, including
the missing-form cleanup, TclOO, Elixir head/clauses and PHP callee matrices.
Its notation derives exact ranges from written source occurrences, independently
of implementation results. The families below track coverage ownership; they
do not let adapters choose between incompatible outcomes. A documented limitation
has a named unavailable or unresolved result, not an omitted promised form.

| Adapter | Required forms | Exact outcome rule |
| --- | --- | --- |
| [Rust #1664](https://github.com/dekobon/big-code-analysis/issues/1664) | `//!`, `/*!...*/`, `///`, `/**...*/`; cooked/raw `#[doc]` and `#![doc]`; mixtures | Inner docs describe the enclosing item/root; outer docs describe the following item/field/variant/slot; ordered mixed fragments form one document |
| Rust | Empty `/*!*/`, literal `#[doc = ""]`, `#[doc(hidden)]`, aliases and computed/cfg attributes | First two are present-empty; hidden/alias are metadata, not prose; computed/cfg-dependent content is unavailable or conditional evidence, never evaluated |
| Rust | Macro token trees, docs across attributes/plain comments, pruned test runs | Preserve macro/source uncertainty and original legacy coverage; source docs are independent of metric pruning |
| [Python #1665](https://github.com/dekobon/big-code-analysis/issues/1665) | Module/class/function/method first strings, additional strings, attribute assignments/type aliases, `#:` blocks | Distinct primary/additional/attribute/Sphinx roles; same-line/leading comments resolve qualifying target subsets |
| Python | Parentheses, implicit concatenation, comments/continuations between literals, raw strings, named escapes and surrogates | Parentheses/trivia are coverage only; literals compose without separators; named/non-scalar unavailability follows the decoder boundary |
| [JVM #1666](https://github.com/dekobon/big-code-analysis/issues/1666) | Java traditional/Markdown/package/module/record docs; Kotlin KDoc and split annotations; Groovy types/members/constructors | Preserve per-language nearest/last selection and normalized declaration owners; synthetic metric constructors do not steal type docs |
| JVM | Empty/new Java docs, `////`, blank source lines, annotations/body placements, package-info filename and Unicode escapes | Source marker rules differ from legacy; blank source lines split Markdown runs; filename eligibility requires explicit context; prelexing uncertainty remains explicit |
| [Csharp #1667](https://github.com/dekobon/big-code-analysis/issues/1667) | Types/delegates; methods/constructors/destructors/operators; fields/events/properties/indexers/enum members | XML stays literal; owners normalize expression-bodied/signature forms and preserve attributes/directive evidence |
| Csharp | Primary constructors, getter/setter trivia, function-body comments and `#if` members | Type docs remain type docs; excluded accessor/body placements remain unattached; uncertain branches remain alternatives |
| [Php #1668](https://github.com/dekobon/big-code-analysis/issues/1668) | File preambles, one/two/three blocks, native declarations, attributes, promoted properties, enum/define | Apply the explicit selected profile precedence; promoted-property docs belong to the property slot, not automatically the constructor |
| Php | `define`, `\\define`, grouping/uppercase, qualified/imported aliases, in-body calls | Static supported builtin forms have declared provenance; imported/dynamic identities remain unresolved rather than guessed |
| [JS #1669](https://github.com/dekobon/big-code-analysis/issues/1669) | Both dialects; functions/classes/members/variables/assignments/exports; nested bound callables | Normalize semantic binding owner and retain wrapper; do not attach to every declarator in a multi-declaration |
| JS | In-body comments, @file/@module/@name, duplicate or forward names, line-versus-block trivia | Unattached legacy DLOC has no fabricated owner; file/virtual docs preserve roles; collisions remain unresolved |
| [TS #1670](https://github.com/dekobon/big-code-analysis/issues/1670) | Both variants; export/declare/decorators, anonymous defaults, overloads, namespaces/interfaces/types/enums, supported parameters | Distinct source overload owners; declaration wrapper normalization and parameter-only after-code subset remain explicit |
| TS | @module/@packageDocumentation, assignment/destructuring exclusions, TSX JSX neighbors | Structural file blocks take precedence; excluded assignment/destructuring blocks remain unattached evidence, with no resolved binding edge |
| [C-family #1671](https://github.com/dekobon/big-code-analysis/issues/1671) | Every dialect; leading/trailing/member/signature/in-body docs, multi-declarators, ObjC methods/properties/protocols | Resolve qualifying concrete owners or source slots, with shared documents rather than copied text |
| C-family | @file gathering, page/group/explicit names, recovery/conditionals, masked names, quoted/CDATA tags | Preserve gatherer once; bounded structural keys finalize; ambiguity/masked original names remain unresolved; quoted/CDATA text does not create structural targets |
| [Go #1672](https://github.com/dekobon/big-code-analysis/issues/1672) | Package/function/method, type/var/const groups/specs, multi-name fields and interface elements | Group docs stay group docs; member docs remain separate; one field document can serve several named slots |
| Go | Build/generate/other directives, blank comment rows versus source rows, local/trailing placements | Retain directives as evidence but exclude them from prose as specified; source blank rows detach; excluded local/trailing placements do not get inferred owners |
| [Ruby #1673](https://github.com/dekobon/big-code-analysis/issues/1673) | Both profiles; methods/classes/modules/singletons/constants/aliases/attributes/visibility wrappers; body/modifier wrappers | Source attachment follows selected scope/call policy; later visibility changes affect eligibility, not occurrence identity |
| Ruby | `##`, `@!method`, `@!attribute`, `:nodoc:`, shebang/magic comments and named collisions | RDoc/YARD differences stay profile-qualified; virtual docs remain virtual; directives/magic comments are not arbitrary prose |
| [Lua #1674](https://github.com/dekobon/big-code-analysis/issues/1674) | `---`, long comments, first-file/module contexts, functions/assignments/returns/table fields, structural tags | Opener content and context determine attachment; standalone ordinary leading runs require inference; native comments on documented table fields/function parameters retain LDoc provenance |
| [Perl #1675](https://github.com/dekobon/big-code-analysis/issues/1675) | Direct POD, multiple opaque-node/data-section blocks, headings, comment-only and mixed regions, EOF/CRLF | Preserve physical subranges and POD markup; comment-only regions are not prose; default file/package docs do not attach to the nearest sub |
| Perl | Unique/forward/duplicate local headings, missing symbols | Explicit heading profile resolves only one qualifying local sub; duplicate/missing cases remain unresolved, with default file docs still available |
| [Elixir #1676](https://github.com/dekobon/big-code-analysis/issues/1676) | Module/function/macro/type/callback docs, explicit heads/clauses, distinct arities and visibility | Group source-written definitions by module/kind/name/arity; no synthesized default-argument arities or quote-template concrete owners |
| Elixir | Static/raw/cooked strings/heredocs/sigils; interpolation/computed; false/nil/empty/metadata; repeated attributes | Preserve per-attribute source evidence and selected compiler-style value/metadata composition; dynamic content unavailable; false/nil/empty/metadata remain distinct |
| [Bash #1677](https://github.com/dekobon/big-code-analysis/issues/1677) | Both function spellings, shdoc file/description blocks, opt-in leading inference, lexical continuation boundaries | Associate static same-scope functions/file; intervening commands detach; shebang/tool directives and heredoc/string look-alikes are not prose |
| [Tcl #1678](https://github.com/dekobon/big-code-analysis/issues/1678) | Static proc/TclOO method/constructor, lead/later #ruff blocks, literal namespace preamble | One logical body document per declaration with ordered block fragments; namespace preamble has namespace owner |
| Tcl | Dynamic names/hooks/preambles, later ordinary comments, continuations/recovery | Dynamic owner/content remains explicit; later unmarked comments excluded; #1802 affected boundaries/phantom declarations remain uncertain |
| [Irules #1679](https://github.com/dekobon/big-code-analysis/issues/1679) | proc/when, priority/modifiers, neighboring equal-name handlers, leading and body docs | Each handler occurrence has independent identity; both roles remain separate; exact same-scope/row adjacency |
| Irules | Nested control comments, quoted text, blank/intervening commands, continuations/recovery | No inferred nested control owner; continuation-affected apparent handlers remain uncertain; unrelated positive docs survive |

Sphinx, RDoc block and Go block mapping examples are in
[comment cleanup cases](documentation_extraction_cases.md#comment-cleanup).
Lua ordinary trailing comments on documented table fields and function
parameters have native LDoc provenance, as described in
[LDoc's code-inference reference](https://lunarmodules.github.io/ldoc/manual/manual.md.html#Inferring_more_from_Code).
They are not the standalone ordinary leading runs that require generic inference.

### Reference evidence and baseline

The semantic baseline is commit
[`91087f52f5d4a58e31ae320851351990f1436411`](https://github.com/dekobon/big-code-analysis/tree/91087f52f5d4a58e31ae320851351990f1436411).
Its grammar versions are recorded in the versioned
[`langs.rs`](https://github.com/dekobon/big-code-analysis/blob/91087f52f5d4a58e31ae320851351990f1436411/big-code-analysis-ast/src/langs.rs)
and [`Cargo.toml`](https://github.com/dekobon/big-code-analysis/blob/91087f52f5d4a58e31ae320851351990f1436411/Cargo.toml).
The characterization manifest records those exact pins rather than relying on
the current branch's versions. Recapture if the implementation base changes.

| Reference | Evidence used here | Qualification boundary |
| --- | --- | --- |
| Zulu OpenJDK 25.0.5.0.3 | `JavacTask.parse`, `DocTrees.getDocComment` and `getDocCommentTree` report `/` for `/**/` and `////`, empty for `/***/` and `/****/` | Observed source attachment/text anomaly, not universal version parity |
| Python 3.12.13 | `ast.get_docstring(clean=False)` then `inspect.cleandoc` returns empty primary, disjoint `@audit`, and preserved trailing spaces | Named escapes work in Python itself; BCA's unsupported result is deliberate |
| Rust 1.99.0 | Tag-qualified cleanup source above | Source inspection, not a claimed rustdoc output comparison |
| phpDocumentor Reflection 7.0.0 | Commit `70254fa0b3058b16908a3a9d95f7dca904e03396`, `createFileDocBlock` | Source inspection; exactly-two rule and restricted declaration set |
| Elixir 1.20.1 | `Module.put_attribute`, `compile_doc` and metadata merge source | Source inspection; later non-keyword values replace, false is not sticky |
| Ruff! 3.0.4 | Procedure and directive manual sections | Manual evidence; adapter qualification also checks implementation |
| Other named profiles | Linked authoritative manuals and each adapter's source/tool fixtures | Do not claim executed tool parity until its issue records version, configuration and command |

The legacy numbers in these worked cases were measured with the baseline's
`bca` after its full validation gate passed. The command above selects LOC;
each language case forces its own language and uses the exact written source.
The characterization issue retains whole-metric captures, both MI modes and
additional independent row-set assertions before migration.

The [reference observation record](https://gist.github.com/dekobon/acb5b4fd9ada1c2b63f86ae0f0514524)
preserves the exact Java/Python commands, source inputs and legacy measurements.

## Qualification and delivery {#qualification}

Author the source/form inventory independently from implementation capabilities.
Each source/dialect variant requires useful declaration-associated nonempty text;
helper outcomes are explicit exceptions. Test Tsx, Mozjs and Mozcpp separately.
Every additional promised form needs its own exact result: empty, disabled,
metadata-only, ambiguous and unavailable results cannot stand in for missing
positive language support.

Create raw-byte fixtures under `tests/fixtures/documentation/` in the
[characterization issue](https://github.com/dekobon/big-code-analysis/issues/1657).
Share routing through AST test-support, keeping root metric assertions at the
root. Record baseline revision, exact grammar pins, tool/configuration references,
source bytes and independently specified expected owners/text/spans. External
tools are optional reproduction oracles; CI uses committed deterministic inputs.

Keep source semantics and legacy preservation assertions separate. Compare
whole relevant metrics and physical row sets in both MI documentation modes,
selected-metric paths and `exclude_tests`. Run EOF/CRLF inputs verbatim. Do not
sort or trim observations, derive expected owners from capabilities, or rely
only on facade/delegate equality. Follow the [testing guide](testing.md) to
perturb decision-bearing expressions and verify the intended assertion fails.

| Work | Completion evidence |
| --- | --- |
| Facts and characterization (#1656/#1657) | Contract examples fit the model; independent fixtures and baseline precede migrations |
| Marker/positional migrations (#1658/#1659) | Atomic root caller migration; legacy reading/timing and zero extra metric-only materialization |
| Text and index (#1660/#1661) | Joint real comment → owner → complete text → exact map production smoke |
| Initial performance/fuzz infrastructure (#1662/#1663) | Direct-AST dispatch and representative positive implemented paths |
| Each adapter (#1664–#1679) | Its promised forms, independent positive cases, negative controls, probes and seeds |
| Stable facade (#1680) | Shared implementation, root API/docs/stability/changelog, facade probes and fuzz fanout |
| Full qualification (#1681) | Exhaustive independent language/form inventory, feature isolation and legacy compatibility |
| Guide and closeout (#1682) | Consumer guide matches implementation and all earlier evidence is linked |

The text/index pair is a coordinated completion unit. Initial infrastructure
does not wait for future adapters; each adapter supplies its own positive
qualification. The full inventory is checked again at final qualification.
Run required gates and triggered scaling/chain/fuzz checks from [AGENTS.md](../../AGENTS.md).
Tie review and validation evidence to exact revisions. Local completion and
integration remain distinct from publication and merge.
