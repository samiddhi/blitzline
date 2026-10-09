"""Exercise real Blitzer/FFmpeg with isolated transcript and fake model inputs.

Scope statement: test the complete local stage sequence and recovery behavior.
Included: imported ASR/SRT, automated review/proofread/translation, real audio
and video clips, manual review, checksum invalidation, and empty results.
Excluded: paid LLM APIs, live user Anki profiles, and heavyweight ASR downloads.
Start here: test_full_pipeline performs a complete automatic offline run.
"""

import csv
import shutil
from io import StringIO

import pytest
from conftest import approve_request, configure_llm

from blitzline.core import export_run, resume_pipeline, run_pipeline
from blitzline.records import NeedsReview, PipelineError
from blitzline.storage import read_json, write_json
from blitzline.transcript import parse_srt


def require_blitzer():
    """Skip local-tool integration only when the Blitzer executable is absent."""
    if not shutil.which("bltzr"):
        pytest.skip("Blitzer required for end-to-end integration")


def transcript_file(tmp_path):
    """Write tiny ASR JSON with a repeated sentence and one cross-segment sentence."""
    path = tmp_path / "asr.json"
    write_json(
        path,
        {
            "segments": [
                {"start": 0.1, "end": 0.9, "text": "Cat runs."},
                {"start": 1.0, "end": 1.9, "text": "Cat runs."},
                {"start": 2.0, "end": 2.5, "text": "Dog"},
                {"start": 2.6, "end": 3.4, "text": "runs."},
            ]
        },
    )
    return path


def test_full_pipeline(tmp_path, recording, settings):
    """Create source/English SRT, reviewed cards, real clips, and importable TSV."""
    require_blitzer()
    configure_llm(settings)
    settings["run"].update(proofread=True, translate=True)
    directory = run_pipeline(
        recording,
        "eng",
        settings,
        asr_json=transcript_file(tmp_path),
        requester=approve_request,
    )
    manifest = read_json(directory / "manifest.json")
    assert manifest["state"] == "complete" and manifest["cards"] == 3
    source = parse_srt((directory / "artifacts/source.srt").read_text(), 4000)
    english = parse_srt((directory / "artifacts/english.srt").read_text(), 4000)
    assert [(c.id, c.start_ms, c.end_ms) for c in source] == [
        (c.id, c.start_ms, c.end_ms) for c in english
    ]
    raw = (directory / "exports/tsv/cards.tsv").read_text()
    body = "\n".join(line for line in raw.splitlines() if not line.startswith("#"))
    rows = list(csv.reader(StringIO(body), delimiter="\t"))
    assert len(rows) == 3 and all(len(row) == 12 for row in rows)
    assert any("<b>" in row[2] for row in rows)
    before = {p.name: p.stat().st_mtime_ns for p in (directory / "media").glob("*.mp3")}
    resume_pipeline(directory, requester=approve_request)
    assert before == {
        p.name: p.stat().st_mtime_ns for p in (directory / "media").glob("*.mp3")
    }


def test_manual_review_resume(tmp_path, recording, settings):
    """Pause unchecked vocabulary, accept edited JSON, and preserve blank Sentence2."""
    require_blitzer()
    output = tmp_path / "run"
    with pytest.raises(NeedsReview):
        run_pipeline(
            recording,
            "eng",
            settings,
            directory=output,
            asr_json=transcript_file(tmp_path),
        )
    assert read_json(output / "manifest.json")["state"] == "needs-review"
    candidates = read_json(output / "artifacts/04-vocabulary.json")
    review = read_json(output / "artifacts/review.json")
    by_id = {c["id"]: c for c in candidates}
    for row in review["decisions"]:
        row.update(
            decision="keep",
            reason="",
            english="meaning",
            context_ids=[by_id[row["candidate_id"]]["contexts"][0]["id"]],
        )
    path = output / "artifacts/review.json"
    write_json(path, review)
    resume_pipeline(output, review_file=path)
    assert read_json(output / "manifest.json")["state"] == "complete"
    assert any(
        not card["contexts"][1:]
        for card in read_json(output / "artifacts/06-media.json")["cards"]
    )


def test_video_and_export_checksum(tmp_path, recording, settings):
    """Encode video examples and refuse exports after referenced media corruption."""
    require_blitzer()
    configure_llm(settings)
    settings["media"]["kind"] = "video"
    directory = run_pipeline(
        recording,
        "eng",
        settings,
        asr_json=transcript_file(tmp_path),
        requester=approve_request,
    )
    assert list((directory / "media").glob("*.mp4"))
    clip = next((directory / "media").glob("*.mp4"))
    clip.write_bytes(b"corrupted")
    with pytest.raises(PipelineError, match="changed"):
        export_run(directory)
    resume_pipeline(directory, requester=approve_request)
    assert clip.stat().st_size > 100


def test_changed_transcript_invalidates_descendants(tmp_path, recording, settings):
    """Invalidate completed cards when source subtitles change and stop before rereview."""
    require_blitzer()
    configure_llm(settings)
    path = transcript_file(tmp_path)
    directory = run_pipeline(
        recording, "eng", settings, asr_json=path, requester=approve_request
    )
    value = read_json(path)
    value["segments"][0]["text"] = "Bird flies."
    write_json(path, value)
    resume_pipeline(directory, stop_after="transcript", requester=approve_request)
    manifest = read_json(directory / "manifest.json")
    assert "06-media" not in manifest["stages"]
    with pytest.raises(PipelineError, match="resume"):
        export_run(directory)


def test_known_list_change_invalidates_extraction(tmp_path, recording, settings):
    """Observe known-list changes without writing to known lists or history."""
    require_blitzer()
    configure_llm(settings)
    known = tmp_path / "known.txt"
    known.write_text("")
    settings["blitzer"]["known_file"] = str(known)
    directory = run_pipeline(
        recording,
        "eng",
        settings,
        asr_json=transcript_file(tmp_path),
        requester=approve_request,
    )
    before = read_json(directory / "artifacts/04-vocabulary.json")
    known.write_text("cat\n")
    resume_pipeline(directory, requester=approve_request)
    after = read_json(directory / "artifacts/04-vocabulary.json")
    assert len(after) == len(before) - 1
    assert known.read_text() == "cat\n"


def test_empty_transcript_succeeds(tmp_path, recording, settings):
    """Treat silent transcripts as successful empty subtitle/card outputs."""
    require_blitzer()
    path = tmp_path / "empty.json"
    write_json(path, {"segments": []})
    directory = run_pipeline(recording, "eng", settings, asr_json=path)
    assert read_json(directory / "manifest.json")["cards"] == 0
    assert (directory / "artifacts/source.srt").read_text() == ""


def test_export_rejects_changed_inputs(tmp_path, recording, settings):
    """Reject standalone delivery when filtering inputs or final subtitles changed."""
    require_blitzer()
    configure_llm(settings)
    known = tmp_path / "known.txt"
    known.write_text("")
    settings["blitzer"]["known_file"] = str(known)
    directory = run_pipeline(
        recording,
        "eng",
        settings,
        asr_json=transcript_file(tmp_path),
        requester=approve_request,
    )
    known.write_text("cat\n")
    with pytest.raises(PipelineError, match="Blitzer inputs changed"):
        export_run(directory)
    known.write_text("")
    subtitle = directory / "artifacts/source.srt"
    subtitle.write_text(subtitle.read_text().replace("Cat", "Bird"))
    with pytest.raises(PipelineError, match="upstream artifact changed"):
        export_run(directory)
