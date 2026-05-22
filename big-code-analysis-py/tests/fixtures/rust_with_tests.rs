fn prod(x: i32) -> i32 {
    if x > 0 { x + 1 } else { -x }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn helper() -> i32 {
        42
    }

    #[test]
    fn checks_positive() {
        assert_eq!(prod(1), 2);
        assert_eq!(helper(), 42);
    }

    #[test]
    fn checks_negative() {
        assert_eq!(prod(-1), 1);
    }
}
