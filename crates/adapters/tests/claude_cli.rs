use blitzline_adapters::pipeline::{self as rt, Runtime, config};
use blitzline_engine::pipeline::Pipeline;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../tests/fixtures/reference/text.json")).unwrap()
}
/// A stand-in for `claude -p`. MODE is "good", "repair" (first answer invalid), "exit", "error" or "sleep".
const FAKE: &str = r#"
import json, os, sys, time
out, mode = sys.argv[1], sys.argv[2]
stdin = sys.stdin.read()
n = len([f for f in os.listdir(out) if f.startswith("call")])
open(f"{out}/call{n}.json", "w").write(json.dumps({"argv": sys.argv[3:], "env": sorted(os.environ), "stdin": stdin, "cwd": os.getcwd()}))
if mode == "exit":
    sys.stderr.write("private transcript"); sys.exit(1)
if mode == "sleep":
    time.sleep(30)
if mode == "error":
    print(json.dumps({"is_error": True, "subtype": "success", "result": "private transcript"})); sys.exit(0)
ids = json.loads(stdin)["requested_ids"]
rows = [r for r in json.load(open(f"{out}/decisions.json")) if r["candidate_id"] in ids]
if mode == "repair" and n == 0:
    rows[1]["english"] = ""
print(json.dumps({"is_error": False, "subtype": "success", "structured_output": {"decisions": rows}}))
"#;

struct Fake {
    dir: tempfile::TempDir,
}
impl Fake {
    fn out(&self) -> std::path::PathBuf {
        self.dir.path().join("out")
    }
    fn calls(&self) -> Vec<Value> {
        let mut files: Vec<_> = fs::read_dir(self.out())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("call"))
            .collect();
        files.sort();
        files.iter().map(|p| serde_json::from_slice(&fs::read(p).unwrap()).unwrap()).collect()
    }
}
fn setup(mode: &str, timeout: u64) -> (Fake, Pipeline<Runtime>) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("out")).unwrap();
    fs::write(dir.path().join("fake.py"), FAKE).unwrap();
    fs::write(dir.path().join("out/decisions.json"), fixture()["decisions"].to_string()).unwrap();
    let root = dir.path();
    let source = root.join("sample.txt");
    fs::write(&source, fixture()["raw"].as_str().unwrap()).unwrap();
    let vocab = root.join("vocabulary.json");
    fs::write(&vocab, fixture()["rows"].to_string()).unwrap();
    let command = json!(["python3", "-I", root.join("fake.py"), root.join("out"), mode]);
    let raw = json!({"profiles":{"cli":{"provider":"claude-cli","model":"sonnet","command":command,"timeout":timeout}},"llm":{"default":"cli"},"run":{"method":"api"}});
    let mut p = rt::start(
        &root.join("run"),
        &source,
        "slv",
        "text",
        config::merge(raw, root).unwrap(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    p.backend.verbose = false;
    p.job.request.vocabulary_json = Some(vocab.to_string_lossy().into());
    (Fake { dir }, p)
}

#[test]
fn cli_repairs_bad_items_with_the_transcript_on_stdin_only_and_a_clean_environment() {
    // SAFETY: only this test touches the variable, and nothing else in this binary reads it.
    unsafe { std::env::set_var("ANTHROPIC_API_KEY", "MUST-NOT-REACH-THE-CLI") };
    let (fake, mut p) = setup("repair", 60);
    p.process(None).unwrap();
    assert_eq!(p.job.cards, 2);
    let calls = fake.calls();
    assert_eq!(calls.len(), 2);
    for call in &calls {
        let argv = call["argv"].as_array().unwrap();
        let joined = argv.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ");
        for flag in ["-p", "--tools", "--strict-mcp-config", "--setting-sources", "--disable-slash-commands", "--no-session-persistence", "--json-schema", "--model sonnet"] {
            assert!(joined.contains(flag), "missing {flag}");
        }
        assert!(!argv.iter().any(|a| a == "--bare"));
        // The candidates are request data; they must never be placed in arguments.
        let data: Value = serde_json::from_str(call["stdin"].as_str().unwrap()).unwrap();
        let term = data["candidates"][0]["term"].as_str().unwrap();
        assert!(!joined.contains(term));
        let env = call["env"].as_array().unwrap();
        assert!(!env.iter().any(|k| k.as_str().unwrap().starts_with("ANTHROPIC_")));
        assert_eq!(Path::new(call["cwd"].as_str().unwrap()).canonicalize().unwrap(), std::env::temp_dir().canonicalize().unwrap());
    }
    assert!(!calls[0]["argv"].to_string().contains("--effort"));
    let second: Value = serde_json::from_str(calls[1]["stdin"].as_str().unwrap()).unwrap();
    assert_eq!(second["requested_ids"].as_array().unwrap().len(), 1);
}

#[test]
fn cli_failures_are_reported_without_echoing_output() {
    for mode in ["exit", "error"] {
        let (_fake, mut p) = setup(mode, 60);
        let error = p.process(None).unwrap_err();
        assert_eq!(error.code, "transport_failed", "{mode}");
        assert!(!error.message.contains("private"), "{mode}");
        assert!(p.job.pending.is_some());
    }
}

#[test]
fn cli_times_out_and_can_be_cancelled() {
    let (_fake, mut p) = setup("sleep", 1);
    let start = Instant::now();
    let error = p.process(None).unwrap_err();
    assert_eq!(error.code, "transport_failed");
    assert!(start.elapsed() < Duration::from_secs(15));

    let (_fake, mut p) = setup("sleep", 60);
    let cancel = p.backend.cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let start = Instant::now();
    assert_eq!(p.process(None).unwrap_err().code, "paused");
    assert!(start.elapsed() < Duration::from_secs(15));
}

#[test]
fn claude_cli_profile_validation() {
    let ok = |profile: Value| config::merge(json!({"profiles":{"c":profile}}), Path::new(".")).map(|s| s["profiles"]["c"].clone());
    let profile = ok(json!({"provider":"claude-cli","model":"opus"})).unwrap();
    assert_eq!(profile["command"], json!(["claude"]));
    assert!(ok(json!({"provider":"claude-cli","model":"gpt-4"})).is_err());
    assert!(ok(json!({"provider":"claude-cli","model":"haiku","command":[]})).is_err());
    // Other providers are unchanged, so their cached answers stay valid.
    assert!(ok(json!({"provider":"openrouter","model":"m"})).unwrap().get("command").is_none());
}

#[test]
fn runs_saved_before_the_anki_settings_still_validate() {
    let mut s = config::merge(json!({}), Path::new(".")).unwrap();
    for key in ["anki_collection", "anki_deck", "anki_fields"] {
        s["blitzer"].as_object_mut().unwrap().remove(key);
    }
    config::validate(&s).unwrap();
}

#[test]
fn effort_is_passed_only_when_set_and_must_be_valid() {
    let ok = |effort: &str| config::merge(json!({"profiles":{"c":{"provider":"claude-cli","model":"haiku","effort":effort}}}), Path::new(".")).map(|s| s["profiles"]["c"].clone());
    assert_eq!(ok("low").unwrap()["effort"], "low");
    assert!(ok("").is_ok());
    assert!(ok("turbo").is_err());
}
