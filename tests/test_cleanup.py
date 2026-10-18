"""Check explicit cleanup leaves verified portable exports intact.

Scope statement: test cleanup previews, protection, and final receipts.
Included: complete APKG runs, unchanged dry runs, corrupt exports, and CLI flags.
Excluded: Anki application imports and paid provider calls.
Start here: test_clean_preview_and_apply exercises the public command.
"""

import json
from pathlib import Path

import pytest
from click.testing import CliRunner

from blitzline.cleanup import cleanup_plan
from blitzline.cli import cli
from blitzline.records import PipelineError
from blitzline.storage import RunStore, fingerprint, read_json


def exported_run(tmp_path: Path) -> Path:
    """Create a completed run with one trusted portable export and intermediates."""
    run = tmp_path / "run"
    store = RunStore(run, {})
    package = run / "exports/apkg/cards.apkg"
    package.parent.mkdir(parents=True, exist_ok=True)
    package.write_bytes(b"portable deck")
    (run / "artifacts/review.json").write_text('{"decisions": []}')
    (run / "media/clip.mp3").write_bytes(b"audio")
    (run / "logs/events.jsonl").write_text("event\n")
    cache = run / ".cache/requests"
    cache.mkdir(parents=True)
    (cache / "answer.json").write_text("{}")
    store.manifest.update(
        state="complete",
        settings={"export": {"format": "apkg"}},
        stages={
            "07-export-apkg": {
                "state": "complete",
                "outputs": {"exports/apkg/cards.apkg": fingerprint(package)},
            }
        },
    )
    store.save()
    return run


def test_clean_preview_and_apply(tmp_path):
    """Preview without deletion, then remove process files while retaining deck."""
    run = exported_run(tmp_path)
    runner = CliRunner()
    preview = runner.invoke(cli, ["clean", str(run)])
    assert preview.exit_code == 0, preview.output
    assert "Would remove 4 files" in preview.output
    assert (run / "artifacts/review.json").exists()
    applied = runner.invoke(cli, ["clean", str(run), "--apply"])
    assert applied.exit_code == 0, applied.output
    assert "Removed 4 files" in applied.output
    assert (run / "exports/apkg/cards.apkg").read_bytes() == b"portable deck"
    for name in ("artifacts", "media", "logs", ".cache"):
        assert not (run / name).exists()
    receipt = read_json(run / "manifest.json")
    assert receipt["state"] == "cleaned"
    assert "settings" not in receipt
    resumed = runner.invoke(cli, ["resume", str(run)])
    assert resumed.exit_code != 0
    assert "cleaned" in resumed.output


def test_clean_refuses_incomplete_and_changed_export(tmp_path):
    """Never delete intermediates without a current, completed portable export."""
    run = exported_run(tmp_path)
    manifest = read_json(run / "manifest.json")
    manifest["state"] = "stopped"
    (run / "manifest.json").write_text(json.dumps(manifest))
    with pytest.raises(PipelineError, match="Finish the run"):
        cleanup_plan(run)
    manifest["state"] = "complete"
    (run / "manifest.json").write_text(json.dumps(manifest))
    (run / "exports/apkg/cards.apkg").write_bytes(b"changed")
    with pytest.raises(PipelineError, match="Export changed"):
        cleanup_plan(run)
    assert (run / "artifacts/review.json").exists()
