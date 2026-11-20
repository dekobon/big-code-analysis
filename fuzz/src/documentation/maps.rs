use big_code_analysis_ast::documentation::*;
use std::ops::Range;

use super::{Check, MAX_QUERIES, require};

pub(super) struct Coverage<'s> {
    source: &'s [u8],
    excluded: Vec<usize>,
}

impl<'s> Coverage<'s> {
    pub fn new(tree: &tree_sitter::Tree, source: &'s [u8]) -> Self {
        let ranges = tree.included_ranges();
        let mut at = 0;
        let mut excluded = Vec::with_capacity(source.len() + 1);
        excluded.push(0);
        for byte in 0..source.len() {
            while ranges.get(at).is_some_and(|range| range.end_byte <= byte) {
                at += 1;
            }
            let included = ranges.get(at).is_some_and(|range| range.start_byte <= byte);
            excluded.push(excluded[byte] + usize::from(!included));
        }
        Self { source, excluded }
    }

    pub fn piece(&self, piece: &SourcePiece<'_>) -> Check {
        let range = piece.range();
        require(
            range.start < range.end && range.end <= self.source.len(),
            "piece bounds",
        )?;
        require(
            self.source.get(range.clone()) == Some(piece.bytes()),
            "piece bytes",
        )?;
        require(
            self.excluded[range.start] == self.excluded[range.end],
            "piece crosses excluded bytes",
        )
    }

    pub fn pieces(&self, pieces: &[SourcePiece<'_>]) -> Check {
        require(!pieces.is_empty(), "empty raw spelling")?;
        let mut end = 0;
        for piece in pieces {
            self.piece(piece)?;
            require(end <= piece.range().start, "piece source order")?;
            end = piece.range().end;
        }
        Ok(())
    }
}

pub(super) fn validate(complete: CompleteText<'_, '_>, coverage: &Coverage<'_>) -> Check {
    let mut end = 0;
    for mapping in complete.mappings() {
        let range = mapping.output.range();
        require(
            range.start == end && range.start < range.end && range.end <= complete.text().len(),
            "map must partition output",
        )?;
        match &mapping.origin {
            Origin::Copied(pieces) => {
                coverage.pieces(pieces.pieces())?;
                require(
                    pieces.pieces().len() == 1,
                    "copied map run must be contiguous",
                )?;
                require(
                    complete.text().as_bytes().get(range.clone())
                        == Some(pieces.pieces()[0].bytes()),
                    "copied output differs from source",
                )?;
            }
            Origin::Decoded(pieces) | Origin::Expanded(pieces) => {
                coverage.pieces(pieces.pieces())?
            }
            Origin::Synthetic => {}
            _ => return Err("unhandled origin variant"),
        }
        // Stored runs can divide a scalar when included source pieces divide it.
        end = range.end;
    }
    require(
        end == complete.text().len(),
        "map does not cover complete text",
    )
}

pub(super) fn queries(complete: CompleteText<'_, '_>, selector: u8) -> Check {
    let text = complete.text();
    let len = text.len();
    let boundaries: Vec<_> = text.char_indices().map(|(at, _)| at).chain([len]).collect();
    let chosen = usize::from(selector) % boundaries.len();
    let start = boundaries[chosen];
    let end = boundaries.get(chosen + 1).copied().unwrap_or(len);
    let late = boundaries[boundaries.len().saturating_sub(2)];
    let split = text
        .char_indices()
        .find(|(_, scalar)| scalar.len_utf8() > 1)
        .map_or(len + 1, |(at, _)| at + 1);
    let ranges: [Range<usize>; MAX_QUERIES] = [
        0..len,
        len..len,
        Range { start: 1, end: 0 },
        0..len + 1,
        split..split,
        start..end,
        late..len,
        0..0,
    ];
    for query in ranges {
        let expected = if query.start > query.end || query.end > len {
            Err(TextRangeError::Bounds)
        } else if !text.is_char_boundary(query.start) || !text.is_char_boundary(query.end) {
            Err(TextRangeError::ScalarBoundary)
        } else {
            Ok(())
        };
        match (expected, complete.origins(query.clone())) {
            (Err(expected), Err(actual)) => {
                require(expected == actual, "wrong map query rejection")?
            }
            (Ok(()), Ok(mut actual)) => {
                if !query.is_empty() {
                    for mapping in complete.mappings() {
                        let range = mapping.output.range();
                        let intersection = range.start.max(query.start)..range.end.min(query.end);
                        if intersection.start >= intersection.end {
                            continue;
                        }
                        let portion = actual.next().ok_or("missing map query portion")?;
                        require(portion.output == intersection, "map query clipping")?;
                        match (&mapping.origin, portion.origin) {
                            (Origin::Copied(pieces), OriginView::Copied(piece)) => {
                                let start = pieces.pieces()[0].range().start + intersection.start
                                    - range.start;
                                require(
                                    piece.range() == (start..start + intersection.len()),
                                    "copied query source offset",
                                )?;
                                require(
                                    piece.bytes() == &text.as_bytes()[intersection],
                                    "copied query bytes",
                                )?;
                            }
                            (Origin::Decoded(pieces), OriginView::Decoded(actual))
                            | (Origin::Expanded(pieces), OriginView::Expanded(actual)) => require(
                                pieces.pieces() == actual,
                                "transformed query lost token pieces",
                            )?,
                            (Origin::Synthetic, OriginView::Synthetic) => {}
                            _ => return Err("query changed origin kind"),
                        }
                    }
                }
                require(actual.next().is_none(), "extra map query portion")?;
                require(actual.next().is_none(), "map iterator is not fused")?;
            }
            _ => return Err("map query acceptance mismatch"),
        }
    }
    Ok(())
}
