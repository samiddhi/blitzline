use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn cli(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_blitzline"))
        .args(arguments)
        // A working text pipeline must not rely on interpreters or helper tools.
        .env("PATH", root.join("empty-path"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env_remove("BLITZLINE_CONFIG")
        .current_dir(root)
        .output()
        .unwrap()
}

#[test]
fn standalone_binary_completes_browser_review_and_both_portable_exports() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let fixture: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/reference/text.json")).unwrap();
    fs::write(root.join("source.txt"), fixture["raw"].as_str().unwrap()).unwrap();
    fs::write(root.join("vocabulary.json"), fixture["rows"].to_string()).unwrap();
    fs::write(root.join("config.toml"), "").unwrap();
    let output = cli(
        root,
        &[
            "--json",
            "run",
            "source.txt",
            "--text-only",
            "--language",
            "slv",
            "--config",
            "config.toml",
            "--method",
            "browser",
            "--vocabulary-json",
            "vocabulary.json",
            "--output",
            "run",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let job: Value =
        serde_json::from_slice(&fs::read(root.join("run/manifest.json")).unwrap()).unwrap();
    assert_eq!(job["state"], "needs-review");
    let prompt = cli(root, &["--json", "prompt", "run"]);
    assert!(prompt.status.success());
    let prompt: Value = serde_json::from_slice(&prompt.stdout).unwrap();
    assert!(prompt["prompt"].as_str().unwrap().contains("exchange_id"));
    fs::write(root.join("reply.json"), json!({"exchange_id": job["pending"]["id"], "revision": job["revision"], "decisions": fixture["decisions"]}).to_string()).unwrap();
    assert!(
        cli(root, &["submit", "run", "--file", "reply.json"])
            .status
            .success()
    );
    // Submitting a reply never silently runs the next stage.
    assert!(!root.join("run/exports/tsv/cards.tsv").exists());
    let resumed = cli(root, &["resume", "run"]);
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    assert!(root.join("run/exports/tsv/cards.tsv").is_file());
    let exported = cli(root, &["export", "run", "--format", "apkg"]);
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    assert!(root.join("run/exports/apkg/cards.apkg").is_file());
    assert!(!root.join("run/media").exists());
    let preview = cli(root, &["clean", "run"]);
    assert!(preview.status.success());
    assert!(root.join("run/artifacts").exists());
    assert!(cli(root, &["clean", "run", "--apply"]).status.success());
    assert!(!cli(root, &["resume", "run"]).status.success());
    assert!(root.join("run/exports/apkg/cards.apkg").is_file());
}

#[test]
fn invalid_text_media_flags_fail_before_creating_a_job() {
    let directory = tempfile::tempdir().unwrap();
    let output = cli(
        directory.path(),
        &[
            "run",
            "missing.txt",
            "--text-only",
            "--language",
            "slv",
            "--clips",
            "audio",
            "--output",
            "run",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(!directory.path().join("run").exists());
}

#[cfg(feature = "test-tools")]
#[test]
fn native_blitzer_contract_runs_without_an_interpreter() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(
        root.join("source.txt"),
        "Mačka spi.\n\nCafé čaka. Mačka spi.",
    )
    .unwrap();
    let executable =
        serde_json::to_string(env!("CARGO_BIN_EXE_blitzline-contract-fixture")).unwrap();
    fs::write(
        root.join("config.toml"),
        format!("[blitzer]\nexecutable = {executable}\nno_config = true\n"),
    )
    .unwrap();
    let output = cli(
        root,
        &[
            "run",
            "source.txt",
            "--text-only",
            "--language",
            "slv",
            "--config",
            "config.toml",
            "--method",
            "browser",
            "--output",
            "run",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let vocabulary: Value =
        serde_json::from_slice(&fs::read(root.join("run/artifacts/04-vocabulary.json")).unwrap())
            .unwrap();
    assert_eq!(vocabulary.as_array().unwrap().len(), 2);
    assert_eq!(
        fs::read_to_string(root.join("source.txt")).unwrap(),
        "Mačka spi.\n\nCafé čaka. Mačka spi."
    );
}
