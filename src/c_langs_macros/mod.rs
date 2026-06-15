mod c_macros;
pub(crate) use c_macros::*;

mod c_specials;
pub(crate) use c_specials::*;

// Regression tests pinning specific bogus entries that the
// codegen-generated `*_lookup` smoke tests cannot cover (those are
// rendered generically from `names[0]`/`names[1]`). These guard the
// pruning of non-existent standard names from the generated tables;
// see dekobon/big-code-analysis#760 (unsigned types have no `*_MIN`
// macro — the minimum of any unsigned type is 0) and #762
// (`char64_t`/`charptr_t` are not real C/C++ types).
#[cfg(test)]
mod prune_regression_tests {
    use super::*;

    // ISO C defines `UINTN_MAX` but no `UINTN_MIN`: the minimum of an
    // unsigned type is always 0, so no such macro exists (#760).
    #[test]
    fn unsigned_min_macros_are_not_predefined() {
        for name in [
            "UINT16_MIN",
            "UINT32_MIN",
            "UINT64_MIN",
            "UINT8_MIN",
            "UINTMAX_MIN",
            "UINTPTR_MIN",
            "UINT_FAST8_MIN",
            "UINT_FAST16_MIN",
            "UINT_FAST32_MIN",
            "UINT_FAST64_MIN",
            "UINT_LEAST8_MIN",
            "UINT_LEAST16_MIN",
            "UINT_LEAST32_MIN",
            "UINT_LEAST64_MIN",
        ] {
            assert!(
                !is_predefined_macros(name),
                "{name} is not a real C standard macro and must not be predefined"
            );
        }
    }

    // The signed `INT*_MIN` macros are genuine and must stay (#760).
    #[test]
    fn signed_min_macros_remain_predefined() {
        for name in ["INT16_MIN", "INTMAX_MIN", "INTPTR_MIN", "INT_FAST8_MIN"] {
            assert!(
                is_predefined_macros(name),
                "{name} is a real C standard macro and must stay predefined"
            );
        }
        // The unsigned `*_MAX` macros are genuine too.
        assert!(is_predefined_macros("UINT16_MAX"));
    }

    // `char64_t` and `charptr_t` are not real C/C++ types and the
    // generator never emits them (#762).
    #[test]
    fn bogus_special_types_are_not_specials() {
        assert!(!is_specials("char64_t"));
        assert!(!is_specials("charptr_t"));
    }

    // The genuine fixed-width character types must stay (#762).
    #[test]
    fn real_char_types_remain_specials() {
        assert!(is_specials("char8_t"));
        assert!(is_specials("char16_t"));
        assert!(is_specials("char32_t"));
    }
}

// These samples exercise the Mozilla/Gecko macro overlay
// (`MOZ_*`, `QM_TRY_*`, `nsPrintfCString`, …). Since #720 that overlay
// lives only in the opt-in `Mozcpp` grammar — upstream `tree-sitter-cpp`
// (now backing `LANG::Cpp`) ERROR-cascades on these forms — so the
// parse helper drives `MozcppParser`, and the module is gated on the
// `mozcpp` feature.
#[cfg(all(test, feature = "mozcpp"))]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {

    use std::path::PathBuf;

    use crate::*;

    fn parse(samples: &[&str], debug: bool) {
        let path = PathBuf::from("foo.cpp");
        for (n, sample) in samples.iter().enumerate() {
            let v_sample = sample.as_bytes().to_vec();
            let parser = MozcppParser::new(v_sample.clone(), &path, None);
            let root = parser.root();
            if debug || root.has_error() {
                eprintln!("Sample (MOZCPP) {n}: {sample}");
                dump_node(&v_sample, &root, -1, None, None).unwrap();
            }
            assert!(!root.has_error());
        }
    }

    #[test]
    fn test_fn_macros() {
        let samples = vec![
            "MOZ_ALWAYS_INLINE void f() { }",
            "MOZ_NEVER_INLINE void f() { }",
        ];
        parse(&samples, false);
    }

    #[test]
    fn test_fn_macros_cpp() {
        let samples = vec!["class MOZ_NONHEAP_CLASS Factory : public IClassFactory {};"];
        parse(&samples, false);
    }

    #[test]
    #[ignore = "FIXME: parse error in nsPrintfCString sample (see dekobon/big-code-analysis#83)"]
    fn test_fn_id_strings() {
        let samples = vec!["nsPrintfCString(\"%\" PRIi32, lifetime.mTag);"];
        parse(&samples, false);
    }

    #[test]
    fn test_fn_qm_try_inspect_cpp() {
        let samples = vec![
            "QM_TRY_INSPECT(const int32_t& storageVersion, MOZ_TO_RESULT_INVOKE(aConnection, GetSchemaVersion));",
        ];
        parse(&samples, false);
    }
}
