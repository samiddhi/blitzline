"""Verify recoverable provider failures and truthful stage cache/dependency contracts.

Scope statement:
    Reproduce companion defects using temporary runs and injected transports.
In scope:
    - Permanent/exhausted transport failures, preserved reports, and retryable stages.
    - Actual XDG/custom pack dependencies and safe missing-path fingerprints.
Included: isolated stage/provider/dependency regression coverage.
Out of scope:
    - Real credentials, paid provider requests, personal runs, and Windows execution.
Excluded: native platform proof and distribution publication.
Start here:
    Run these regressions alongside the maintained pipeline/storage suites.
"""

import json
import subprocess
from pathlib import Path

import pytest
from click.testing import CliRunner

from blitzline.cli import cli
from blitzline.core import prepare_review
from blitzline.integrations import blitzer
from blitzline.integrations.llm import RequestFailure
from blitzline.records import Candidate, Context
from blitzline.review import repair_cues
from blitzline.storage import RunStore


def candidates():
    """Supply one timed example without invoking extraction or language downloads."""
    return [
        Candidate(
            "cat", "cat", 1, (Context("example", "cat", 0, 3, ("cue",), 0, 1000),)
        )
    ]


@pytest.mark.parametrize("retryable", [False, True])
def test_transport_failure_is_not_retried_as_content_repair(
    settings, tmp_path, retryable
):
    """A provider failure stops review instead of creating a manual review task."""
    settings["profiles"]["test"] = {"provider": "openrouter", "model": "test"}
    settings["llm"]["review"] = "test"
    calls = []

    def fail(*args):
        """Simulate a sanitized provider error with explicit retry classification."""
        calls.append(1)
        raise RequestFailure("API returned HTTP 402", retryable=retryable)

    store = RunStore(tmp_path / "run", {})
    with pytest.raises(RequestFailure, match="402"):
        prepare_review(store, candidates(), "eng", settings, None, fail)
    assert calls == [1]
    assert not store.artifact("review.json").exists()
    assert not store.artifact("review-template.json").exists()


def test_cue_transport_failure_is_not_retried_by_content_repair(settings, tmp_path):
    """Cue repair likewise retains exhausted transport evidence without resubmitting it."""
    calls = []

    def fail(*args):
        """Supply an exhausted transport error."""
        calls.append(1)
        raise RequestFailure("Connection failed", retryable=True)

    rows, errors = repair_cues(
        [{"cue_id": "cue", "text": "cat"}],
        [],
        "eng",
        False,
        {},
        "Translate",
        settings,
        tmp_path,
        {},
        fail,
    )
    assert not rows and calls == [1] and errors[0]["kind"] == "transport"


def test_error_bearing_stage_is_partial_and_reruns_when_explicitly_resumed(tmp_path):
    """Correcting credentials then invoking the stage again cannot reuse cached failure as success."""
    store = RunStore(tmp_path, {})
    calls = []

    def failed():
        """Retain useful completed items alongside an operational failure."""
        calls.append("failed")
        return {"decisions": [], "errors": [{"error": "credit unavailable"}]}, []

    def repaired():
        """Represent the explicitly retried operation after account repair."""
        calls.append("repaired")
        return {"decisions": [], "errors": []}, []

    store.stage("05-review", "unchanged-input", failed)
    assert store.manifest["stages"]["05-review"]["state"] == "partial"
    assert not store.stage("05-review", "unchanged-input", repaired)["errors"]
    store.stage("05-review", "unchanged-input", repaired)
    assert calls == ["failed", "repaired"]


def version_runner(args, **kwargs):
    """Supply executable version output without invoking a real CLI."""
    return subprocess.CompletedProcess(args, 0, "bltzr test", "")


@pytest.mark.parametrize("spelling", ["language_packs_dir", "plugins_dir"])
def test_pack_dependencies_follow_actual_custom_directory(settings, tmp_path, spelling):
    """Modern/legacy explicit pack locations hash real pack bytes instead of an unrelated default."""
    config = tmp_path / "bltzr.toml"
    config.write_text(f'[locations]\n{spelling} = "custom packs"\n')
    pack = tmp_path / "custom packs" / "eng"
    pack.mkdir(parents=True)
    dictionary = pack / "lemmas.db"
    dictionary.write_bytes(b"first revision")
    options = {
        **settings["blitzer"],
        "config": str(config),
        "no_config": False,
        "plugins_dir": "",
    }
    first = blitzer.dependencies("eng", options, version_runner)
    assert first["pack"] == str(pack) and str(dictionary) in first["files"]
    dictionary.write_bytes(b"new revision")
    assert blitzer.dependencies("eng", options, version_runner) != first


def test_relative_xdg_roots_use_home_fallback(settings, tmp_path, monkeypatch):
    """Invalid relative XDG values use the same fallback as actual Blitzer configuration."""
    monkeypatch.setattr(Path, "home", lambda: tmp_path / "home")
    monkeypatch.setenv("XDG_DATA_HOME", "invalid-relative")
    options = {**settings["blitzer"], "plugins_dir": "", "no_config": True}
    assert blitzer.dependencies("eng", options, version_runner)["pack"] == str(
        tmp_path / "home/.local/share/bltzr/languages/eng"
    )


def test_explicit_retry_invalidates_descendants_and_retains_earlier_artifacts(tmp_path):
    """The public retry boundary touches only applicable stages and never calls a provider."""
    store = RunStore(tmp_path, {})
    first = store.stage("01-transcript", "input", lambda: ({"text": "cat"}, []))
    store.stage("05-review", "input", lambda: ({"errors": [{"error": "account"}]}, []))
    store.stage("06-media", "input", lambda: ({"cards": []}, []))
    before = store.artifact("01-transcript.json").read_bytes()
    result = CliRunner().invoke(
        cli, ["retry-stage", str(tmp_path), "--stage", "05-review"]
    )
    assert result.exit_code == 0
    assert json.loads(result.output)["invalidated"] == ["05-review", "06-media"]
    loaded = RunStore(tmp_path, {})
    assert loaded.manifest["stages"]["01-transcript"]["state"] == "complete"
    assert loaded.manifest["stages"]["06-media"]["state"] == "retry-requested"
    assert loaded.artifact("01-transcript.json").read_bytes() == before and first == {
        "text": "cat"
    }
    assert (
        CliRunner()
        .invoke(cli, ["retry-stage", str(tmp_path), "--stage", "01-transcript"])
        .exit_code
        != 0
    )
    caps = CliRunner().invoke(cli, ["capabilities"])
    assert json.loads(caps.output)["retry_stage"] == 1
