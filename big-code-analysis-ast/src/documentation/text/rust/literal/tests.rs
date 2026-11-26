use super::*;
use crate::documentation::{FragmentForm, SourceError};
use crate::{AnyParser, LANG};
use std::cell::Cell;

#[cfg(feature = "rust")]
#[derive(Default)]
struct CountingBudget {
    bytes: Cell<usize>,
    steps: Cell<usize>,
    checkpoints: Cell<usize>,
    fail_byte: Option<usize>,
}

#[cfg(feature = "rust")]
impl WorkBudget for CountingBudget {
    type Error = SourceError;

    fn checkpoint(&self) -> Result<(), SourceError> {
        self.checkpoints.set(self.checkpoints.get() + 1);
        Ok(())
    }

    fn step(&self) -> Result<(), SourceError> {
        self.steps.set(self.steps.get() + 1);
        Ok(())
    }

    #[cfg(feature = "rust")]
    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.bytes.set(self.bytes.get() + count);
        if self.fail_byte.is_some_and(|at| self.bytes.get() >= at) {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "rust")]
impl CountingBudget {
    #[cfg(feature = "rust")]
    fn counts(&self) -> (usize, usize, usize) {
        (self.bytes.get(), self.steps.get(), self.checkpoints.get())
    }
}

#[cfg(feature = "rust")]
fn parsed_literal(body: &str, check: impl FnOnce(&SourceContext<'_>, &Fragment<'_>, Range<usize>)) {
    let source = format!("#[doc = \"{body}\"] fn f() {{}}");
    let parser = AnyParser::parse(LANG::Rust, source.as_bytes().to_vec(), None, None).unwrap();
    let root = parser.ts_tree().root_node();
    assert!(!root.has_error());
    let item = root.named_child(0).unwrap();
    assert_eq!(item.kind(), "attribute_item");
    let attribute = item.named_child(0).unwrap();
    assert_eq!(attribute.kind(), "attribute");
    assert!(attribute.child_by_field_name("arguments").is_none());
    let value = attribute.child_by_field_name("value").unwrap();
    assert_eq!(value.kind(), "string_literal");
    assert_eq!(value.byte_range(), 8..10 + body.len());
    assert!(!value.has_error() && !value.is_missing());
    let context = SourceContext::new(parser.ts_tree(), parser.code());
    let fragment = Fragment {
        source: context.pieces(item.byte_range()).unwrap(),
        node: Some(context.node(item).unwrap()),
        form: FragmentForm::Attribute,
    };
    check(&context, &fragment, value.byte_range());
}

#[test]
#[cfg(feature = "rust")]
fn continuation_whitespace_is_charged_before_any_append() {
    for size in [8, 32, 128] {
        for (ending, prefix_bytes) in [("\n", 2), ("\r\n", 3)] {
            for (unit, width) in [(" ", 1), (" \t\n\r\n", 5)] {
                let body = format!("\\{ending}{}", unit.repeat(size));
                parsed_literal(&body, |context, fragment, range| {
                    let budget = CountingBudget {
                        fail_byte: Some(prefix_bytes + 1),
                        ..CountingBudget::default()
                    };
                    let copies = crate::documentation::text::copy_chunks::observed();
                    assert!(matches!(
                        decode(context, fragment, range.clone(), &budget),
                        Err(TextError::Source(SourceError::Cancelled))
                    ));
                    // expected: backslash, LF/CR, optional LF, then the first whitespace byte.
                    assert_eq!(budget.counts(), (prefix_bytes + 1, 0, 0));
                    assert_eq!(
                        crate::documentation::text::copy_chunks::observed() - copies,
                        0
                    );

                    let budget = CountingBudget::default();
                    let decoded = decode(context, fragment, range, &budget).unwrap();
                    assert_eq!(decoded.text, "");
                    assert_eq!(decoded.mappings, []);
                    // expected: all physical continuation whitespace is consumed once, without output.
                    assert_eq!(budget.counts(), (prefix_bytes + width * size, 0, 0));
                    assert_eq!(
                        crate::documentation::text::copy_chunks::observed() - copies,
                        0
                    );
                });
            }
        }
    }
}

#[test]
#[cfg(feature = "rust")]
fn unicode_digits_are_charged_before_piece_gathering_and_append() {
    for size in [1, 8, 32, 128] {
        parsed_literal(&"\\u{1F600}".repeat(size), |context, fragment, range| {
            let budget = CountingBudget {
                fail_byte: Some(3),
                ..CountingBudget::default()
            };
            let copies = crate::documentation::text::copy_chunks::observed();
            assert!(matches!(
                decode(context, fragment, range.clone(), &budget),
                Err(TextError::Source(SourceError::Cancelled))
            ));
            // expected: backslash + u + first digit; no source-piece step or append checkpoint yet.
            assert_eq!(budget.counts(), (3, 0, 0));
            assert_eq!(
                crate::documentation::text::copy_chunks::observed() - copies,
                0
            );

            let budget = CountingBudget::default();
            let decoded = decode(context, fragment, range, &budget).unwrap();
            assert_eq!(decoded.text, "😀".repeat(size));
            assert_eq!(decoded.mappings.len(), size);
            // expected per token: 2 outer bytes + 5 digits + } + two four-byte append charges.
            // One retained source piece is examined; the append makes two checkpoints and one copy.
            assert_eq!(budget.counts(), (16 * size, size, 2 * size));
            assert_eq!(
                crate::documentation::text::copy_chunks::observed() - copies,
                size
            );
            for (at, mapping) in decoded.mappings.iter().enumerate() {
                assert_eq!(mapping.output.range(), 4 * at..4 * at + 4);
                let Origin::Decoded(pieces) = &mapping.origin else {
                    panic!("Unicode token origin")
                };
                assert_eq!(pieces.pieces().len(), 1);
                assert_eq!(pieces.pieces()[0].range(), 9 + 9 * at..18 + 9 * at);
                assert_eq!(pieces.pieces()[0].bytes(), b"\\u{1F600}");
            }
        });
    }
}
