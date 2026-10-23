use blitzline_adapters::pipeline::{self as rt, Runtime, config};
use blitzline_engine::pipeline::Pipeline;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};
fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../tests/fixtures/reference/text.json")).unwrap()
}
fn setup(root: &Path) -> Pipeline<Runtime> {
    let f = fixture();
    let source = root.join("sample.txt");
    fs::write(&source, f["raw"].as_str().unwrap()).unwrap();
    let vocab = root.join("vocabulary.json");
    fs::write(&vocab, f["rows"].to_string()).unwrap();
    let mut config = config::defaults();
    config["run"]["method"] = json!("browser");
    config["media"]["ffmpeg"] = json!("must-not-run-ffmpeg");
    config["media"]["ffprobe"] = json!("must-not-run-ffprobe");
    let mut p = rt::start(
        &root.join("run"),
        &source,
        "slv",
        "text",
        config,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    p.backend.verbose = false;
    p.job.request.vocabulary_json = Some(vocab.to_string_lossy().into());
    p
}
fn reply(p: &mut Pipeline<Runtime>, decisions: Value) {
    let task = p.job.pending.as_ref().unwrap();
    p.submit(
        &json!({"exchange_id":task.id,"revision":task.revision,"decisions":decisions}).to_string(),
    )
    .unwrap();
}
fn ready(root: &Path) -> Pipeline<Runtime> {
    let mut p = setup(root);
    assert_eq!(p.process(None).unwrap_err().code, "needs_review");
    reply(&mut p, fixture()["decisions"].clone());
    p.process(None).unwrap();
    p
}
#[test]
fn browser_reply_survives_restart_and_needs_no_media_tools() {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture();
    let mut p = setup(dir.path());
    assert_eq!(p.process(None).unwrap_err().code, "needs_review");
    reply(&mut p, json!([f["decisions"][0]]));
    let root = p.backend.root.clone();
    drop(p);
    let mut p = rt::open(&root, Arc::new(AtomicBool::new(false))).unwrap();
    p.backend.verbose = false;
    assert_eq!(p.job.decisions.len(), 1);
    assert_eq!(p.process(None).unwrap_err().code, "needs_review");
    reply(&mut p, json!([f["decisions"][1]]));
    p.process(None).unwrap();
    assert_eq!(p.job.cards, 2);
    assert_eq!(p.job.state, "complete");
    assert!(root.join("exports/tsv/cards.tsv").is_file());
    assert!(!root.join("media").exists());
    let raw = fs::read_to_string(root.join("exports/tsv/cards.tsv")).unwrap();
    assert!(raw.contains("<b>Mačka</b>"));
    assert!(!raw.contains("[sound:"));
}
#[test]
fn stale_unknown_and_duplicate_replies_leave_saved_work_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = setup(dir.path());
    p.process(None).unwrap_err();
    let f = fixture();
    let task = p.job.pending.clone().unwrap();
    let stale = json!({"exchange_id":"old","revision":task.revision,"decisions":f["decisions"]});
    assert!(p.submit(&stale.to_string()).is_err());
    let mut duplicate = f["decisions"].as_array().unwrap().clone();
    duplicate.push(duplicate[0].clone());
    assert!(
        p.submit(
            &json!({"exchange_id":task.id,"revision":task.revision,"decisions":duplicate})
                .to_string()
        )
        .is_err()
    );
    assert!(p.job.decisions.is_empty());
}
#[test]
fn native_apkg_matches_reference_notes_templates_and_conditional_cards() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = ready(dir.path());
    let gold: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/reference/packages.json"
    ))
    .unwrap();
    for preset in ["blitzer", "legacy"] {
        for recall in [false, true] {
            p.job.settings["export"]["format"] = json!("apkg");
            p.job.settings["export"]["preset"] = json!(preset);
            p.job.settings["export"]["two_way"] = json!(recall);
            p.export_current().unwrap();
            let package = p.backend.root.join("exports/apkg/cards.apkg");
            let mut zip = zip::ZipArchive::new(fs::File::open(package).unwrap()).unwrap();
            let dbfile = dir.path().join("collection.db");
            {
                let mut file = fs::File::create(&dbfile).unwrap();
                std::io::copy(&mut zip.by_name("collection.anki2").unwrap(), &mut file).unwrap();
            }
            let db = rusqlite::Connection::open(&dbfile).unwrap();
            let expected = &gold[format!("{preset}-{}", if recall { "True" } else { "False" })];
            let notes = db
                .prepare("SELECT guid,mid,flds FROM notes ORDER BY id")
                .unwrap()
                .query_map([], |r| {
                    Ok(json!([
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?
                    ]))
                })
                .unwrap()
                .map(|r| r.unwrap())
                .collect::<Vec<_>>();
            assert_eq!(json!(notes), expected["notes"]);
            let cards = db
                .prepare("SELECT ord,due FROM cards ORDER BY id")
                .unwrap()
                .query_map([], |r| {
                    Ok(json!([r.get::<_, i64>(0)?, r.get::<_, i64>(1)?]))
                })
                .unwrap()
                .map(|r| r.unwrap())
                .collect::<Vec<_>>();
            assert_eq!(json!(cards), expected["cards"]);
            let models: Value = serde_json::from_str(
                &db.query_row::<String, _, _>("SELECT models FROM col", [], |r| r.get(0))
                    .unwrap(),
            )
            .unwrap();
            let actual = models.as_object().unwrap().values().next().unwrap();
            let reference = expected["models"]
                .as_object()
                .unwrap()
                .values()
                .next()
                .unwrap();
            assert_eq!(actual["req"], reference["req"]);
            assert_eq!(
                actual["tmpls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| (&t["name"], &t["qfmt"], &t["afmt"]))
                    .collect::<Vec<_>>(),
                reference["tmpls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| (&t["name"], &t["qfmt"], &t["afmt"]))
                    .collect::<Vec<_>>()
            );
        }
    }
}
#[test]
fn checksums_block_changed_sources_and_corrupt_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = ready(dir.path());
    fs::write(p.backend.root.join("artifacts/06-media.json"), b"{}").unwrap();
    assert!(p.export_current().is_err());
    p.process(None).unwrap();
    fs::write(&p.job.request.source, "Different source").unwrap();
    assert!(p.export_current().is_err());
    assert!(p.process(None).is_err());
}
#[test]
fn cleanup_is_explicit_verified_and_nonresumable() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = ready(dir.path());
    let preview = p.backend.cleanup(&p.job, false).unwrap();
    assert!(preview["files"].as_u64().unwrap() > 0);
    assert!(p.backend.root.join("artifacts").exists());
    p.backend.cleanup(&p.job, true).unwrap();
    p.job = p.backend.load().unwrap();
    assert_eq!(p.job.state, "cleaned");
    assert!(p.process(None).is_err());
    assert!(p.backend.root.join("exports/tsv/cards.tsv").exists());
}
#[test]
fn two_writers_cannot_modify_one_run() {
    let dir = tempfile::tempdir().unwrap();
    let p = setup(dir.path());
    assert!(rt::open(&p.backend.root, Arc::new(AtomicBool::new(false))).is_err());
}
#[test]
fn script_tools_are_accepted_and_unrecognized_configuration_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("bltzr");
    fs::write(&script, "#!/usr/bin/python3\nprint('allowed')").unwrap();
    assert!(rt::find_tool(script.to_str().unwrap()).is_ok());
    assert!(config::merge(json!({"run":{"typo":true}}), dir.path()).is_err());
    assert!(
        config::merge(
            json!({"profiles":{"x":{"model":"x","api_key":"secret"}}}),
            dir.path()
        )
        .is_err()
    );
}

#[test]
fn conflicting_accepted_answer_rejects_the_whole_reply() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = setup(dir.path());
    p.process(None).unwrap_err();
    let f = fixture();
    reply(&mut p, json!([f["decisions"][1]]));
    let original = serde_json::to_value(&p.job).unwrap();
    let task = p.job.pending.as_ref().unwrap();
    let mut changed = f["decisions"].clone();
    changed[1]["english"] = json!("changed accepted answer");
    assert!(
        p.submit(
            &json!({"exchange_id":task.id,"revision":task.revision,"decisions":changed})
                .to_string()
        )
        .is_err()
    );
    assert_eq!(serde_json::to_value(&p.job).unwrap(), original);
    let saved: Value =
        serde_json::from_slice(&fs::read(p.backend.root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(saved, original);
}

#[test]
fn interrupted_cleanup_can_be_completed_without_losing_exports() {
    use blitzline_engine::pipeline::Backend;
    let dir = tempfile::tempdir().unwrap();
    let mut p = ready(dir.path());
    p.job.state = "cleaning".into();
    p.backend.save(&p.job).unwrap();
    fs::remove_dir_all(p.backend.root.join("artifacts")).unwrap();
    assert!(p.process(None).is_err());
    assert_eq!(p.job.state, "cleaning");
    p.backend.cleanup(&p.job, true).unwrap();
    assert_eq!(p.backend.load().unwrap().state, "cleaned");
    assert!(p.backend.root.join("exports/tsv/cards.tsv").is_file());
}

#[test]
fn native_ffmpeg_clips_are_verified_packaged_and_rebuilt_after_corruption() {
    let ffmpeg = match rt::find_tool("ffmpeg") {
        Ok(path) => path,
        Err(error) => {
            assert!(
                std::env::var_os("BLITZLINE_REQUIRE_MEDIA_TESTS").is_none(),
                "{error}"
            );
            eprintln!("Skipping native media test: {error}");
            return;
        }
    };
    let ffprobe = rt::find_tool("ffprobe").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.wav");
    let size: u32 = 16000 * 2 * 3;
    let mut wave = Vec::new();
    wave.extend(b"RIFF");
    wave.extend((36 + size).to_le_bytes());
    wave.extend(b"WAVEfmt ");
    wave.extend(16u32.to_le_bytes());
    wave.extend(1u16.to_le_bytes());
    wave.extend(1u16.to_le_bytes());
    wave.extend(16000u32.to_le_bytes());
    wave.extend(32000u32.to_le_bytes());
    wave.extend(2u16.to_le_bytes());
    wave.extend(16u16.to_le_bytes());
    wave.extend(b"data");
    wave.extend(size.to_le_bytes());
    wave.resize(wave.len() + size as usize, 0);
    fs::write(&source, wave).unwrap();
    let subtitles = dir.path().join("sample.srt");
    fs::write(&subtitles, "1\n00:00:00,000 --> 00:00:01,000\nMačka spi.\n\n2\n00:00:01,200 --> 00:00:02,600\nCafé čaka. Mačka spi.\n").unwrap();
    let vocabulary = dir.path().join("vocabulary.json");
    fs::write(&vocabulary, fixture()["rows"].to_string()).unwrap();
    let mut settings = config::defaults();
    settings["run"]["method"] = json!("browser");
    settings["media"]["ffmpeg"] = json!(ffmpeg);
    settings["media"]["ffprobe"] = json!(ffprobe);
    settings["export"]["format"] = json!("apkg");
    let mut p = rt::start(
        &dir.path().join("run"),
        &source,
        "slv",
        "media",
        settings,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    p.backend.verbose = false;
    p.job.request.srt = Some(subtitles.to_string_lossy().into());
    p.job.request.vocabulary_json = Some(vocabulary.to_string_lossy().into());
    assert_eq!(p.process(None).unwrap_err().code, "needs_review");
    reply(&mut p, fixture()["decisions"].clone());
    p.process(None).unwrap();
    assert_eq!(p.job.cards, 2);
    let media = p.job.stages["06-media"]
        .outputs
        .keys()
        .find(|name| name.ends_with(".mp3"))
        .unwrap()
        .clone();
    let original = fs::read(p.backend.root.join(&media)).unwrap();
    assert!(!original.is_empty());
    let mut zip = zip::ZipArchive::new(
        fs::File::open(p.backend.root.join("exports/apkg/cards.apkg")).unwrap(),
    )
    .unwrap();
    let mut map = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("media").unwrap(), &mut map).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&map)
            .unwrap()
            .as_object()
            .unwrap()
            .len(),
        2
    );
    fs::write(p.backend.root.join(&media), "corrupt").unwrap();
    assert!(p.export_current().is_err());
    p.process(None).unwrap();
    assert_eq!(fs::read(p.backend.root.join(media)).unwrap(), original);
}
