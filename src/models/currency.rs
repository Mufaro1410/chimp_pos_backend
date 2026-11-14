/// Accept an already-normalized three-letter currency code.
pub fn is_valid_currency_code(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::is_valid_currency_code;

    #[test]
    fn accepts_uppercase_three_letter_codes() {
        assert!(is_valid_currency_code("USD"));
        assert!(is_valid_currency_code("ZWL"));
    }

    #[test]
    fn rejects_lowercase_wrong_length_and_non_ascii_codes() {
        assert!(!is_valid_currency_code("usd"));
        assert!(!is_valid_currency_code("US"));
        assert!(!is_valid_currency_code("USDD"));
        assert!(!is_valid_currency_code("UŞD"));
    }
}
