"""Verify explicit language and card-type routing for users with multiple decks.

Scope statement: test deck profile selection across configuration, CLI, and exports.
Included: multilingual routing, selected template sets, language metadata, profile
inference, ambiguity/mismatch errors, per-profile files, and shared model conflicts.
Excluded: real speech recognizers, Anki profiles, and remote provider calls.
Start here: test_language_decks_export verifies two real packages from one config.
"""

import json
from dataclasses import replace

import pytest
from click.testing import CliRunner
from test_exports import FakeAnki, card_fixture
from test_note_models import package_database

from blitzline.cli import cli
from blitzline.config import (
    get_config,
    resolve_export,
    resolve_language,
    validate_config,
)
from blitzline.core import export_stage, run_pipeline
from blitzline.exports.anki import deliver
from blitzline.exports.model import resolve_model
from blitzline.exports.package import export_package
from blitzline.storage import RunStore, read_json


def deck_profiles(settings):
    """Configure two source languages with deliberately different card selections."""
    settings["decks"] = {
        "slovenian": {
            "language": "slv",
            "deck": "Languages::Slovenian",
            "card_types": ["Sentence1", "Sentence2"],
        },
        "polish": {
            "language": "pol",
            "deck": "Languages::Polish",
            "card_types": ["Recall", "Sentence1"],
        },
    }
    return settings


def test_language_decks_export(tmp_path, settings):
    """Route each language to its configured deck and generate only selected cards."""
    pytest.importorskip("genanki")
    deck_profiles(settings)
    validate_config(settings)
    card, media = card_fixture(tmp_path)
    card = replace(card, contexts=card.contexts * 2, media=card.media * 2)
    transport = FakeAnki()
    for language, profile, expected in [
        ("slv", "slovenian", ["Sentence1", "Sentence2"]),
        ("pol", "polish", ["Recall", "Sentence1"]),
    ]:
        options = resolve_export(settings, language)
        assert options["deck_profile"] == profile
        assert options["model"].endswith("[" + profile + "]")
        assert [t["name"] for t in resolve_model(options)["templates"]] == expected
        path = export_package([card], media, tmp_path / profile, options)[0]
        with package_database(path, tmp_path) as db:
            model = next(
                iter(
                    json.loads(
                        db.execute("select models from col").fetchone()[0]
                    ).values()
                )
            )
            assert [t["name"] for t in model["tmpls"]] == expected
            deck = next(
                d
                for d in json.loads(
                    db.execute("select decks from col").fetchone()[0]
                ).values()
                if d["name"] == options["deck"]
            )
            assert deck["name"] == settings["decks"][profile]["deck"]
            assert db.execute("select count(*) from cards").fetchone()[0] == 2
            assert (
                "blitzline_language_" + language
                in db.execute("select tags from notes").fetchone()[0]
            )
        deliver([card], media, tmp_path / (profile + "-anki"), options, transport)
    assert len(transport.notes) == 2
    assert {n["deckName"] for n in transport.notes.values()} == {
        "Languages::Slovenian",
        "Languages::Polish",
    }


def test_profile_selection_and_language_inference(settings):
    """Reject missing/ambiguous routes and mismatched explicitly selected languages."""
    deck_profiles(settings)
    settings["export"]["deck_profile"] = "polish"
    assert resolve_language(settings, None) == "pol"
    with pytest.raises(ValueError, match="is for pol"):
        resolve_export(settings, "slv")
    settings["export"]["deck_profile"] = ""
    with pytest.raises(ValueError, match="No deck profile"):
        resolve_export(settings, "eng")
    settings["decks"]["slovenian-listening"] = {
        "language": "slv",
        "deck": "Slovenian listening",
        "card_types": ["Sentence1"],
    }
    with pytest.raises(ValueError, match="Multiple deck profiles"):
        resolve_export(settings, "slv")
    settings["export"]["deck_profile"] = "slovenian-listening"
    assert resolve_export(settings, "slv")["deck"] == "Slovenian listening"


def test_cli_profile_language(recording, tmp_path):
    """Infer source language from the selected profile and preserve it on resume."""
    config = tmp_path / "config.toml"
    config.write_text("""[decks.slovenian]
language = "slv"
deck = "Slovenian"
card_types = ["Sentence1", "Sentence2"]
[decks.polish]
language = "pol"
deck = "Polish"
card_types = ["Sentence1"]
""")
    subtitles = tmp_path / "source.srt"
    subtitles.write_text("1\n00:00:00,100 --> 00:00:00,400\nMačka spi.\n\n")
    directory = tmp_path / "run"
    runner = CliRunner()
    result = runner.invoke(
        cli,
        [
            "run",
            str(recording),
            "--config",
            str(config),
            "--deck-profile",
            "slovenian",
            "--output",
            str(directory),
            "--srt",
            str(subtitles),
            "--stop-after",
            "transcript",
        ],
    )
    assert result.exit_code == 0, result.output
    assert read_json(directory / "manifest.json")["request"]["language"] == "slv"
    mismatch = runner.invoke(
        cli,
        [
            "resume",
            str(directory),
            "--deck-profile",
            "polish",
            "--stop-after",
            "transcript",
        ],
    )
    assert mismatch.exit_code == 2 and "is for pol" in mismatch.output


def test_profile_paths_and_language_field(tmp_path, settings):
    """Resolve profile-relative assets/templates and map configured language to a field."""
    card, _ = card_fixture(tmp_path)
    (tmp_path / "front.html").write_text("{{Word}}")
    config = tmp_path / "config.toml"
    config.write_text("""[decks.slovenian]
language = "slv"
deck = "Slovenian"
card_types = ["Custom"]
[[decks.slovenian.fields]]
name = "Word"
source = "Word"
[[decks.slovenian.fields]]
name = "Language"
source = "Language"
[[decks.slovenian.templates]]
name = "Custom"
front_file = "front.html"
back = "{{Word}} {{Language}}"
""")
    configured = get_config(config)
    options = resolve_export(configured, "slv")
    from blitzline.exports.model import render_fields

    assert render_fields(card, options, resolve_model(options))["Language"] == "slv"
    assert configured["decks"]["slovenian"]["templates"][0]["front_file"] == str(
        tmp_path / "front.html"
    )


def test_profile_export_stage_metadata(tmp_path, settings):
    """Apply the run's language to the actual export adapter and record its destination."""
    deck_profiles(settings)
    card, media = card_fixture(tmp_path)
    store = RunStore(tmp_path / "run", {"request": {"language": "slv"}})
    (store.directory / "media" / card.media[0]).write_bytes(
        (media / card.media[0]).read_bytes()
    )
    result = export_stage(store, [card], settings)
    assert (
        result["deck_profile"] == "slovenian"
        and result["deck"] == "Languages::Slovenian"
    )
    definition = read_json(store.directory / "exports/tsv/note-type.json")
    assert [t["name"] for t in definition["templates"]] == ["Sentence1", "Sentence2"]
    assert (
        definition["deck"] == "Languages::Slovenian" and definition["language"] == "slv"
    )


@pytest.mark.parametrize(
    "profile",
    [
        {"deck": "Bad", "card_types": ["Sentence1"]},
        {"language": "slv", "deck": "Bad"},
        {"language": "slv", "deck": "Bad", "card_types": ["Unknown"]},
        {"language": "slv", "deck": "Bad", "card_types": ["Sentence1", "Sentence1"]},
        {"language": "sl", "deck": "Bad", "card_types": ["Sentence1"]},
    ],
)
def test_invalid_deck_profile(settings, profile):
    """Require an explicit source language, deck name, and valid card-type selection."""
    settings["decks"] = {"bad": profile}
    with pytest.raises(ValueError):
        validate_config(settings)


def test_deck_language_and_shared_model_conflicts(settings):
    """Prevent one deck serving conflicting languages or one model incompatible layouts."""
    deck_profiles(settings)
    settings["decks"]["polish"]["deck"] = "Languages::Slovenian"
    with pytest.raises(ValueError, match="multiple languages"):
        validate_config(settings)
    settings["decks"]["polish"]["deck"] = "Polish"
    for profile in settings["decks"].values():
        profile["model"] = "Shared model"
    with pytest.raises(ValueError, match="conflicting definitions"):
        validate_config(settings)


def test_profile_mismatch_before_processing(tmp_path, settings):
    """Reject language/deck mistakes before even looking for the source file."""
    deck_profiles(settings)
    settings["export"]["deck_profile"] = "slovenian"
    with pytest.raises(ValueError, match="is for slv"):
        run_pipeline(tmp_path / "missing.mp4", "pol", settings)
