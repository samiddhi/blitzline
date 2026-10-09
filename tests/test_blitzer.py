"""Verify the current Blitzer skip-list contract and cache dependencies.

Scope statement: cover configuration, CLI forwarding and read-only extraction.
Included: exact words, word families, automatic config discovery, file override
precedence, missing-list fingerprints and vocabulary invalidation on resume.
Excluded: Blitzer internals, production dictionary quality and remote services.
Start here: the extraction test uses a tiny English pack with all forms of be.
"""

import shutil
import subprocess
from pathlib import Path

import pytest
from conftest import approve_request, configure_llm

from blitzline.config import get_config, upgrade_settings
from blitzline.core import export_run, resume_pipeline, run_pipeline
from blitzline.integrations import blitzer
from blitzline.records import PipelineError
from blitzline.storage import digest, read_json, write_json


@pytest.fixture(autouse=True)
def isolated_blitzer_config(tmp_path, monkeypatch):
    """Keep config discovery tests away from the user's actual files."""
    monkeypatch.setattr(Path, "home", lambda: tmp_path / "home")
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path / "xdg"))
    monkeypatch.delenv("BLITZER_CONFIG", raising=False)
    monkeypatch.delenv("BLITZLINE_CONFIG", raising=False)


def version_runner(args, **kwargs):
    """Supply a version without invoking the installed command for hash tests."""
    assert args[-1] == "--version"
    return subprocess.CompletedProcess(args, 0, "bltzr, version test\n", "")


def require_blitzer():
    """Use a real external CLI when available without importing its internals."""
    if not shutil.which("bltzr"):
        pytest.skip("Current Blitzer required for CLI integration")


def test_skip_file_config_paths_and_legacy_runs(tmp_path, settings):
    """Resolve both list paths and supply new defaults when loading old runs."""
    config = tmp_path / "blitzline.toml"
    config.write_text(
        '[blitzer]\nskip_exact_words_file="exact.txt"\n'
        'skip_word_families_file="families.txt"\n'
    )
    selected = get_config(config)["blitzer"]
    assert selected["skip_exact_words_file"] == str(tmp_path / "exact.txt")
    assert selected["skip_word_families_file"] == str(tmp_path / "families.txt")
    settings["blitzer"].pop("skip_exact_words_file")
    settings["blitzer"].pop("skip_word_families_file")
    settings["blitzer"]["known_file"] = str(tmp_path / "old.txt")
    upgraded = upgrade_settings(settings)["blitzer"]
    assert upgraded["skip_exact_words_file"] == ""
    assert upgraded["skip_word_families_file"] == ""
    assert upgraded["known_file"] == settings["blitzer"]["known_file"]


@pytest.mark.parametrize("value", ['["exact.txt"]', "false", "123"])
def test_skip_file_requires_a_path_string(tmp_path, value):
    """Reject arrays and non-path values instead of silently disabling a list."""
    config = tmp_path / "blitzline.toml"
    config.write_text(f"[blitzer]\nskip_exact_words_file={value}\n")
    with pytest.raises(ValueError, match="skip_exact_words_file"):
        get_config(config)


def test_mixed_legacy_and_new_overrides_rejected(tmp_path):
    """Explain conflicting file options before running external tools."""
    config = tmp_path / "blitzline.toml"
    config.write_text(
        '[blitzer]\nknown_file="old.txt"\nskip_exact_words_file="new.txt"\n'
    )
    with pytest.raises(ValueError, match="legacy blitzer.known_file"):
        get_config(config)


def test_arguments_forward_both_lists_without_updates(settings, tmp_path):
    """The two new files are forwarded independently with all writes disabled."""
    options = settings["blitzer"]
    options.update(
        skip_exact_words_file="exact.txt", skip_word_families_file="families.txt"
    )
    args = blitzer.arguments("eng", tmp_path / "text.txt", options)
    assert args[args.index("--skip-exact-words-file") + 1] == "exact.txt"
    assert args[args.index("--skip-word-families-file") + 1] == "families.txt"
    assert "--known-file" not in args and "--filter-by" not in args
    assert "--no-update-known" in args and "--no-save-context" in args


def test_config_discovery_matches_blitzer(settings, tmp_path, monkeypatch):
    """Use bltzr.toml with XDG, home, explicit, environment and no-config precedence."""
    options = settings["blitzer"]
    options["no_config"] = False
    home = Path.home() / ".config" / "bltzr" / "bltzr.toml"
    home.parent.mkdir(parents=True)
    home.write_text("")
    xdg = tmp_path / "xdg" / "bltzr" / "bltzr.toml"
    xdg.parent.mkdir(parents=True)
    (xdg.parent / "config.toml").write_text("obsolete filename")
    assert blitzer.selected_config(options) == home
    xdg.write_text("")
    assert blitzer.selected_config(options) == xdg
    monkeypatch.setenv("CONFIG_DIR", str(tmp_path))
    monkeypatch.setenv("BLITZER_CONFIG", "$CONFIG_DIR/selected.toml")
    assert blitzer.selected_config(options) == tmp_path / "selected.toml"
    options["config"] = str(tmp_path / "explicit.toml")
    assert blitzer.selected_config(options) == tmp_path / "explicit.toml"
    options["no_config"] = True
    assert blitzer.selected_config(options) is None


def test_native_config_fallback(settings, tmp_path, monkeypatch):
    """Without XDG, prefer the home file and then the native platform file."""
    options = settings["blitzer"]
    options["no_config"] = False
    monkeypatch.delenv("XDG_CONFIG_HOME")
    native = tmp_path / "native"
    native.mkdir()
    monkeypatch.setattr(blitzer, "user_config_dir", lambda app: str(native))
    (native / "bltzr.toml").write_text("")
    assert blitzer.selected_config(options) == native / "bltzr.toml"
    home = Path.home() / ".config" / "bltzr" / "bltzr.toml"
    home.parent.mkdir(parents=True)
    home.write_text("")
    assert blitzer.selected_config(options) == home


def test_dependencies_track_only_selected_lists(settings, tmp_path):
    """An override replaces one list; the other and missing files stay tracked."""
    options = settings["blitzer"]
    directory = tmp_path / "config"
    directory.mkdir()
    config = directory / "bltzr.toml"
    config.write_text(
        '[languages.eng]\nskip_exact_words_file="exact.txt"\n'
        'skip_word_families_file="families.txt"\n'
    )
    options.update(no_config=False, config=str(config))
    original = blitzer.dependencies("eng", options, version_runner)
    exact, families = directory / "exact.txt", directory / "families.txt"
    assert original["files"][str(exact)] == "missing"
    assert original["files"][str(families)] == "missing"
    exact.write_text("be\nam\n")
    changed = blitzer.dependencies("eng", options, version_runner)
    assert digest(original) != digest(changed)
    replacement = tmp_path / "replacement.txt"
    replacement.write_text("cat\n")
    options["skip_exact_words_file"] = str(replacement)
    selected = blitzer.dependencies("eng", options, version_runner)
    assert str(exact) not in selected["files"]
    assert str(replacement) in selected["files"]
    assert str(families) in selected["files"]
    exact.write_text("unused change\n")
    assert selected == blitzer.dependencies("eng", options, version_runner)
    families.write_text("be\n")
    assert selected != blitzer.dependencies("eng", options, version_runner)


def test_new_flags_suppress_legacy_config_dependencies(settings, tmp_path):
    """Mirror Blitzer's switch to new lists when given an explicit new file."""
    config = tmp_path / "bltzr.toml"
    config.write_text(
        '[languages.eng]\nknown_file="old.txt"\n'
        'exclusions=["extra.txt"]\nforms_only=["forms.txt"]\n'
    )
    options = settings["blitzer"]
    options.update(no_config=False, config=str(config))
    legacy = blitzer.dependencies("eng", options, version_runner)["files"]
    assert all(
        str(tmp_path / name) in legacy for name in ("old.txt", "extra.txt", "forms.txt")
    )
    options["skip_exact_words_file"] = str(tmp_path / "exact.txt")
    modern = blitzer.dependencies("eng", options, version_runner)["files"]
    assert str(tmp_path / "exact.txt") in modern
    assert all(
        str(tmp_path / name) not in modern
        for name in ("old.txt", "extra.txt", "forms.txt")
    )


def test_real_extraction_exact_words_and_to_be_family(settings, tmp_path):
    """Count other spellings after exact skips, and remove the whole be family."""
    require_blitzer()
    exact, families = tmp_path / "exact.txt", tmp_path / "families.txt"
    exact.write_text("be\nam\nsally-anne\n")
    families.write_text("")
    source = tmp_path / "text.txt"
    source.write_text("Be am. Is are was were being been. Cats.")
    options = settings["blitzer"]
    options.update(
        skip_exact_words_file=str(exact), skip_word_families_file=str(families)
    )
    rows = blitzer.extract("eng", source, options)
    assert {row["term"]: row["count"] for row in rows} == {"be": 6, "cat": 1}
    assert (
        next(row for row in rows if row["term"] == "be")["contexts"][0]["text"]
        == "Is are was were being been."
    )
    families.write_text("be\n")
    rows = blitzer.extract("eng", source, options)
    assert [(row["term"], row["count"]) for row in rows] == [("cat", 1)]
    assert exact.read_text() == "be\nam\nsally-anne\n"
    assert families.read_text() == "be\n"


@pytest.mark.parametrize(
    "setting,word",
    [("skip_exact_words_file", "cats"), ("skip_word_families_file", "cat")],
)
def test_discovered_skip_list_change_refreshes_pipeline(
    settings, recording, tmp_path, setting, word
):
    """Re-read a discovered list on resume and reject stale exports after its edit."""
    require_blitzer()
    configure_llm(settings)
    config = tmp_path / "xdg" / "bltzr" / "bltzr.toml"
    config.parent.mkdir(parents=True)
    list_file = config.parent / "skip.txt"
    list_file.write_text("")
    history = tmp_path / "history.db"
    kind = "exact-words" if setting == "skip_exact_words_file" else "word-families"
    config.write_text(
        f'[locations]\nhistory_file="{history}"\n'
        '[defaults]\nsave_context="always"\nauto_update_known=true\n'
        f'[languages.eng]\n{setting}="skip.txt"\nupdate_list="{kind}"\n'
    )
    settings["blitzer"]["no_config"] = False
    source = tmp_path / "transcript.json"
    write_json(source, {"segments": [{"start": 0.1, "end": 1.9, "text": "Cats run."}]})
    directory = run_pipeline(
        recording, "eng", settings, asr_json=source, requester=approve_request
    )
    before = read_json(directory / "artifacts/04-vocabulary.json")
    assert len(before) == 2
    assert list_file.read_text() == "" and not history.exists()
    list_file.write_text(word + "\n")
    with pytest.raises(PipelineError, match="Blitzer inputs changed"):
        export_run(directory)
    resume_pipeline(directory, requester=approve_request)
    after = read_json(directory / "artifacts/04-vocabulary.json")
    assert len(after) == 1
    assert list_file.read_text() == word + "\n" and not history.exists()
