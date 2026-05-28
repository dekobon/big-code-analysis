// Code generated; DO NOT EDIT.

const {{ u_name }}: &[&str] = &[
    {% for name in names -%}
    "{{ name }}",
    {% endfor %}
];

pub(crate) fn is_{{ l_name }}(mac: &str) -> bool {
    {{ u_name }}.binary_search(&mac).is_ok()
}

#[cfg(test)]
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
    use super::*;

    #[test]
    fn {{ l_name }}_is_sorted() {
        assert!(
            {{ u_name }}.is_sorted(),
            "{{ u_name }} must be sorted for binary_search to work"
        );
    }

    #[test]
    fn {{ l_name }}_lookup() {
        // Smoke test using literal entries from the codegen
        // input file (rendered at template-substitution time so
        // a reviewer reads concrete names rather than indices).
        assert!(is_{{ l_name }}("{{ names[0] }}"));
        assert!(is_{{ l_name }}("{{ names[1] }}"));
        assert!(!is_{{ l_name }}("not-a-real-entry"));
        assert!(!is_{{ l_name }}(""));
    }

    #[test]
    fn {{ l_name }}_lookup_boundaries() {
        let first = {{ u_name }}.first().expect("non-empty list");
        let last = {{ u_name }}.last().expect("non-empty list");
        assert!(
            is_{{ l_name }}(first),
            "first entry {first} must be findable"
        );
        assert!(
            is_{{ l_name }}(last),
            "last entry {last} must be findable"
        );
        // Lexicographically below the first entry and above the last.
        assert!(!is_{{ l_name }}("\u{1}"));
        assert!(!is_{{ l_name }}("zzzzz_unknown"));
    }
}
