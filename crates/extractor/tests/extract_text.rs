use extractor::extract_text;
use std::path::Path;

#[test]
fn extract_text_returns_nonempty_string_from_pdf() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal.pdf");
    let text = extract_text(&fixture).expect("extract_text should succeed");
    assert!(!text.trim().is_empty(), "extracted text should not be empty");
    assert!(text.contains("Hello World"), "extracted text should contain 'Hello World'");
}
