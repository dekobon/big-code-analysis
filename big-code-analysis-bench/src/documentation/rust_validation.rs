//! Source-derived Rust literal, mixed-cleanup and attachment oracles.

use std::collections::HashMap;
use std::ops::Range;

use big_code_analysis_ast::documentation::{
    AssessmentAxis, CompleteText, Composition, DocumentRole, DocumentationIndex, FragmentForm,
    Mapping, Origin, OriginView, Owner, Publication, Resolution, SourcePiece, SourcePieces,
};
use big_code_analysis_ast::tree_sitter::{Node, Point};

use super::{HarnessError, Parsed, SourceProbe, failure, require};

#[derive(Debug)]
pub(super) enum ExpectedOrigin {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Synthetic,
}

struct Expected {
    source: Vec<u8>,
    text: String,
    maps: Vec<(Range<usize>, ExpectedOrigin)>,
    fragments: Vec<(Range<usize>, FragmentForm)>,
    function: Range<usize>,
    depth: usize,
    split: bool,
}

impl Expected {
    #[allow(clippy::too_many_lines)] // Keep each independent source family's bytes and map formulas together.
    fn new(key: &str, n: usize) -> Result<Self, HarnessError> {
        let mut expected = Self {
            source: Vec::new(),
            text: String::new(),
            maps: Vec::new(),
            fragments: Vec::new(),
            function: 0..0,
            depth: 0,
            split: key == "rust-escapes-split",
        };
        match key {
            "rust-mixed" => {
                for _ in 0..n {
                    expected
                        .source
                        .extend_from_slice(b"/**\xe2\x80\x83* a\n * b*/\n#[doc = \"z\"]\n");
                }
                expected.source.extend_from_slice(b"fn f() { //!inner\n}");
                expected.text = format!("{}inner", "\u{2003}* a\n * b\nz\n".repeat(n));
                for i in 0..n {
                    let s = 30 * i;
                    let o = 14 * i;
                    expected.fragments.extend([
                        (s..s + 16, FragmentForm::Comment),
                        (s + 17..s + 29, FragmentForm::Attribute),
                    ]);
                    expected.maps.extend([
                        (o..o + 6, ExpectedOrigin::Copied(s + 3..s + 9)),
                        (o + 6..o + 7, ExpectedOrigin::Copied(s + 9..s + 10)),
                        (o + 7..o + 11, ExpectedOrigin::Copied(s + 10..s + 14)),
                        (o + 11..o + 12, ExpectedOrigin::Synthetic),
                        (o + 12..o + 13, ExpectedOrigin::Copied(s + 26..s + 27)),
                        (o + 13..o + 14, ExpectedOrigin::Synthetic),
                    ]);
                }
                expected
                    .fragments
                    .push((30 * n + 9..30 * n + 17, FragmentForm::Comment));
                expected.maps.push((
                    14 * n..14 * n + 5,
                    ExpectedOrigin::Copied(30 * n + 12..30 * n + 17),
                ));
                expected.function = 30 * n..30 * n + 19;
            }
            "rust-cooked" | "rust-raw" => {
                let raw = key == "rust-raw";
                expected.source.extend_from_slice(if raw {
                    b"#[doc = r###\""
                } else {
                    b"#[doc = \"\\n"
                });
                for _ in 0..n {
                    expected.source.extend_from_slice(if raw {
                        b"a\\x42\r\n"
                    } else {
                        b"a\\r\\r\\n"
                    });
                }
                expected.source.extend_from_slice(if raw {
                    b"b\"###] fn f() {}"
                } else {
                    b"b\"] fn f() {}"
                });
                let (s, width, end) = if raw { (13, 6, 19) } else { (11, 2, 14) };
                expected.text = format!("{}b", if raw { "a\\x42\n" } else { "a\n" }.repeat(n));
                expected
                    .fragments
                    .push((0..7 * n + end, FragmentForm::Attribute));
                for i in 0..n {
                    expected.maps.push((
                        width * i..width * i + width - 1,
                        ExpectedOrigin::Copied(s + 7 * i..s + 7 * i + width - 1),
                    ));
                    expected.maps.push((
                        width * i + width - 1..width * (i + 1),
                        ExpectedOrigin::Decoded(s + 7 * i + 5..s + 7 * i + 7),
                    ));
                }
                expected.maps.push((
                    width * n..width * n + 1,
                    ExpectedOrigin::Copied(s + 7 * n..s + 7 * n + 1),
                ));
                expected.function = 7 * n + end + 1..7 * n + end + 10;
            }
            "rust-attribute-width" | "rust-attribute-depth" | "rust-attribute-depth-width" => {
                let d = if key == "rust-attribute-width" { 0 } else { n };
                let w = if key == "rust-attribute-depth" { 1 } else { n };
                expected.depth = d;
                for _ in 0..d {
                    expected.source.extend_from_slice(b"mod m {");
                }
                for _ in 0..w {
                    expected.source.extend_from_slice(b"#[doc = \"a\"]\n");
                }
                expected.source.extend_from_slice(b"fn f() {}");
                expected.source.extend(std::iter::repeat_n(b'}', d));
                expected.text = std::iter::repeat_n("a", w).collect::<Vec<_>>().join("\n");
                for j in 0..w {
                    let s = 7 * d + 13 * j;
                    expected
                        .fragments
                        .push((s..s + 12, FragmentForm::Attribute));
                    expected
                        .maps
                        .push((2 * j..2 * j + 1, ExpectedOrigin::Copied(s + 9..s + 10)));
                    if j + 1 < w {
                        expected
                            .maps
                            .push((2 * j + 1..2 * j + 2, ExpectedOrigin::Synthetic));
                    }
                }
                expected.function = 7 * d + 13 * w..7 * d + 13 * w + 9;
            }
            "rust-escapes-contiguous" | "rust-escapes-split" => {
                expected.source.extend_from_slice(b"#[doc = \"");
                for _ in 0..n {
                    expected.source.extend_from_slice(b"\\x41");
                }
                expected.source.extend_from_slice(b"\"] fn f() {}");
                expected.text = "A".repeat(n);
                expected
                    .fragments
                    .push((0..11 + 4 * n, FragmentForm::Attribute));
                expected.maps = (0..n)
                    .map(|i| (i..i + 1, ExpectedOrigin::Decoded(9 + 4 * i..13 + 4 * i)))
                    .collect();
                expected.function = 12 + 4 * n..21 + 4 * n;
            }
            _ => return Err(HarnessError(format!("unregistered Rust oracle {key}"))),
        }
        Ok(expected)
    }
}

fn pieces(
    actual: &SourcePieces<'_>,
    range: Range<usize>,
    source: &[u8],
    split: bool,
) -> Result<(), HarnessError> {
    require(!actual.has_excluded_gap(), "Rust unexpected excluded gap")?;
    piece_slice(actual.pieces(), range, source, split)
}

fn piece_slice(
    actual: &[SourcePiece<'_>],
    range: Range<usize>,
    source: &[u8],
    split: bool,
) -> Result<(), HarnessError> {
    let count = if split { range.len() } else { 1 };
    require(actual.len() == count, "Rust responsible piece count")?;
    for (i, piece) in actual.iter().enumerate() {
        let span = if split {
            range.start + i..range.start + i + 1
        } else {
            range.clone()
        };
        require(
            piece.range() == span && piece.bytes() == &source[span],
            "Rust responsible piece range/bytes",
        )?;
    }
    Ok(())
}

pub(super) fn mappings(
    actual: &[Mapping<'_>],
    expected: &[(Range<usize>, ExpectedOrigin)],
    source: &[u8],
    split: bool,
) -> Result<(), HarnessError> {
    require(actual.len() == expected.len(), "Rust full map count")?;
    for (actual, (output, origin)) in actual.iter().zip(expected) {
        require(actual.output.range() == *output, "Rust map output range")?;
        match (&actual.origin, origin) {
            (Origin::Copied(actual), ExpectedOrigin::Copied(range))
            | (Origin::Decoded(actual), ExpectedOrigin::Decoded(range)) => {
                pieces(actual, range.clone(), source, split)?;
            }
            (Origin::Synthetic, ExpectedOrigin::Synthetic) => {}
            _ => return Err(HarnessError("Rust map transformation".into())),
        }
    }
    Ok(())
}

pub(super) fn late_map(
    text: CompleteText<'_, '_>,
    n: usize,
    source: &[u8],
) -> Result<(), HarnessError> {
    let mut portions = text.origins(n - 1..n).map_err(failure)?;
    let portion = portions
        .next()
        .ok_or_else(|| HarnessError("Rust late map absent".into()))?;
    require(
        portion.output == (n - 1..n) && portions.next().is_none(),
        "Rust late map K/output",
    )?;
    let OriginView::Decoded(actual) = portion.origin else {
        return Err(HarnessError("Rust late map transformation".into()));
    };
    piece_slice(actual, 9 + 4 * (n - 1)..13 + 4 * (n - 1), source, true)
}

type Nodes<'s> = HashMap<(&'static str, usize, usize), Node<'s>>;

fn node_at<'s>(
    nodes: &Nodes<'s>,
    kind: &'static str,
    range: Range<usize>,
) -> Result<Node<'s>, HarnessError> {
    nodes
        .get(&(kind, range.start, range.end))
        .copied()
        .ok_or_else(|| HarnessError(format!("Rust canonical {kind} at {range:?} absent")))
}

fn shape<'s>(parsed: &'s Parsed, expected: &Expected) -> Result<Nodes<'s>, HarnessError> {
    let walk = parsed.nodes();
    require(
        walk.iter()
            .all(|(node, _)| !node.has_error() && !node.is_missing()),
        "Rust clean supplied tree",
    )?;
    require(
        walk.iter()
            .filter(|(node, _)| node.kind() == "mod_item")
            .count()
            == expected.depth,
        "Rust module depth",
    )?;
    require(
        walk.iter()
            .filter(|(node, _)| node.kind() == "function_item")
            .count()
            == 1,
        "Rust function count",
    )?;
    let attributes = expected
        .fragments
        .iter()
        .filter(|(_, form)| *form == FragmentForm::Attribute)
        .count();
    require(
        walk.iter()
            .filter(|(node, _)| node.kind() == "attribute_item")
            .count()
            == attributes,
        "Rust attribute width",
    )?;
    require(
        walk.iter()
            .filter(|(node, _)| matches!(node.kind(), "string_literal" | "raw_string_literal"))
            .count()
            == attributes,
        "Rust clean literal carriers",
    )?;
    let mut points = vec![Point::new(0, 0)];
    for byte in parsed.source() {
        let mut point = *points
            .last()
            .ok_or_else(|| HarnessError("Rust source point absent".into()))?;
        if *byte == b'\n' {
            point.row += 1;
            point.column = 0;
        } else {
            point.column += 1;
        }
        points.push(point);
    }
    for (node, _) in &walk {
        require(
            node.start_position() == points[node.start_byte()]
                && node.end_position() == points[node.end_byte()],
            "Rust canonical byte/point coordinates",
        )?;
    }
    let ranges = parsed.tree().included_ranges();
    require(
        ranges.len()
            == if expected.split {
                parsed.source().len()
            } else {
                1
            },
        "Rust included range count",
    )?;
    if expected.split {
        for (at, range) in ranges.iter().enumerate() {
            require(
                range.start_byte == at
                    && range.end_byte == at + 1
                    && range.start_point == points[at]
                    && range.end_point == points[at + 1],
                "Rust bytewise included coordinates",
            )?;
        }
    }
    Ok(walk
        .into_iter()
        .map(|(node, _)| ((node.kind(), node.start_byte(), node.end_byte()), node))
        .collect())
}

#[allow(clippy::too_many_lines)] // Check the complete document/association/owner graph and canonical navigation together.
fn graph(
    index: &DocumentationIndex<'_>,
    expected: &Expected,
    nodes: &Nodes<'_>,
    root: Node<'_>,
) -> Result<(), HarnessError> {
    require(
        index.documents().len() == 1 && index.associations().len() == 1,
        "Rust D/A count",
    )?;
    require(
        index.qualifications().len() == 0 && index.evidence().len() == 0,
        "Rust unexpected qualification or evidence",
    )?;
    let doc = index
        .documents()
        .next()
        .ok_or_else(|| HarnessError("Rust document absent".into()))?;
    require(
        doc.get().role() == DocumentRole::Primary
            && doc.get().composition() == Composition::AttributeSequence,
        "Rust role/composition",
    )?;
    let association = index
        .associations()
        .next()
        .ok_or_else(|| HarnessError("Rust association absent".into()))?;
    require(
        index
            .facts()
            .associated_document(association)
            .map_err(failure)?
            == doc,
        "Rust association document identity",
    )?;
    let function = node_at(nodes, "function_item", expected.function.clone())?;
    let scope = if expected.depth == 0 {
        root
    } else {
        node_at(
            nodes,
            "declaration_list",
            7 * expected.depth - 1..expected.function.end + 1,
        )?
    };
    let fact = association.get();
    require(
        fact.resolution == Resolution::Resolved
            && fact.publication == Publication::Unknown
            && fact.convention.name() == "Rust"
            && fact.owner_count() == 1,
        "Rust association disposition",
    )?;
    require(
        fact.binding.is_some_and(|node| node.node() == function)
            && fact.lexical_scope.node() == scope,
        "Rust binding/scope identity",
    )?;
    let owner = index
        .facts()
        .associated_owner(association, 0)
        .map_err(failure)?;
    require(
        matches!(owner.get(), Owner::Node(node) if node.node() == function),
        "Rust canonical function owner",
    )?;
    publication(index, expected, nodes)?;
    require(
        index.facts().owners().len() == expected.depth + 2,
        "Rust complete owner inventory",
    )?;
    let mut owners = index.facts().owners();
    require(
        owners
            .next()
            .is_some_and(|owner| matches!(owner.get(), Owner::File(node) if node.node() == root)),
        "Rust file owner identity",
    )?;
    for depth in 0..expected.depth {
        let module = node_at(nodes, "mod_item", 7 * depth..expected.source.len() - depth)?;
        require(
            owners.next().is_some_and(
                |owner| matches!(owner.get(), Owner::Node(node) if node.node() == module),
            ),
            "Rust module owner identity",
        )?;
    }
    require(
        owners.next() == Some(owner) && owners.next().is_none(),
        "Rust function owner inventory identity",
    )?;
    let query = index.node_associations(function).map_err(failure)?;
    require(
        !query.projection_pending()
            && query.associations().collect::<Vec<_>>() == [association]
            && query.assessments().collect::<Vec<_>>() == index.assessments().collect::<Vec<_>>()
            && query.qualifications().len() == 0,
        "Rust canonical query links",
    )?;
    require(
        index
            .owner_associations(owner)
            .map_err(failure)?
            .associations()
            .collect::<Vec<_>>()
            == [association],
        "Rust owner adjacency",
    )?;
    Ok(())
}

fn publication(
    index: &DocumentationIndex<'_>,
    expected: &Expected,
    nodes: &Nodes<'_>,
) -> Result<(), HarnessError> {
    let attributes = expected
        .fragments
        .iter()
        .filter(|(_, form)| *form == FragmentForm::Attribute);
    require(
        index.assessments().len() == attributes.clone().count(),
        "Rust publication assessment count",
    )?;
    for (assessment, (range, _)) in index.assessments().zip(attributes) {
        let node = node_at(nodes, "attribute_item", range.clone())?;
        require(
            assessment.get().axis == AssessmentAxis::Publication
                && assessment.get().reason
                    == "Rust documentation/publication metadata awaits interpretation"
                && assessment.get().node.is_some_and(|e| e.node() == node)
                && index
                    .assessed_document(assessment)
                    .map_err(failure)?
                    .is_none(),
            "Rust publication assessment identity",
        )?;
    }
    Ok(())
}

pub(super) fn validate(
    probe: &SourceProbe,
    n: usize,
    parsed: &Parsed,
    index: &DocumentationIndex<'_>,
) -> Result<(), HarnessError> {
    let expected = Expected::new(probe.key, n)?;
    require(
        parsed.source() == expected.source && index.source() == parsed.source(),
        "Rust exact supplied source",
    )?;
    let nodes = shape(parsed, &expected)?;
    graph(index, &expected, &nodes, parsed.tree().root_node())?;
    let doc = index
        .documents()
        .next()
        .ok_or_else(|| HarnessError("Rust document absent".into()))?;
    require(
        doc.get().fragment_count() == expected.fragments.len()
            && index.facts().fragments().len() == expected.fragments.len(),
        "Rust raw fragment count",
    )?;
    let retained: HashMap<_, _> = index
        .facts()
        .fragments()
        .map(|f| (f.get().node().map(|n| n.node().id()), f))
        .collect();
    require(
        retained.len() == expected.fragments.len(),
        "Rust unique fragment nodes",
    )?;
    for (at, (range, form)) in expected.fragments.iter().enumerate() {
        let fragment_ref = index.facts().document_fragment(doc, at).map_err(failure)?;
        require(
            retained.get(&fragment_ref.get().node().map(|n| n.node().id())) == Some(&fragment_ref),
            "Rust ordered fragment identity",
        )?;
        let fragment = fragment_ref.get();
        require(fragment.form() == *form, "Rust raw fragment form")?;
        pieces(
            fragment.source(),
            range.clone(),
            parsed.source(),
            expected.split,
        )?;
        let kind = if *form == FragmentForm::Attribute {
            "attribute_item"
        } else if parsed.source()[range.start..].starts_with(b"/**") {
            "block_comment"
        } else {
            "line_comment"
        };
        let node = fragment
            .node()
            .ok_or_else(|| HarnessError("Rust fragment node absent".into()))?
            .node();
        require(
            node == node_at(
                &nodes,
                kind,
                range.start..range.end + usize::from(kind == "line_comment"),
            )?,
            "Rust raw canonical fragment node",
        )?;
    }
    let text = index
        .complete_text(doc)
        .map_err(failure)?
        .ok_or_else(|| HarnessError("Rust complete text absent".into()))?;
    require(text.text() == expected.text, "Rust complete text bytes")?;
    mappings(
        text.mappings(),
        &expected.maps,
        parsed.source(),
        expected.split,
    )
}
