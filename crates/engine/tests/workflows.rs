use blitzline_domain::*;
use blitzline_engine::Engine;
use serde_json::Value;

fn waiting() -> (Engine, u64) {
    let mut engine = Engine::new(Workspace::default());
    let id = engine.example().unwrap();
    for _ in 0..4 {
        engine.tick();
    }
    assert_eq!(engine.project(id).unwrap().status, Status::Waiting);
    (engine, id)
}
fn complete(engine: &mut Engine, id: u64) {
    for _ in 0..20 {
        if engine.project(id).unwrap().status == Status::Ready {
            return;
        }
        if engine.project(id).unwrap().exchange.is_some() {
            let answer = engine.sample_reply(id).unwrap();
            engine.submit(id, &answer).unwrap();
            engine.next_exchange(id).unwrap();
        }
        engine.tick();
    }
    panic!("project never reached preview");
}
#[test]
fn browser_two_batches_to_preview_and_export() {
    let (mut engine, id) = waiting();
    assert_eq!(
        engine
            .project(id)
            .unwrap()
            .exchange
            .as_ref()
            .unwrap()
            .requested
            .len(),
        3
    );
    complete(&mut engine, id);
    assert_eq!(engine.project(id).unwrap().included(), 5);
    assert_eq!(engine.project(id).unwrap().history.len(), 2);
    assert!(engine.export_demo(id, "APKG").is_err());
    engine.preview(id).unwrap();
    engine.export_demo(id, "APKG").unwrap();
    assert_eq!(engine.project(id).unwrap().status, Status::Finished);
    assert!(engine.tsv(id).unwrap().contains("mačka\tcat"));
}
#[test]
fn partial_answer_preserves_good_items_and_repairs_only_missing() {
    let (mut engine, id) = waiting();
    let mut answer: Value = serde_json::from_str(&engine.sample_reply(id).unwrap()).unwrap();
    answer["decisions"].as_array_mut().unwrap().pop();
    let result = engine.submit(id, &answer.to_string()).unwrap();
    assert_eq!((result.accepted, result.remaining), (2, 1));
    assert!(engine.next_exchange(id).is_err());
    let followup = engine.prompt(id).unwrap();
    assert!(!followup.contains("\"word-1\""));
    assert!(followup.contains("\"word-3\""));
    let repair = engine.sample_reply(id).unwrap();
    assert_eq!(engine.submit(id, &repair).unwrap().remaining, 0);
    engine.next_exchange(id).unwrap();
}
#[test]
fn duplicate_or_unknown_ids_do_not_partially_mutate() {
    let (mut engine, id) = waiting();
    let before = engine.project(id).unwrap().clone();
    let mut answer: Value = serde_json::from_str(&engine.sample_reply(id).unwrap()).unwrap();
    let duplicate = answer["decisions"][0].clone();
    answer["decisions"].as_array_mut().unwrap().push(duplicate);
    assert!(engine.submit(id, &answer.to_string()).is_err());
    assert_eq!(*engine.project(id).unwrap(), before);
    answer["decisions"].as_array_mut().unwrap().pop();
    answer["decisions"][1]["id"] = "invented".into();
    assert!(engine.submit(id, &answer.to_string()).is_err());
    assert_eq!(*engine.project(id).unwrap(), before);
}
#[test]
fn old_exchange_cannot_overwrite_current_work() {
    let (mut engine, id) = waiting();
    let answer = engine.sample_reply(id).unwrap();
    engine.submit(id, &answer).unwrap();
    engine.next_exchange(id).unwrap();
    engine.tick();
    let before = engine.project(id).unwrap().clone();
    assert!(engine.submit(id, &answer).is_err());
    assert_eq!(*engine.project(id).unwrap(), before);
}
#[test]
fn repeated_answer_is_idempotent() {
    let (mut engine, id) = waiting();
    let answer = engine.sample_reply(id).unwrap();
    engine.submit(id, &answer).unwrap();
    assert_eq!(engine.submit(id, &answer).unwrap().accepted, 0);
    assert_eq!(engine.project(id).unwrap().included(), 3);
}
#[test]
fn malformed_fences_and_empty_meanings_are_actionable() {
    let (mut engine, id) = waiting();
    assert!(engine.submit(id, "Hello! Here is your answer {}").is_err());
    assert!(
        engine
            .submit(id, "```json\n{}\n```\n```json\n{}\n```")
            .is_err()
    );
    let mut answer: Value = serde_json::from_str(&engine.sample_reply(id).unwrap()).unwrap();
    answer["decisions"][0]["meaning"] = " ".into();
    let raw = format!("```json\n{}\n```", answer);
    let result = engine.submit(id, &raw).unwrap();
    assert_eq!((result.accepted, result.remaining), (2, 1));
}
#[test]
fn restart_retains_partial_reply_and_draft_but_pauses_queued_work() {
    let (mut engine, id) = waiting();
    engine
        .project_mut(id)
        .unwrap()
        .exchange
        .as_mut()
        .unwrap()
        .draft = "unfinished paste".into();
    let queued = engine.example().unwrap();
    let restored: Workspace =
        serde_json::from_str(&serde_json::to_string(&engine.state).unwrap()).unwrap();
    let restored = Engine::new(restored);
    assert_eq!(
        restored
            .project(id)
            .unwrap()
            .exchange
            .as_ref()
            .unwrap()
            .draft,
        "unfinished paste"
    );
    assert_eq!(restored.project(id).unwrap().status, Status::Waiting);
    assert_eq!(restored.project(queued).unwrap().status, Status::Paused);
}
#[test]
fn waiting_project_does_not_block_explicitly_queued_project() {
    let (mut engine, id) = waiting();
    let second = engine.example().unwrap();
    for _ in 0..4 {
        engine.tick();
    }
    assert_eq!(engine.project(id).unwrap().status, Status::Waiting);
    assert_eq!(engine.project(second).unwrap().status, Status::Waiting);
}
#[test]
fn source_edit_invalidates_answers_and_retains_export_history() {
    let (mut engine, id) = waiting();
    let old = engine.sample_reply(id).unwrap();
    complete(&mut engine, id);
    engine.preview(id).unwrap();
    engine.export_demo(id, "APKG").unwrap();
    engine
        .edit_transcript(id, "A changed source".into())
        .unwrap();
    assert_eq!(engine.project(id).unwrap().receipts.len(), 1);
    assert_eq!(engine.project(id).unwrap().revision, 2);
    assert!(engine.tsv(id).is_err());
    engine.resume(id).unwrap();
    engine.tick();
    engine.tick();
    assert!(engine.submit(id, &old).is_err());
}
#[test]
fn manual_method_keeps_accepted_answers_and_requires_meanings() {
    let (mut engine, id) = waiting();
    let answer = engine.sample_reply(id).unwrap();
    engine.submit(id, &answer).unwrap();
    engine.change_method(id, Method::Manual).unwrap();
    assert_eq!(engine.project(id).unwrap().included(), 3);
    assert!(engine.decide(id, "word-4", Decision::Keep).is_err());
    engine.decide(id, "word-4", Decision::Reject).unwrap();
    engine.decide(id, "word-5", Decision::Reject).unwrap();
    engine.finish_manual(id).unwrap();
    engine.tick();
    assert_eq!(engine.project(id).unwrap().status, Status::Ready);
}
#[test]
fn smaller_request_is_fresh_and_preserves_progress() {
    let (mut engine, id) = waiting();
    let stale = engine.sample_reply(id).unwrap();
    engine.shorten_request(id).unwrap();
    assert_eq!(
        engine
            .project(id)
            .unwrap()
            .exchange
            .as_ref()
            .unwrap()
            .requested
            .len(),
        2
    );
    assert!(engine.submit(id, &stale).is_err());
    complete(&mut engine, id);
    assert_eq!(engine.project(id).unwrap().included(), 5);
}
#[test]
fn api_simulation_and_optional_text_stages_finish_without_network() {
    let mut engine = Engine::new(Workspace::default());
    let id = engine.example().unwrap();
    let p = engine.project_mut(id).unwrap();
    p.settings.method = Method::Api;
    p.settings.proofread = true;
    p.settings.translate = true;
    for _ in 0..6 {
        engine.tick();
    }
    assert_eq!(engine.project(id).unwrap().status, Status::Ready);
    assert!(!engine.project(id).unwrap().translation.is_empty());
}
#[test]
fn model_failure_can_retry_after_simulated_install() {
    let mut engine = Engine::new(Workspace::default());
    let id = engine.example().unwrap();
    engine.project_mut(id).unwrap().source.media = true;
    engine.tick();
    assert_eq!(engine.project(id).unwrap().status, Status::Failed);
    engine.state.preferences.local_model_installed = true;
    engine.resume(id).unwrap();
    engine.tick();
    assert_eq!(engine.project(id).unwrap().stage, Stage::Transcript);
}
#[test]
fn editing_exported_word_requires_fresh_preview() {
    let (mut engine, id) = waiting();
    complete(&mut engine, id);
    engine.preview(id).unwrap();
    engine.export_demo(id, "APKG").unwrap();
    engine
        .edit_word(id, "word-1", "".into(), "".into())
        .unwrap();
    assert_eq!(engine.project(id).unwrap().pending(), 1);
    assert_eq!(engine.project(id).unwrap().revision, 2);
    assert!(engine.export_demo(id, "APKG").is_err());
    assert!(engine.preview(id).is_err());
}
#[test]
fn manual_proofreading_and_translation_exchange() {
    let mut engine = Engine::new(Workspace::default());
    let id = engine.example().unwrap();
    let p = engine.project_mut(id).unwrap();
    p.settings.proofread = true;
    p.settings.translate = true;
    engine.tick();
    engine.tick();
    assert_eq!(
        engine.project(id).unwrap().exchange.as_ref().unwrap().stage,
        Stage::Transcript
    );
    complete(&mut engine, id);
    assert_eq!(engine.project(id).unwrap().history.len(), 4);
}
