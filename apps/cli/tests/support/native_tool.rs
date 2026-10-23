//! Compiled subprocess fixture, enabled only by the test-tools development feature.
use serde_json::Value;
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    for flag in [
        "--freq",
        "--lemmatize",
        "--context",
        "--no-prompt",
        "--no-src",
        "--no-update-known",
        "--no-save-context",
        "--no-config",
    ] {
        assert!(
            args.iter().any(|argument| argument == flag),
            "missing {flag}"
        );
    }
    for pair in [
        ["--language", "slv"],
        ["--bold", "html"],
        ["--format", "json"],
    ] {
        assert!(args.windows(2).any(|arguments| arguments == pair));
    }
    let input = &args[args
        .iter()
        .position(|argument| argument == "--file")
        .unwrap()
        + 1];
    assert_eq!(
        std::fs::read_to_string(input).unwrap(),
        "Mačka spi. Café čaka. Mačka spi."
    );
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/reference/text.json"
    ))
    .unwrap();
    println!("{}", fixture["rows"]);
}
