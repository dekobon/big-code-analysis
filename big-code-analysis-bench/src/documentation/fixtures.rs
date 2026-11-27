//! Stable workload identities and source generators; shared by both timing methods.

use big_code_analysis_ast::LANG;
use big_code_analysis_ast::documentation::DocumentationOptions;
use std::fmt::Write;

use super::{HarnessError, require};

/// Statistic implied by the operation's declared input/output growth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatisticClass {
    /// Construction with linear or linearithmic legitimate work/output.
    Exponent,
    /// Indexed query with a fixed number of returned records.
    FixedOutputRatio,
    /// Fixed-output map query, normalized by log2(mapping segments + 1).
    LogMapRatio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Collect,
    Owner,
    Pending,
    OwnerQualifications,
    DocumentQualifications,
    Map,
    MapPieces,
    Retained,
    Render,
    EvidencePair,
}

/// One source workload, including the declared timing and growth policy.
#[derive(Clone, Copy, Debug)]
pub struct SourceProbe {
    /// Stable full Criterion/gate identifier.
    pub id: &'static str,
    /// Source grammar; the default profile is used unless explicitly overridden.
    pub language: LANG,
    /// Actual measured size ladder.
    pub sizes: [usize; 3],
    /// Class-specific statistic, never an untyped exponent/ratio field.
    pub class: StatisticClass,
    /// Frozen bound, from the approved documentation infrastructure policy.
    pub bound: f64,
    /// Varied dimensions and held-fixed dimensions.
    pub dimensions: &'static str,
    /// Timed work, preparation and destruction boundary.
    pub timing: &'static str,
    /// Policy provenance, independent of observed timings.
    pub policy: &'static str,
    pub(super) operation: Operation,
    pub(super) key: &'static str,
    pub(super) evidence: bool,
}

const fn probe(
    id: &'static str,
    key: &'static str,
    language: LANG,
    large: bool,
    operation: Operation,
    dimensions: &'static str,
) -> SourceProbe {
    SourceProbe {
        id,
        key,
        language,
        sizes: if large {
            [1024, 2048, 4096]
        } else {
            [256, 512, 1024]
        },
        class: match operation {
            Operation::Collect | Operation::Render | Operation::EvidencePair => {
                StatisticClass::Exponent
            }
            Operation::Map | Operation::MapPieces => StatisticClass::LogMapRatio,
            _ => StatisticClass::FixedOutputRatio,
        },
        bound: 1.50,
        dimensions,
        timing: match operation {
            Operation::Collect => "parse excluded; collect eager text/maps and drop index included",
            Operation::EvidencePair => {
                "parse excluded; off collection/drop then on collection/drop included; both outputs validated separately"
            }
            Operation::Render => {
                "parse/index/validated fragment preparation excluded; fresh render and content drop included"
            }
            _ => {
                "parse/index/query handles excluded; lookup and lazy record consumption included; borrowed result drop included"
            }
        },
        policy: "docs/development/benchmarking.md#source-documentation: premeasurement median/exponent/fixed-K policy, bound 1.50",
        operation,
        evidence: false,
    }
}

/// The original workloads followed by infrastructure and Rust literal ladders.
pub const REGISTRY: &[SourceProbe] = &[
    probe(
        "documentation/cold/depth",
        "depth",
        LANG::Rust,
        false,
        Operation::Collect,
        "grow lexical depth/nodes/bytes; F=E=1, text=5, K fixed",
    ),
    probe(
        "documentation/cold/width",
        "width",
        LANG::Rust,
        false,
        Operation::Collect,
        "grow siblings/docs/F/E/text/maps; lexical depth fixed",
    ),
    probe(
        "documentation/cold/fragments",
        "fragments",
        LANG::Go,
        false,
        Operation::Collect,
        "grow fragments/text/maps/bytes; D=E=1, depth fixed",
    ),
    probe(
        "documentation/cold/single-line-lua",
        "single-line-lua",
        LANG::Lua,
        false,
        Operation::Collect,
        "grow comment bytes/text; D=F=E=1, nodes fixed",
    ),
    probe(
        "documentation/cold/single-line-jsdoc",
        "single-line-jsdoc",
        LANG::Javascript,
        false,
        Operation::Collect,
        "grow comment bytes/text; D=F=E=1, nodes fixed",
    ),
    probe(
        "documentation/cold/c-slots",
        "c-slots",
        LANG::C,
        false,
        Operation::Collect,
        "grow slots/edges/name bytes; D=F=1, text=5",
    ),
    probe(
        "documentation/cold/wide-error",
        "wide-error",
        LANG::C,
        false,
        Operation::Collect,
        "grow recovery siblings; one unresolved document, E=0",
    ),
    probe(
        "documentation/cold/missing",
        "missing",
        LANG::Rust,
        false,
        Operation::Collect,
        "grow visible parentheses; one missing token, D=E=0",
    ),
    probe(
        "documentation/cold/names",
        "names",
        LANG::Cpp,
        false,
        Operation::Collect,
        "grow names/references/docs/edges; distinct buckets, variable-width legacy names",
    ),
    probe(
        "documentation/secondary/single-line-php",
        "single-line-php",
        LANG::Php,
        false,
        Operation::Collect,
        "grow comment bytes/text; D=F=E=1",
    ),
    probe(
        "documentation/secondary/single-line-ruby",
        "single-line-ruby",
        LANG::Ruby,
        false,
        Operation::Collect,
        "grow comment bytes/text; D=F=E=1",
    ),
    probe(
        "documentation/secondary/single-line-doxygen",
        "single-line-doxygen",
        LANG::Cpp,
        false,
        Operation::Collect,
        "grow comment bytes/text; D=F=E=1",
    ),
    probe(
        "documentation/secondary/text-fixed-fanout",
        "text-fixed-fanout",
        LANG::C,
        false,
        Operation::Collect,
        "grow text bytes; D=F=1, E=2, map segments=1",
    ),
    probe(
        "documentation/secondary/fanout-fixed-text",
        "fanout-fixed-text",
        LANG::C,
        false,
        Operation::Collect,
        "grow slots/edges; D=F=1, text=6143, map segments=1",
    ),
    probe(
        "documentation/secondary/visible-declarator-depth",
        "visible-declarator-depth",
        LANG::C,
        false,
        Operation::Collect,
        "grow visible array-declarator depth; D=F=E=1; not hidden-only FFI cost",
    ),
    probe(
        "documentation/secondary/ruby-modifiers",
        "ruby-modifiers",
        LANG::Ruby,
        false,
        Operation::Collect,
        "grow visible modifier depth and field requests; D=F=E=1",
    ),
    probe(
        "documentation/secondary/java-undocumented",
        "java-undocumented",
        LANG::Java,
        false,
        Operation::Collect,
        "grow lexical depth/owners; D=F=E=0",
    ),
    probe(
        "documentation/secondary/pending-scope",
        "pending-scope",
        LANG::Rust,
        false,
        Operation::Collect,
        "grow unrelated owners; one pending doc, three assessments",
    ),
    probe(
        "documentation/prepared/owner",
        "prepared",
        LANG::Rust,
        false,
        Operation::Owner,
        "grow F/text/maps; one owner, query K=1",
    ),
    probe(
        "documentation/prepared/late-map",
        "prepared",
        LANG::Rust,
        false,
        Operation::Map,
        "grow S=2n-1; one last-byte origin, K=1; normalize log2(S+1)",
    ),
    probe(
        "documentation/prepared/retained",
        "prepared",
        LANG::Rust,
        false,
        Operation::Retained,
        "grow F/text/maps; one borrowed complete-text view, K=1",
    ),
    probe(
        "documentation/prepared/late-owner",
        "width",
        LANG::Rust,
        false,
        Operation::Owner,
        "grow unrelated owners/docs; last owner K=1",
    ),
    probe(
        "documentation/ranges/cold",
        "ranges",
        LANG::Rust,
        false,
        Operation::Collect,
        "grow R/D/F/E equally; one intersection per fragment, depth fixed",
    ),
    probe(
        "documentation/remediation/inline-spans",
        "inline-spans",
        LANG::Cpp,
        true,
        Operation::Collect,
        "grow inline spans/bytes/text; D=F=E=1",
    ),
    probe(
        "documentation/remediation/gathered-disposition",
        "gathered-disposition",
        LANG::Cpp,
        true,
        Operation::Collect,
        "grow gathered fragments and final block bytes; one file document",
    ),
    probe(
        "documentation/remediation/complex-signatures",
        "complex-signatures",
        LANG::Cpp,
        true,
        Operation::Collect,
        "grow declaration depth=n/16; pending signatures, D=0",
    ),
    probe(
        "documentation/remediation/order-multiple-chunks",
        "width",
        LANG::Rust,
        true,
        Operation::Collect,
        "grow docs/F/E/text/maps through ordering chunk boundary",
    ),
    probe(
        "documentation/remediation/text-budget-boundary",
        "text-budget-boundary",
        LANG::Rust,
        true,
        Operation::Collect,
        "grow text bytes=8n through copy-budget boundary; D=F=E=1",
    ),
    probe(
        "documentation/remediation/go-fold",
        "go-fold",
        LANG::Go,
        true,
        Operation::Collect,
        "grow field depth=n/16; undocumented",
    ),
    probe(
        "documentation/remediation/ruby-fold",
        "ruby-fold",
        LANG::Ruby,
        true,
        Operation::Collect,
        "grow class depth=n/16; one inner documented function",
    ),
    probe(
        "documentation/remediation/multiline-adjacent-ranges",
        "multiline-adjacent-ranges",
        LANG::Java,
        true,
        Operation::Collect,
        "grow rows/R/intersections/text/maps; D=F=E=1",
    ),
    probe(
        "documentation/qualifications/candidates",
        "qualified-candidates",
        LANG::Cpp,
        false,
        Operation::Collect,
        "grow C with D=1: E=Q=C; no D*C diagonal",
    ),
    probe(
        "documentation/qualifications/references",
        "qualified-references",
        LANG::Cpp,
        false,
        Operation::Collect,
        "grow D with C=2: E=2D,Q=2D; fixed candidate factor",
    ),
    probe(
        "documentation/qualifications/late-owner",
        "qualified-unrelated",
        LANG::Cpp,
        false,
        Operation::OwnerQualifications,
        "grow unrelated Q/docs/owners; last owner qualification K=1",
    ),
    probe(
        "documentation/qualifications/late-document",
        "qualified-unrelated",
        LANG::Cpp,
        false,
        Operation::DocumentQualifications,
        "grow unrelated Q/docs/owners; last document qualification K=1",
    ),
    SourceProbe {
        class: StatisticClass::Exponent,
        ..probe(
            "documentation/pending/ancestry",
            "pending-ancestry",
            LANG::Rust,
            false,
            Operation::Pending,
            "grow nonempty pending ancestry; K=2n, linear-output query",
        )
    },
    probe(
        "documentation/pending/late",
        "pending-late",
        LANG::Rust,
        false,
        Operation::Pending,
        "grow unrelated owners and empty scopes; fixed pending K=2",
    ),
    probe(
        "documentation/structure/wide-depth",
        "wide-depth",
        LANG::Rust,
        false,
        Operation::Collect,
        "grow lexical depth; innermost width=32 documented siblings fixed",
    ),
    probe(
        "documentation/structure/recovery-depth",
        "recovery-depth",
        LANG::C,
        false,
        Operation::Collect,
        "grow enclosing struct depth; innermost recovered field-list has 32 documented siblings and one invalid token",
    ),
    probe(
        "documentation/structure/comment-run-depth",
        "comment-run-depth",
        LANG::Rust,
        false,
        Operation::Collect,
        "grow depth and innermost F=n; one doc, bytes/text/maps linear",
    ),
    probe(
        "documentation/structure/conditional-depth",
        "conditional-depth",
        LANG::Cpp,
        false,
        Operation::Collect,
        "one conditional per struct depth; nodes/bytes grow, one leaf document",
    ),
    probe(
        "documentation/names/missing",
        "missing-name",
        LANG::Cpp,
        false,
        Operation::Collect,
        "grow unrelated declarations with fixed-width names; one valid unresolved reference",
    ),
    SourceProbe {
        evidence: true,
        ..probe(
            "documentation/evidence/rejected",
            "rejected",
            LANG::Java,
            false,
            Operation::EvidencePair,
            "grow rejected blocks, evidence=n-1; timed off/on pair, one selected doc in both",
        )
    },
    SourceProbe {
        evidence: true,
        ..probe(
            "documentation/evidence/directives",
            "directives",
            LANG::Go,
            false,
            Operation::EvidencePair,
            "grow directive-only comments, evidence=n; timed off/on pair, no selected docs",
        )
    },
    SourceProbe {
        sizes: [4096, 8192, 16384],
        ..probe(
            "documentation/render/bytes",
            "text-fixed-fanout",
            LANG::C,
            false,
            Operation::Render,
            "grow text bytes; D=F=1,E=2,S=1; fresh output, not retained access",
        )
    },
    probe(
        "documentation/render/fragments",
        "prepared",
        LANG::Rust,
        false,
        Operation::Render,
        "grow F=n,S=2n-1,text=2n-1; one document/owner",
    ),
    probe(
        "documentation/rust-unit1/mixed-fragments",
        "rust-mixed",
        LANG::Rust,
        false,
        Operation::Collect,
        "grow mixed F=2n+1,text=14n+5,S=6n+1; D=E=1, depth fixed",
    ),
    SourceProbe {
        sizes: [4096, 8192, 16384],
        ..probe(
            "documentation/rust-unit1/cooked-large",
            "rust-cooked",
            LANG::Rust,
            false,
            Operation::Collect,
            "grow cooked escape bytes and cleanup; F=D=E=1,text=S=2n+1",
        )
    },
    SourceProbe {
        sizes: [4096, 8192, 16384],
        ..probe(
            "documentation/rust-unit1/raw-large",
            "rust-raw",
            LANG::Rust,
            false,
            Operation::Collect,
            "grow raw physical CRLF bytes; F=D=E=1,text=6n+1,S=2n+1",
        )
    },
    probe(
        "documentation/rust-unit1/attribute-width",
        "rust-attribute-width",
        LANG::Rust,
        false,
        Operation::Collect,
        "grow attribute width w=n at d=0; F=n,text=S=2n-1,D=E=1",
    ),
    probe(
        "documentation/rust-unit1/attribute-depth",
        "rust-attribute-depth",
        LANG::Rust,
        false,
        Operation::Collect,
        "grow module depth d=n at w=1; F=D=E=text=S=1",
    ),
    probe(
        "documentation/rust-unit1/attribute-depth-width",
        "rust-attribute-depth-width",
        LANG::Rust,
        false,
        Operation::Collect,
        "grow d=w=n; affine nodes/bytes/output, F=n,text=S=2n-1,D=E=1",
    ),
    probe(
        "documentation/rust-unit1/escapes-contiguous",
        "rust-escapes-contiguous",
        LANG::Rust,
        true,
        Operation::Collect,
        "grow n escapes; text=S=n,F=D=E=R=1, one responsible piece/token",
    ),
    probe(
        "documentation/rust-unit1/escapes-split",
        "rust-escapes-split",
        LANG::Rust,
        true,
        Operation::Collect,
        "grow n escapes; text=S=n,F=D=E=1,R=21+4n, four pieces/token",
    ),
    probe(
        "documentation/rust-unit1/mixed-retained",
        "rust-mixed",
        LANG::Rust,
        false,
        Operation::Retained,
        "grow mixed F=2n+1,text=14n+5,S=6n+1; one borrowed text/map view K=1",
    ),
    probe(
        "documentation/rust-unit1/mixed-owner",
        "rust-mixed",
        LANG::Rust,
        false,
        Operation::Owner,
        "grow mixed F=2n+1,text=14n+5,S=6n+1; canonical function association K=1",
    ),
    probe(
        "documentation/rust-unit1/split-late-map",
        "rust-escapes-split",
        LANG::Rust,
        true,
        Operation::MapPieces,
        "grow S=n; query final byte, consume one origin and four borrowed pieces, normalize log2(S+1)",
    ),
];

impl SourceProbe {
    /// The explicit options used by both runners.
    #[must_use]
    pub fn options(&self) -> DocumentationOptions {
        DocumentationOptions::default()
            .with_unbound_evidence(self.evidence && self.key != "directives")
            .with_directive_evidence(self.evidence && self.key == "directives")
    }

    /// Generate the exact source for a positive size, without newline normalization.
    ///
    /// # Errors
    /// Rejects sizes outside 1..=2^20 or unregistered fixture keys.
    #[allow(clippy::too_many_lines)] // One explicit spelling per registered fixture keeps byte preservation reviewable.
    pub fn source(&self, size: usize) -> Result<Vec<u8>, HarnessError> {
        require(
            size > 0 && size <= 1 << 20,
            "source fixture size must be in 1..=2^20",
        )?;
        let depth = size / 16;
        let source = match self.key {
            "depth" => format!(
                "{}///audit\nfn leaf() {{}}{}",
                "mod m {".repeat(size),
                "}".repeat(size)
            ),
            "width" | "ranges" => "///audit\nfn f() {}\n".repeat(size),
            "fragments" => format!("package p\n{}func F() {{}}", "// audit\n".repeat(size)),
            "single-line-lua" => format!(
                "module(\"m\")\n---{}\nfunction f() end",
                "audit ".repeat(size)
            ),
            "single-line-jsdoc" => format!("/**{}*/\nfunction f() {{}}", "audit ".repeat(size)),
            "c-slots" => format!("/**audit*/\nint {};", slots(size)),
            "wide-error" => format!("/**audit*/\nstruct {{\n{}", "int x;\n".repeat(size)),
            "missing" => format!(
                "fn f() {{ {}0{}; }}",
                "(".repeat(size),
                ")".repeat(size - 1)
            ),
            "names" => (0..size).fold(String::new(), |mut source, at| {
                let _ = writeln!(source, "/** @fn void f{at}() */\nvoid f{at}() {{}}");
                source
            }),
            "single-line-php" => {
                format!("<?php\n/** {}*/\nfunction f() {{}}", "audit ".repeat(size))
            }
            "single-line-ruby" => format!("# {}\ndef f\nend", "audit ".repeat(size)),
            "single-line-doxygen" => format!("/**{}*/\nvoid f() {{}}", "audit ".repeat(size)),
            "text-fixed-fanout" => format!("/**{}*/\nint a,b;", "audit ".repeat(size)),
            "fanout-fixed-text" => format!("/**{}*/\nint {};", "audit ".repeat(1024), slots(size)),
            "visible-declarator-depth" => format!("/**audit*/\nint a{};", "[1]".repeat(size)),
            "ruby-modifiers" => format!("# audit\ndef f; end{}", " if true".repeat(size)),
            "java-undocumented" => format!(
                "{}void f() {{}}{}",
                "class C {".repeat(size),
                "}".repeat(size)
            ),
            "pending-scope" => format!("#![doc(hidden)]\n{}", "fn f() {}\n".repeat(size)),
            "prepared" => format!("{}fn f() {{}}", "///a\n".repeat(size)),
            "inline-spans" => format!("/**{}*/\nvoid f() {{}}", "`x` ".repeat(size)),
            "gathered-disposition" => {
                format!("{}/**{}@file*/", "/**a*/\n".repeat(size), "x ".repeat(size))
            }
            "complex-signatures" => format!(
                "{}void leaf();{}",
                "void f(int x = [] { ".repeat(depth),
                " return 1; }());".repeat(depth)
            ),
            "text-budget-boundary" => format!("///{}\nfn f() {{}}", "abcd".repeat(size * 2)),
            "go-fold" => format!(
                "package p\ntype S struct {{\n{}X int\n{} }}\n",
                "X struct {\n".repeat(depth),
                "}\n".repeat(depth)
            ),
            "ruby-fold" => format!(
                "{}# audit\ndef f; end\n{}",
                "class C\n".repeat(depth),
                "end\n".repeat(depth)
            ),
            "multiline-adjacent-ranges" => {
                format!("/**\n{}*/\nclass C {{}}", " * audit\n".repeat(size))
            }
            "qualified-candidates" => format!(
                "#if FLAG\n{}#endif\n/**@fn f*/",
                "void f(void);\n".repeat(size)
            ),
            "qualified-references" => format!(
                "#if FLAG\nvoid f(void);\nvoid f(void);\n#endif\n{}",
                "/**@fn f*/\nint separator;\n".repeat(size)
            ),
            "qualified-unrelated" => (0..size).fold(String::new(), |mut source, at| {
                let _ = writeln!(
                    source,
                    "#if FLAG\nvoid f{at:08}(void);\n#endif\n/**@fn f{at:08}*/\nint separator;"
                );
                source
            }),
            "pending-ancestry" => format!(
                "{}fn leaf() {{}}{}",
                "mod m {\n#![doc(hidden)]\n".repeat(size),
                "}\n".repeat(size)
            ),
            "pending-late" => format!(
                "#![doc(hidden)]\n{}{}fn leaf() {{}}{}",
                "fn f() {}\n".repeat(size),
                "mod m {".repeat(size),
                "}".repeat(size)
            ),
            "wide-depth" => format!(
                "{}{}{}",
                "mod m {".repeat(size),
                "///audit\nfn f() {}\n".repeat(32),
                "}".repeat(size)
            ),
            "recovery-depth" => format!(
                "{}@ {}{}",
                "struct S {\n".repeat(size),
                "/**audit*/\nint x;\n".repeat(32),
                "};\n".repeat(size)
            ),
            "comment-run-depth" => format!(
                "{}{}fn f() {{}}{}",
                "mod m {".repeat(size),
                "///a\n".repeat(size),
                "}".repeat(size)
            ),
            "conditional-depth" => format!(
                "{}/**audit*/\nvoid f();\n{}",
                "struct S {\n#if FLAG\n".repeat(size),
                "#endif\n};\n".repeat(size)
            ),
            "missing-name" => format!(
                "{}/**@fn absent*/",
                (0..size).fold(String::new(), |mut source, at| {
                    let _ = writeln!(source, "void f{at:08}(void);");
                    source
                })
            ),
            "rejected" => format!("{}class C {{}}", "/**audit*/\n".repeat(size)),
            "directives" => format!(
                "package p\n{}func F() {{}}",
                "//go:generate command\n".repeat(size)
            ),
            "rust-mixed" => format!(
                "{}fn f() {{ //!inner\n}}",
                "/**\u{2003}* a\n * b*/\n#[doc = \"z\"]\n".repeat(size)
            ),
            "rust-cooked" => format!("#[doc = \"\\n{}b\"] fn f() {{}}", "a\\r\\r\\n".repeat(size)),
            "rust-raw" => format!(
                "#[doc = r###\"{}b\"###] fn f() {{}}",
                "a\\x42\r\n".repeat(size)
            ),
            "rust-attribute-width" | "rust-attribute-depth" | "rust-attribute-depth-width" => {
                let d = if self.key == "rust-attribute-width" {
                    0
                } else {
                    size
                };
                let w = if self.key == "rust-attribute-depth" {
                    1
                } else {
                    size
                };
                format!(
                    "{}{}fn f() {{}}{}",
                    "mod m {".repeat(d),
                    "#[doc = \"a\"]\n".repeat(w),
                    "}".repeat(d)
                )
            }
            "rust-escapes-contiguous" | "rust-escapes-split" => {
                format!("#[doc = \"{}\"] fn f() {{}}", "\\x41".repeat(size))
            }
            _ => {
                return Err(HarnessError(format!(
                    "unregistered source fixture {}",
                    self.key
                )));
            }
        };
        Ok(source.into_bytes())
    }
}

fn slots(size: usize) -> String {
    (0..size)
        .map(|at| format!("v{at}"))
        .collect::<Vec<_>>()
        .join(",")
}
