use blitzline_adapters::*;
use blitzline_domain::*;
use blitzline_engine::{Engine, Repository};
#[test]
fn atomic_roundtrip_and_exclusive_writer() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(temp.path().into()).unwrap();
    assert!(Store::open(temp.path().into()).is_err());
    let mut engine = Engine::new(store.load().unwrap());
    engine.example().unwrap();
    engine.state.setup.pasted_text = "unfinished text".into();
    store.save(&engine.state).unwrap();
    assert_eq!(store.load().unwrap(), engine.state);
    drop(store);
    assert!(Store::open(temp.path().into()).is_ok());
}
#[test]
fn corrupt_state_is_never_silently_replaced() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("workspace.json"), "{broken").unwrap();
    let store = Store::open(temp.path().into()).unwrap();
    assert!(store.load().is_err());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("workspace.json")).unwrap(),
        "{broken"
    );
}
#[test]
fn future_schema_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(temp.path().into()).unwrap();
    store
        .save(&Workspace {
            schema: 99,
            ..Default::default()
        })
        .unwrap();
    assert!(store.load().is_err());
}
#[test]
fn import_reports_unsupported_settings_and_ignores_credentials() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let raw = "[decks.slovenian]\nlanguage='slv'\ndeck='Study'\n[asr]\nbackend='modal'\n[profiles.test]\napi_key='SECRET_NEVER_IMPORT'\n";
    std::fs::write(&path, raw).unwrap();
    let (profiles, report) = import_profiles(&path).unwrap();
    assert_eq!(profiles[0].deck, "Study");
    assert!(report.iter().any(|r| r.contains("Modal")));
    assert!(
        !serde_json::to_string(&(profiles, report))
            .unwrap()
            .contains("SECRET")
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), raw);
}
#[test]
fn unsupported_documents_get_actionable_error() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("book.pdf");
    assert!(
        load_source(&path)
            .unwrap_err()
            .to_string()
            .contains("Paste text")
    );
}

#[test]
fn supported_audio_extensions_do_not_fall_through_to_text_import() {
    let dir = tempfile::tempdir().unwrap();
    for extension in [
        "opus", "aiff", "aif", "m4b", "wma", "oga", "caf", "mp3", "wav",
    ] {
        let path = dir.path().join(format!("recording.{extension}"));
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(3 * 1024 * 1024).unwrap();
        let source = load_source(&path).unwrap();
        assert!(source.media, "{extension}");
        assert!(source.text.is_empty());
    }
}
#[cfg(unix)]
#[test]
fn multi_gigabyte_recordings_have_no_import_size_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long-recording.opus");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(6 * 1024 * 1024 * 1024).unwrap();
    assert!(load_source(&path).unwrap().media);
}
#[test]
fn recording_headers_are_recognized_without_a_known_extension() {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        ("recording.bin", b"OggS".as_slice()),
        ("track", b"RIFF\0\0\0\0WAVE".as_slice()),
        ("download.dat", b"\0\0\0\x18ftypisom".as_slice()),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        assert!(load_source(&path).unwrap().media, "{name}");
    }
}
#[test]
fn text_import_accepts_more_than_two_megabytes_without_caching_the_document() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("book.txt");
    std::fs::write(&path, "word ".repeat(600_000)).unwrap();
    let source = load_source(&path).unwrap();
    assert!(!source.media);
    assert!(source.text.is_empty());
    assert!(source.path.is_some());
}
