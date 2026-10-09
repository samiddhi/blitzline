"""Verify shared card content, portable exports, and recoverable Anki delivery.

Scope statement: exercise export adapters against temporary media and fake Anki.
Included: escaped fields, TSV empty cells, APKG contents when installed,
idempotent direct delivery, scheduling-safe updates, and partial receipts.
Excluded: a real Anki profile, real network access, and transcription.
Start here: FakeAnki models the actual request protocol used by deliver.
"""

import json
import zipfile
from dataclasses import replace

import pytest

from blitzline.exports.anki import deliver
from blitzline.exports.common import fields
from blitzline.exports.package import export_package
from blitzline.exports.tsv import export_tsv
from blitzline.records import Card, Context, PipelineError
from blitzline.storage import read_json


def card_fixture(tmp_path):
    """Return one complete card and a temporary media directory."""
    media = tmp_path / "media"
    media.mkdir()
    (media / "blitzline-test.mp3").write_bytes(b"fake media bytes")
    context = Context("example", "Č & <word>.", 4, 10, ("cue",), 0, 1000)
    card = Card(
        "blitzline-123",
        "word",
        "meaning\t<&>",
        "",
        1,
        (context,),
        "source",
        ("blitzline-test.mp3",),
    )
    return card, media


class FakeAnki:
    """Simulate AnkiConnect with notes, model fields, and optional transport failure."""

    def __init__(self):
        """Initialize an empty isolated collection and action log."""
        self.notes = {}
        self.models = {}
        self.calls = []
        self.templates = {}
        self.styling = {}
        self.media = []
        self.fail_after_add = False

    def __call__(self, url, payload, headers, timeout):
        """Dispatch the exact request actions used by the delivery adapter."""
        action, params = payload["action"], payload["params"]
        self.calls.append(action)
        result = self.action(action, params)
        return {"result": result, "error": None}

    def action(self, action, params):
        """Apply one simulated collection operation without production helpers."""
        if action == "version":
            return 6
        if action == "modelNames":
            return list(self.models)
        if action == "modelFieldNames":
            return self.models[params["modelName"]]
        if action == "createDeck":
            return 1
        if action == "createModel":
            self.models[params["modelName"]] = params["inOrderFields"]
            self.templates[params["modelName"]] = {
                t["Name"]: {"Front": t["Front"], "Back": t["Back"]}
                for t in params["cardTemplates"]
            }
            self.styling[params["modelName"]] = params["css"]
            return 1
        if action == "modelTemplates":
            return self.templates[params["modelName"]]
        if action == "updateModelTemplates":
            self.templates[params["model"]["name"]].update(params["model"]["templates"])
            return None
        if action == "updateModelStyling":
            self.styling[params["model"]["name"]] = params["model"]["css"]
            return None
        if action == "storeMediaFile":
            self.media.append(params["filename"])
            return params["filename"]
        if action == "findNotes":
            return [
                i
                for i, note in self.notes.items()
                if ('"note:' + note["modelName"] + '"') in params["query"]
                and (
                    any('"tag:' + tag + '"' in params["query"] for tag in note["tags"])
                    or (
                        '"BlitzlineId:'
                        + note["fields"].get("BlitzlineId", "__missing__")
                        + '"'
                    )
                    in params["query"]
                )
            ]
        if action == "addNote":
            identity = len(self.notes) + 1
            self.notes[identity] = params["note"]
            if self.fail_after_add:
                self.fail_after_add = False
                raise PipelineError("Lost response after add")
            return identity
        if action == "updateNoteFields":
            row = params["note"]
            self.notes[row["id"]]["fields"].update(row["fields"])
            return None
        raise AssertionError(action)


def test_tsv_blanks_and_escaping(tmp_path, settings):
    """Preserve empty cells and escape untrusted text without losing media."""
    card, media = card_fixture(tmp_path)
    paths = export_tsv([card], media, tmp_path / "export", settings["export"])
    raw = paths[0].read_text()
    assert "meaning&#9;&lt;&amp;&gt;" in raw
    assert "\t\t" in raw
    assert fields(card)["Sentence1"] == "Č &amp; <b>&lt;word&gt;</b>."
    assert (tmp_path / "export/media/blitzline-test.mp3").is_file()


def test_anki_idempotence_and_update(tmp_path, settings):
    """Skip repeat delivery and update fields without rebuilding existing notes."""
    card, media = card_fixture(tmp_path)
    transport = FakeAnki()
    destination = tmp_path / "export"
    deliver([card], media, destination, settings["export"], transport)
    deliver([card], media, destination, settings["export"], transport)
    assert len(transport.notes) == 1
    settings["export"]["existing"] = "update"
    deliver(
        [replace(card, english="new meaning")],
        media,
        destination,
        settings["export"],
        transport,
    )
    assert transport.notes[1]["fields"]["Translation"] == "new meaning"
    assert transport.calls.count("addNote") == 1
    assert transport.calls.count("updateNoteFields") == 1


def test_lost_anki_response_recovered(tmp_path, settings):
    """Record failure and recover by looking up the note created before a lost reply."""
    card, media = card_fixture(tmp_path)
    transport = FakeAnki()
    transport.fail_after_add = True
    destination = tmp_path / "export"
    with pytest.raises(PipelineError):
        deliver([card], media, destination, settings["export"], transport)
    assert read_json(destination / "anki-delivery.json")["state"] == "failed"
    deliver([card], media, destination, settings["export"], transport)
    assert len(transport.notes) == 1
    assert read_json(destination / "anki-delivery.json")["state"] == "complete"


def test_incompatible_anki_model_no_mutation(tmp_path, settings):
    """Refuse a mismatched note type before creating a deck or uploading media."""
    card, media = card_fixture(tmp_path)
    transport = FakeAnki()
    transport.models[settings["export"]["model"]] = ["Different"]
    with pytest.raises(PipelineError, match="incompatible"):
        deliver([card], media, tmp_path / "export", settings["export"], transport)
    assert "createDeck" not in transport.calls
    assert "storeMediaFile" not in transport.calls


def test_apkg_archive(tmp_path, settings):
    """Inspect actual generated APKG notes/media when the optional writer is present."""
    pytest.importorskip("genanki")
    card, media = card_fixture(tmp_path)
    path = export_package([card], media, tmp_path / "export", settings["export"])[0]
    with zipfile.ZipFile(path) as archive:
        assert "collection.anki2" in archive.namelist()
        assert list(json.loads(archive.read("media")).values()) == [
            "blitzline-test.mp3"
        ]
