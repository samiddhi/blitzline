"""Verify release decisions and recovery without real publication.

Scope statement: isolated repository release orchestration tests.
Included: version planning, no-op detection, checkpoints and GitHub drafts.
Excluded: real commits, pushes, uploads, authentication and remote jobs.
Start here: mocked run calls capture the commands a release would execute.
"""

import json
from unittest.mock import Mock

import pytest

from maintenance import release


def test_patch_versions():
    """Increment patch numbers while rejecting unsupported version shapes."""
    assert release.next_patch("0.1.0") == "0.1.1"
    assert release.next_patch("0.1.9") == "0.1.10"
    with pytest.raises(ValueError):
        release.next_patch("0.1")


def test_unchanged_source_skips_release(monkeypatch):
    """Avoid tests or checkpoint writes when source matches its release tag."""
    monkeypatch.setattr(release, "project_version", Mock(return_value="0.1.0"))
    monkeypatch.setattr(release, "source_changed", Mock(return_value=False))
    commands = Mock()
    checkpoint = Mock()
    monkeypatch.setattr(release, "run", commands)
    monkeypatch.setattr(release, "write_json", checkpoint)
    assert release.prepare_release() is None
    commands.assert_not_called()
    checkpoint.assert_not_called()


def test_preparation_checkpoints_after_tests(tmp_path, monkeypatch):
    """Save the next version only after the tests succeed."""
    pending = tmp_path / "pending.json"
    monkeypatch.setattr(release, "PENDING", pending)
    monkeypatch.setattr(release, "project_version", Mock(return_value="0.1.0"))
    monkeypatch.setattr(release, "source_changed", Mock(return_value=True))
    commands = Mock(side_effect=RuntimeError("tests failed"))
    monkeypatch.setattr(release, "run", commands)
    with pytest.raises(RuntimeError, match="tests failed"):
        release.prepare_release()
    assert not pending.exists()
    commands.side_effect = None
    state = release.prepare_release()
    assert state == json.loads(pending.read_text())
    assert state == {"version": "0.1.1", "previous_version": "0.1.0", "head": None}


def test_resume_committed_release_does_not_bump(tmp_path, monkeypatch):
    """Retry publication without creating another version or commit."""
    pending = tmp_path / "pending.json"
    pending.write_text("{}")
    monkeypatch.setattr(release, "PENDING", pending)
    commit = Mock(side_effect=AssertionError("unexpected bump"))
    monkeypatch.setattr(release, "commit_release", commit)
    push = Mock()
    publish = Mock()
    monkeypatch.setattr(release, "push_release", push)
    monkeypatch.setattr(release, "publish_github", publish)
    state = {"version": "0.1.1", "head": "commit"}
    release.finish_release(state)
    commit.assert_not_called()
    push.assert_called_once_with(state, "v0.1.1")
    publish.assert_called_once_with(state, "v0.1.1")
    assert not pending.exists()


def test_failed_publication_preserves_checkpoint(tmp_path, monkeypatch):
    """Retain retry information when GitHub publication fails."""
    pending = tmp_path / "pending.json"
    pending.write_text("{}")
    monkeypatch.setattr(release, "PENDING", pending)
    monkeypatch.setattr(release, "push_release", Mock())
    monkeypatch.setattr(
        release, "publish_github", Mock(side_effect=OSError("upload failed"))
    )
    with pytest.raises(OSError):
        release.finish_release({"version": "0.1.1", "head": "commit"})
    assert pending.exists()


def test_github_draft_published_once(monkeypatch):
    """Create a draft before publishing and reuse an already public release."""
    missing = Mock(returncode=1)
    commands = Mock(side_effect=[missing, None, None])
    monkeypatch.setattr(release, "run", commands)
    release.publish_github({"version": "0.1.1"}, "v0.1.1")
    assert "--draft" in commands.call_args_list[1].args
    assert "--draft=false" in commands.call_args_list[2].args
    commands.reset_mock()
    commands.side_effect = [Mock(returncode=0, stdout='{"isDraft": false}')]
    release.publish_github({"version": "0.1.1"}, "v0.1.1")
    assert commands.call_count == 1
