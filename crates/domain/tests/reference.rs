use blitzline_domain::pipeline::{notes, text, vocabulary, *};
use serde_json::{Value, json};
fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../tests/fixtures/reference/text.json")).unwrap()
}
#[test]
fn python_text_candidates_and_note_identities_match() {
    let f = fixture();
    let cues = text::import(f["raw"].as_str().unwrap(), 1000).unwrap();
    assert_eq!(serde_json::to_value(&cues).unwrap(), f["cues"]);
    assert_eq!(text::plaintext(&cues).0, f["plaintext"]);
    let c = vocabulary::map_candidates(&f["rows"], &cues).unwrap();
    assert_eq!(serde_json::to_value(&c).unwrap(), f["candidates"]);
    let decisions = f["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| serde_json::from_value::<Decision>(v.clone()).unwrap())
        .map(|d| (d.candidate_id.clone(), d))
        .collect();
    let request = Request {
        source: "sample.txt".into(),
        source_kind: "text".into(),
        source_hash: f["source_hash"].as_str().unwrap().into(),
        language: "slv".into(),
        srt: None,
        asr_json: None,
        text_file: None,
        vocabulary_json: None,
    };
    assert_eq!(
        serde_json::to_value(vocabulary::build_cards(&decisions, &c, &request)).unwrap(),
        f["cards"]
    );
    for case in f["identity_cases"].as_array().unwrap() {
        assert_eq!(digest(&case["value"]), case["digest"]);
    }
}
#[test]
fn python_presets_fields_and_model_ids_match() {
    let f = fixture();
    let card: Card = serde_json::from_value(f["cards"][0].clone()).unwrap();
    for preset in ["legacy", "blitzer"] {
        let s = json!({"preset":preset,"model":"Blitzer Basic","language":"slv","max_sentence_words":25,"two_way":false,"tags":[]});
        let model = notes::definition(&s).unwrap();
        assert_eq!(model, f["models"][preset]["definition"]);
        assert_eq!(
            notes::model_id("Blitzer Basic", &model),
            f["models"][preset]["model_id"]
        );
        let fields = notes::render_fields(&card, &s, &model)
            .unwrap()
            .into_iter()
            .map(|(k, v)| (k, json!(v)))
            .collect::<serde_json::Map<_, _>>();
        assert_eq!(json!(fields), f["models"][preset]["fields"]);
        assert_eq!(
            notes::numeric_id("Blitzline"),
            f["models"][preset]["deck_id"]
        );
    }
}
#[test]
fn unicode_offsets_are_characters_not_bytes() {
    let cues = text::import("🦊 Café čaka. Café čaka.", 1000).unwrap();
    let candidates=vocabulary::map_candidates(&json!([{"term":"čakati","count":2,"contexts":[{"text":"Café čaka.","highlight_start":5,"highlight_end":9}]}]),&cues).unwrap();
    assert_eq!(candidates[0].contexts[0].matches, vec![2, 13]);
    assert!(vocabulary::map_candidates(&json!([{"term":"x","count":1,"contexts":[{"text":"Café","highlight_start":0,"highlight_end":5}]}]),&cues).is_err());
}
#[test]
fn timing_validation_and_bankers_rounding() {
    assert_eq!(text::milliseconds(0.0005).unwrap(), 0);
    assert_eq!(text::milliseconds(0.0015).unwrap(), 2);
    assert!(text::milliseconds(f64::NAN).is_err());
    assert!(text::milliseconds(-0.1).is_err());
    let raw = "1\n00:00:00,100 --> 00:00:01,000\nMačka spi.\n\n2\n00:00:00,900 --> 00:00:02,000\nCafé čaka.\n";
    let cues = text::parse_srt(raw, 2000).unwrap();
    assert_eq!(text::render_srt(&cues, 42).unwrap(), raw);
    assert!(text::parse_srt(raw, 1999).is_err());
}
#[test]
fn shared_forms_stay_together_and_oversize_is_explicit() {
    let f = fixture();
    let mut candidates: Vec<Candidate> = serde_json::from_value(f["candidates"].clone()).unwrap();
    candidates[1].contexts[0].text = "MAČKA".into();
    candidates[1].contexts[0].highlight_start = 0;
    candidates[1].contexts[0].highlight_end = 5;
    let groups = vocabulary::groups(&candidates, &Default::default());
    assert_eq!(groups.len(), 1);
    assert!(vocabulary::batches(groups, 1, 24000).is_err());
}
#[test]
fn unknown_and_duplicate_review_ids_do_not_become_cards() {
    let f = fixture();
    let c: Vec<Candidate> = serde_json::from_value(f["candidates"].clone()).unwrap();
    let mut rows = f["decisions"].as_array().unwrap().clone();
    rows.push(rows[0].clone());
    let (valid, errors) = vocabulary::validate_decisions(&json!({"decisions":rows}), &c);
    assert_eq!(valid.len(), 1);
    assert_eq!(errors.len(), 1);
    rows[2]["candidate_id"] = json!("not-owned");
    let (valid, errors) = vocabulary::validate_decisions(&json!({"decisions":rows}), &c);
    assert!(valid.is_empty());
    assert_eq!(errors[0]["candidate_id"], "");
}

#[test]
fn shortened_examples_keep_the_selected_occurrence_and_aligned_bounds() {
    let cue = Cue {
        id: "cue-000001".into(),
        start_ms: Some(0),
        end_ms: Some(1200),
        text: "Cat sleeps. Dog waits.".into(),
        words: vec![
            AlignedWord {
                text: "Cat".into(),
                start_ms: 0,
                end_ms: 300,
            },
            AlignedWord {
                text: "sleeps.".into(),
                start_ms: 300,
                end_ms: 600,
            },
            AlignedWord {
                text: "Dog".into(),
                start_ms: 600,
                end_ms: 900,
            },
            AlignedWord {
                text: "waits.".into(),
                start_ms: 900,
                end_ms: 1200,
            },
        ],
    };
    let candidates = vocabulary::map_candidates(&json!([{"term":"dog","count":1,"contexts":[{"text":cue.text,"highlight_start":12,"highlight_end":15}]}]), std::slice::from_ref(&cue)).unwrap();
    let mut context = vocabulary::shorten(&candidates[0].contexts[0], 25);
    vocabulary::retime(&mut context, &[cue]).unwrap();
    assert_eq!(context.text, "Dog waits.");
    assert_eq!(context.matches, vec![12]);
    assert_eq!((context.start_ms, context.end_ms), (Some(600), Some(1200)));
}
