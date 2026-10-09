"""Verify note presets and customization across real portable exports and fake Anki.

Scope statement: exercise note schemas, card generation, and presentation inputs.
Included: default conditional cards, custom mappings/assets/templates, TSV parity,
model update policy, legacy identity, and file-aware export cache invalidation.
Excluded: live Anki profiles, scheduling engines, and paid provider calls.
Start here: test_default_card_generation inspects actual generated APKG SQLite.
"""

import json
import sqlite3
import zipfile
from dataclasses import replace

import pytest
from test_exports import FakeAnki, card_fixture

from blitzline.config import get_config, upgrade_settings, validate_config
from blitzline.core import export_stage
from blitzline.exports.anki import deliver
from blitzline.exports.common import FIELDS, model_identity
from blitzline.exports.model import model_id, render_fields, resolve_model
from blitzline.exports.package import export_package
from blitzline.exports.tsv import export_tsv
from blitzline.records import PipelineError
from blitzline.storage import RunStore, read_json


def package_database(path, tmp_path):
    """Extract only the generated package database for independent SQL assertions."""
    with zipfile.ZipFile(path) as archive:
        database_path = tmp_path / "collection.db"
        database_path.write_bytes(archive.read("collection.anki2"))
    return sqlite3.connect(database_path)


@pytest.mark.parametrize(
    "examples,two_way,ordinals",
    [(1, False, [1]), (2, False, [1, 2]), (1, True, [0, 1])],
)
def test_default_card_generation(tmp_path, settings, examples, two_way, ordinals):
    """Generate sentence cards only for available examples and opt-in recall."""
    pytest.importorskip("genanki")
    card, media = card_fixture(tmp_path)
    if examples == 2:
        card = replace(card, contexts=card.contexts * 2, media=card.media * 2)
    settings["export"]["two_way"] = two_way
    path = export_package([card], media, tmp_path / "export", settings["export"])[0]
    with package_database(path, tmp_path) as db:
        assert [
            r[0] for r in db.execute("select ord from cards order by ord")
        ] == ordinals
        model = next(
            iter(
                json.loads(db.execute("select models from col").fetchone()[0]).values()
            )
        )
        assert [f["name"] for f in model["flds"]] == [
            "Word",
            "Translation",
            "Sentence1",
            "Sentence2",
            "Comments",
            "Frequency",
            "Sentence1 Audio",
            "Sentence2 Audio",
            "Word Audio",
            "Image",
            "FillForTwoWay",
        ]
        assert [t["name"] for t in model["tmpls"]] == [
            "Recall",
            "Sentence1",
            "Sentence2",
        ]
        assert "Avenir Next" in model["css"] and "35px" in model["css"]
        assert "showBoldOnly" in model["tmpls"][1]["qfmt"]
        values = db.execute("select flds,tags from notes").fetchone()
        assert values[0].split("\x1f")[1] == "meaning&#9;&lt;&amp;&gt;"
        assert "blitzline_id_" + card.id in values[1]
        assert "blitzline_source_source" in values[1]


def test_custom_definition_parity(tmp_path, settings):
    """Use arbitrary fields, two templates, tags, and assets in all three formats."""
    pytest.importorskip("genanki")
    card, media = card_fixture(tmp_path)
    asset = tmp_path / "picture.png"
    asset.write_bytes(b"example asset")
    options = settings["export"]
    options.update(
        model="Custom vocabulary",
        fields=[
            {"name": "Prompt", "format": "<div>$Word</div>$Sentence1"},
            {"name": "Answer", "source": "Translation"},
            {"name": "Picture", "value": '<img src="picture.png">', "html": True},
            {"name": "Extra", "value": "<untrusted>"},
            {"name": "Recording", "source": "Source"},
        ],
        templates=[
            {
                "name": "Forward",
                "front": "{{Prompt}}",
                "back": "{{FrontSide}}<hr>{{Answer}}{{Picture}}{{Extra}}",
            },
            {"name": "Reverse", "front": "{{Answer}}", "back": "{{Prompt}}"},
            {"name": "Disabled", "enabled": False},
        ],
        css=".card { color: purple; }",
        assets=[str(asset)],
        tags=["topic", "word_{word}"],
    )
    validate_config(settings)
    definition = resolve_model(options)
    values = render_fields(card, options, definition)
    assert values["Extra"] == "&lt;untrusted&gt;"
    assert values["Picture"] == '<img src="picture.png">'
    tsv = export_tsv([card], media, tmp_path / "tsv", options)
    description = read_json(tmp_path / "tsv/note-type.json")
    assert description["fields"] == list(values)
    assert len(description["templates"]) == 2
    assert tsv[0].read_text().splitlines()[2] == "#columns:" + "\t".join(
        [*values, "Tags"]
    )
    package = export_package([card], media, tmp_path / "apkg", options)[0]
    with package_database(package, tmp_path) as db:
        assert db.execute("select flds from notes").fetchone()[0].split("\x1f") == list(
            values.values()
        )
        assert db.execute("select count(*) from cards").fetchone()[0] == 2
    with zipfile.ZipFile(package) as archive:
        assert "picture.png" in json.loads(archive.read("media")).values()
    transport = FakeAnki()
    deliver([card], media, tmp_path / "anki", options, transport)
    assert transport.notes[1]["fields"] == values
    assert "word_word" in transport.notes[1]["tags"]
    assert "picture.png" in transport.media
    assert transport.templates[options["model"]] == {
        t["name"]: {"Front": t["front"], "Back": t["back"]}
        for t in definition["templates"]
    }


def test_existing_model_styles_are_opt_in(tmp_path, settings):
    """Preserve existing styling until explicit model updates are requested."""
    card, media = card_fixture(tmp_path)
    transport = FakeAnki()
    deliver([card], media, tmp_path / "export", settings["export"], transport)
    settings["export"]["css"] = ".card { color: red; }"
    deliver([card], media, tmp_path / "export", settings["export"], transport)
    assert "updateModelStyling" not in transport.calls
    settings["export"]["update_model"] = True
    deliver([card], media, tmp_path / "export", settings["export"], transport)
    assert transport.styling[settings["export"]["model"]] == settings["export"]["css"]
    assert len(transport.notes) == 1 and "updateNoteFields" not in transport.calls
    transport.templates[settings["export"]["model"]] = {"Different": {}}
    transport.calls.clear()
    with pytest.raises(PipelineError, match="templates differ"):
        deliver([card], media, tmp_path / "export", settings["export"], transport)
    assert (
        "createDeck" not in transport.calls and "storeMediaFile" not in transport.calls
    )


@pytest.mark.parametrize("name", ["Blitzline v1", "Existing custom model"])
def test_legacy_saved_settings(settings, name):
    """Upgrade old manifests without changing their model schema or numeric ID."""
    from blitzline.exports.common import BACK, CSS, FRONT

    previous = settings.copy()
    previous["export"] = {
        "format": "tsv",
        "deck": "Blitzline",
        "model": name,
        "anki_url": "http://127.0.0.1:8765",
        "existing": "skip",
        "front_template": FRONT,
        "back_template": BACK,
        "css": CSS,
    }
    previous["asr"] = {
        k: v
        for k, v in settings["asr"].items()
        if k
        not in {
            "backend",
            "endpoint",
            "api_key_env",
            "modal_app",
            "modal_function",
            "modal_environment",
            "remote_compute_type",
            "chunk_seconds",
            "retries",
            "options",
        }
    }
    upgraded = upgrade_settings(previous)
    validate_config(upgraded)
    definition = resolve_model(upgraded["export"])
    assert upgraded["asr"]["backend"] == "local"
    assert [f["name"] for f in definition["fields"]] == FIELDS
    assert model_id(name, definition) == model_identity(name)


def test_template_paths_and_export_invalidation(tmp_path, settings):
    """Load relative template/CSS files and regenerate exports after edits."""
    card, media = card_fixture(tmp_path)
    (tmp_path / "front.html").write_text("{{Word}}")
    (tmp_path / "back.html").write_text("{{Translation}}")
    (tmp_path / "style.css").write_text(".card { color: teal; }")
    config = tmp_path / "config.toml"
    config.write_text("""[export]
css_file = "style.css"
[[export.templates]]
name = "Custom"
front_file = "front.html"
back_file = "back.html"
""")
    options = get_config(config)["export"]
    settings["export"] = options
    store = RunStore(tmp_path / "run", {})
    store_media = store.directory / "media"
    store_media.mkdir(exist_ok=True)
    (store_media / card.media[0]).write_bytes((media / card.media[0]).read_bytes())
    export_stage(store, [card], settings)
    first = store.manifest["stages"]["07-export-tsv"]["key"]
    (tmp_path / "front.html").write_text("{{Word}}<p>Edited</p>")
    export_stage(store, [card], settings)
    assert store.manifest["stages"]["07-export-tsv"]["key"] != first
    assert (
        "Edited" in (store.directory / "exports/tsv/templates/1-front.html").read_text()
    )


@pytest.mark.parametrize(
    "change",
    [
        {"fields": [{"name": "Word", "source": "typo"}]},
        {"templates": [{"name": "Bad", "front": "{{Missing}}", "back": "{{Word}}"}]},
        {
            "fields": [
                {"name": "Word", "source": "Word"},
                {"name": "word", "source": "Word"},
            ]
        },
        {"tags": ["{word.__class__}"]},
        {"templates": "bad"},
    ],
)
def test_invalid_customization(settings, change):
    """Reject unusable definitions before any note or remote API mutation."""
    settings["export"].update(change)
    with pytest.raises(ValueError):
        validate_config(settings)


def test_sentence_click_script_preserves_escaping(settings):
    """Execute the shipped JavaScript through toggles, missing elements, and plain text."""
    import re
    import shutil
    import subprocess

    node = shutil.which("node")
    if not node:
        pytest.skip("Node required for sentence JavaScript execution")
    definition = resolve_model(settings["export"])
    scripts = [
        re.search(r"<script>(.*?)</script>", t["front"], re.S).group(1)
        for t in definition["templates"]
        if t["name"].startswith("Sentence")
    ]
    harness = r"""
const assert = require('node:assert/strict');
const vm = require('node:vm');
const scripts = JSON.parse(require('node:fs').readFileSync(0, 'utf8'));
for (const script of scripts) {
  vm.runInNewContext(script, {document: {getElementById: () => null}});
  for (const highlighted of [true, false]) {
    const bold = '<b>&lt;img src=x onerror=alert(1)&gt;</b>';
    const full = highlighted ? 'Before ' + bold + ' after.' : 'No bold word.';
    const classes = new Set();
    let click;
    const el = {
      innerHTML: full,
      querySelectorAll: () => highlighted ? [{outerHTML: bold, textContent: '<img src=x onerror=alert(1)>'}] : [],
      classList: {add: x => classes.add(x), remove: x => classes.delete(x), contains: x => classes.has(x)},
      addEventListener: (name, handler) => { assert.equal(name, 'click'); click = handler; }
    };
    vm.runInNewContext(script, {document: {getElementById: () => el}});
    assert.equal(el.innerHTML, highlighted ? bold : full);
    click();
    assert.equal(el.innerHTML, full);
    click();
    assert.equal(el.innerHTML, highlighted ? bold : full);
    assert.ok(!el.innerHTML.includes('<img'));
  }
}
"""
    subprocess.run(
        [node, "-e", harness],
        input=json.dumps(scripts),
        text=True,
        capture_output=True,
        check=True,
    )


def test_select_preset_template_by_name(settings):
    """Retain an individual preset template without copying its HTML into config."""
    settings["export"]["templates"] = [{"name": "Sentence1"}]
    definition = resolve_model(settings["export"])
    assert [t["name"] for t in definition["templates"]] == ["Sentence1"]
    assert "showBoldOnly" in definition["templates"][0]["front"]


def test_apkg_typed_answer_filter(tmp_path, settings):
    """Create a typed-answer-only card while preserving the actual Anki filter syntax."""
    pytest.importorskip("genanki")
    card, media = card_fixture(tmp_path)
    settings["export"].update(
        fields=[{"name": "Prompt", "source": "Word"}],
        templates=[
            {"name": "Typed", "front": "{{type:Prompt}}", "back": "{{type:Prompt}}"}
        ],
    )
    path = export_package([card], media, tmp_path / "export", settings["export"])[0]
    with package_database(path, tmp_path) as db:
        assert db.execute("select count(*) from cards").fetchone()[0] == 1
        model = next(
            iter(
                json.loads(db.execute("select models from col").fetchone()[0]).values()
            )
        )
        assert model["tmpls"][0]["qfmt"] == "{{type:Prompt}}"
