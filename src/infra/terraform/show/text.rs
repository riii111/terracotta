use crate::app::review::PlanDocument;

use super::PlanParseError;

pub(super) fn parse_document(bytes: Vec<u8>) -> Result<PlanDocument, PlanParseError> {
    String::from_utf8(bytes)
        .map(PlanDocument::new)
        .map_err(|_| PlanParseError::InvalidUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_text_order_newlines_and_sensitive_markers() {
        let source = "first\n  password = (sensitive value)\nlast\n";

        let document = parse_document(source.as_bytes().to_vec()).expect("text should parse");

        assert_eq!(document.text(), source);
        assert!(!format!("{document:?}").contains("password"));
    }

    #[test]
    fn rejects_invalid_utf8_without_exposing_bytes() {
        assert_eq!(parse_document(vec![0xff]), Err(PlanParseError::InvalidUtf8));
    }
}
