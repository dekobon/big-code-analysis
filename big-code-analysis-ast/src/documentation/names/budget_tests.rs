use super::*;
use std::cell::Cell;

#[derive(Default)]
struct Counting {
    bytes: Cell<usize>,
    stop: Option<usize>,
}

impl WorkBudget for Counting {
    type Error = SourceError;
    fn checkpoint(&self) -> Result<(), SourceError> {
        Ok(())
    }
    fn step(&self) -> Result<(), SourceError> {
        Ok(())
    }
    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.bytes.set(self.bytes.get() + count);
        if self.stop.is_some_and(|limit| self.bytes.get() >= limit) {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[test]
fn tokenizer_accounts_for_every_consumed_delimiter_and_token_byte() {
    for source in [
        "/**/".repeat(8000) + "void f()",
        "/*".to_owned() + &"x".repeat(40_000) + "*/",
        "a".repeat(40_000),
        " && :: \t\n".repeat(4000),
        "/*unterminated".to_owned(),
    ] {
        let budget = Counting::default();
        let result = tokens::tokens(source.as_bytes(), &budget).unwrap();
        assert_eq!(budget.bytes.get(), source.len());
        assert_eq!(result.is_none(), source == "/*unterminated");
    }
}

#[test]
fn tokenizer_cancels_inside_long_identifier_comment_and_trivia() {
    for source in [
        "a".repeat(40_000),
        "/*".to_owned() + &"x".repeat(40_000) + "*/",
        "/**/".repeat(10_000),
    ] {
        let budget = Counting {
            stop: Some(16 * 1024),
            ..Counting::default()
        };
        assert!(matches!(
            tokens::tokens(source.as_bytes(), &budget),
            Err(SourceError::Cancelled)
        ));
        assert_eq!(budget.bytes.get(), 16 * 1024);
        assert!(
            tokens::tokens(b"void f()", &Counting::default())
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn key_output_charges_separators_and_cancels_within_emission() {
    let hasher = RandomState::new();
    let input = vec![b"a".as_slice(); 20_000];
    let budget = Counting::default();
    let (_, key) = emit_key(&input, &hasher, &budget).unwrap();
    assert_eq!(key.len(), 39_999);
    assert_eq!(budget.bytes.get(), 2 * key.len());
    let cancel = Counting {
        stop: Some(16 * 1024),
        ..Counting::default()
    };
    assert!(matches!(
        emit_key(&input, &hasher, &cancel),
        Err(SourceError::Cancelled)
    ));
    assert_eq!(cancel.bytes.get(), 16 * 1024);
    let long = b"a".repeat(40_000);
    let cancel = Counting {
        stop: Some(16 * 1024),
        ..Counting::default()
    };
    assert!(matches!(
        emit_key(&[&long], &hasher, &cancel),
        Err(SourceError::Cancelled)
    ));
    assert_eq!(cancel.bytes.get(), 16 * 1024);
}

#[test]
fn ordinary_and_complex_candidates_intern_name_once() {
    let mut names = Names::default();
    let before = key_bytes_stored::observed();
    for ordinal in 0..64 {
        names
            .insert_candidate(
                b"f",
                (ordinal % 2 == 0).then_some(b"void f()".as_slice()),
                NodeId(0),
                NamedOwner {
                    owner: OwnerId(ordinal),
                    qualification: QualificationSet::default(),
                },
                &super::super::budget::NoBudget,
            )
            .unwrap();
    }
    // f: 1 byte, and void\0f\0(\0): 10 bytes; candidates never duplicate spelling.
    assert_eq!(key_bytes_stored::observed() - before, 11);
    assert_eq!(names.identifiers.values().map(Vec::len).sum::<usize>(), 1);
    let entry = &names.identifiers.values().next().unwrap()[0];
    assert_eq!(entry.owners.len(), 64);
    assert_eq!(entry.complex.len(), 32);
    let before = name_candidates::observed();
    let result = names
        .resolve_in(b"void f()", NodeId(0), &super::super::budget::NoBudget)
        .unwrap();
    assert_eq!(result.owners.len(), 64);
    assert!(result.pending);
    assert_eq!(result.reason, Some(UnresolvedReason::Ambiguous));
    assert_eq!(name_candidates::observed() - before, 2);
    let before = key_bytes_stored::observed();
    names
        .insert_candidate(
            b"f",
            None,
            NodeId(1),
            NamedOwner {
                owner: OwnerId(64),
                qualification: QualificationSet::default(),
            },
            &super::super::budget::NoBudget,
        )
        .unwrap();
    assert_eq!(key_bytes_stored::observed() - before, 1);
    assert_eq!(
        names
            .resolve_in(b"f", NodeId(1), &super::super::budget::NoBudget)
            .unwrap()
            .owners
            .len(),
        1
    );
}

#[test]
fn builtin_and_keyword_inventory_has_explicit_positive_and_negative_cases() {
    let builtins = [
        "void",
        "bool",
        "_Bool",
        "char",
        "signed char",
        "unsigned char",
        "wchar_t",
        "char8_t",
        "char16_t",
        "char32_t",
        "short",
        "short int",
        "signed short",
        "signed short int",
        "unsigned short",
        "unsigned short int",
        "int",
        "signed",
        "signed int",
        "unsigned",
        "unsigned int",
        "long",
        "long int",
        "signed long",
        "signed long int",
        "unsigned long",
        "unsigned long int",
        "long long",
        "long long int",
        "signed long long",
        "signed long long int",
        "unsigned long long",
        "unsigned long long int",
        "float",
        "double",
        "long double",
    ];
    let budget = Counting::default();
    for base in builtins {
        let signature = format!("{base} f()");
        let tokens = tokens::tokens(signature.as_bytes(), &budget)
            .unwrap()
            .unwrap();
        assert_eq!(
            tokens::reference_name(&tokens, &budget).unwrap(),
            Some(b"f".as_slice()),
            "{base}"
        );
    }
    for bad in [
        "unsigned double",
        "short long",
        "long long long",
        "signed unsigned",
        "void int",
        "int int",
        "int unsigned",
        "auto",
        "class",
        "typename",
        "_Atomic",
        "decltype",
        "static",
        "12",
    ] {
        let signature = format!("{bad} f()");
        let tokens = tokens::tokens(signature.as_bytes(), &budget)
            .unwrap()
            .unwrap();
        assert_eq!(
            tokens::reference_name(&tokens, &budget).unwrap(),
            None,
            "{bad}"
        );
    }
}
