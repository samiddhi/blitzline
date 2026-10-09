"""Resolve configurable Anki note definitions shared by every export adapter.

Scope statement: own presets, field mappings, templates, tags, and model assets.
Included: Blitzer/legacy defaults, file overrides, validation, deterministic
model identity, and rendering controlled HTML from escaped card data.
Excluded: note delivery, scheduling, media generation, and ASR.
Start here: resolve_model returns a complete definition; render_fields maps a card.
"""

import json
import re
from copy import deepcopy
from importlib.resources import files
from pathlib import Path
from string import Formatter, Template
from urllib.parse import quote

from blitzline.exports.common import (
    BACK,
    CSS,
    FIELDS,
    FRONT,
    escaped,
    fields,
    media_paths,
)
from blitzline.records import text
from blitzline.storage import digest, fingerprint

SOURCES = set(FIELDS) | {
    "Translation",
    "Comments",
    "Sentence1 Audio",
    "Sentence2 Audio",
    "Word Audio",
    "Image",
    "FillForTwoWay",
    "Language",
}
SPECIAL_FIELDS = {"FrontSide", "Tags", "Type", "Deck", "Subdeck", "Card", "CardFlag"}


def preset_definition(name: str) -> dict:
    """Load a bundled preset independent of the example archive at runtime."""
    if name == "blitzer":
        value = json.loads(
            files("blitzline.exports").joinpath("presets/blitzer.json").read_text()
        )
        value["fields"] = [{"name": n, "source": n} for n in value["fields"]]
        return value
    if name == "legacy":
        return {
            "fields": [{"name": n, "source": n} for n in FIELDS],
            "templates": [{"name": "Vocabulary", "front": FRONT, "back": BACK}],
            "css": CSS,
        }
    raise ValueError("export.preset must be blitzer or legacy")


def file_text(inline, filename: str, label: str):
    """Read a configured UTF-8 file or inline text, rejecting ambiguous overrides."""
    if filename and inline is not None:
        raise ValueError(f"Choose either {label} or {label}_file")
    if filename:
        path = Path(text(filename, label + "_file"))
        if not path.is_file():
            raise ValueError(f"Missing {label} file: {path}")
        return path.read_text(encoding="utf-8")
    if inline is not None:
        return text(inline, label, empty=True)
    return None


def validate_template(value: str, names: list[str], name: str) -> None:
    """Check field references and balanced Anki conditional sections."""
    stack = []
    for token in re.findall(r"{{\s*([^{}]+?)\s*}}", value):
        marker = token[0]
        reference = token.lstrip("#^/").split(":")[-1].strip()
        if reference not in names and reference not in SPECIAL_FIELDS:
            raise ValueError(f"Template {name}: unknown field {reference!r}")
        if marker in {"#", "^"}:
            stack.append(reference)
        elif marker == "/":
            if not stack or stack.pop() != reference:
                raise ValueError(
                    f"Template {name}: unbalanced conditional {reference!r}"
                )
    if stack:
        raise ValueError(f"Template {name}: unclosed conditional {stack[-1]!r}")


def resolve_model(settings: dict) -> dict:
    """Resolve and validate ordered fields, card templates, CSS, and extra assets."""
    for key in ("fields", "templates", "assets", "tags", "card_types"):
        if not isinstance(settings[key], list):
            raise ValueError(f"export.{key} must be an array")
    model = preset_definition(settings["preset"])
    preset_templates = {t["name"]: t for t in model["templates"]}
    if settings["fields"]:
        model["fields"] = deepcopy(settings["fields"])
    names = []
    for field in model["fields"]:
        if not isinstance(field, dict) or field.keys() - {
            "name",
            "source",
            "value",
            "html",
            "format",
        }:
            raise ValueError("Invalid export.fields entry")
        name = text(field.get("name"), "field name")
        if (
            any(c in name for c in "{}:\t\r\n")
            or name in SPECIAL_FIELDS
            or name.startswith(("#", "/", "^"))
        ):
            raise ValueError(f"Invalid or reserved field name: {name}")
        names.append(name)
        if sum(k in field for k in ("source", "value", "format")) != 1:
            raise ValueError(
                f"Field {name}: choose exactly one source, value, or format"
            )
        if "source" in field and field["source"] not in SOURCES:
            raise ValueError(f"Unknown field source: {field['source']}")
        for key in ("value", "format"):
            if key in field:
                text(field[key], f"Field {name} {key}", empty=True)
        if "html" in field and type(field["html"]) is not bool:
            raise ValueError(f"Field {name}: html must be true or false")
        if "html" in field and "value" not in field:
            raise ValueError(f"Field {name}: html applies only to literal values")
        if "format" in field:
            try:
                Template(field["format"]).substitute({key: "" for key in SOURCES})
            except (ValueError, KeyError) as error:
                raise ValueError(f"Invalid format for field {name}: {error}") from None
    if len(set(n.casefold() for n in names)) != len(names):
        raise ValueError("export.fields names must be unique")
    if settings["templates"]:
        model["templates"] = deepcopy(settings["templates"])
    if settings["front_template"] is not None or settings["back_template"] is not None:
        if settings["templates"]:
            raise ValueError("Use templates or front_template/back_template, not both")
        model["templates"] = [
            {
                "name": "Vocabulary",
                "front": settings["front_template"]
                if settings["front_template"] is not None
                else model["templates"][0]["front"],
                "back": settings["back_template"]
                if settings["back_template"] is not None
                else model["templates"][0]["back"],
            }
        ]
    templates = []
    for entry in model["templates"]:
        if not isinstance(entry, dict) or entry.keys() - {
            "name",
            "front",
            "back",
            "front_file",
            "back_file",
            "enabled",
        }:
            raise ValueError("Invalid export.templates entry")
        name = text(entry.get("name"), "template name")
        if type(entry.get("enabled", True)) is not bool:
            raise ValueError(f"Template {name}: enabled must be true or false")
        if not entry.get("enabled", True):
            continue
        template = {"name": name}
        for side in ("front", "back"):
            value = file_text(
                entry.get(side),
                entry.get(side + "_file", ""),
                f"template {name} {side}",
            )
            if value is None:
                value = preset_templates.get(name, {}).get(side)
            text(value, f"Template {name} {side}")
            validate_template(value, names, name)
            template[side] = value
        templates.append(template)
    if not templates or len({t["name"] for t in templates}) != len(templates):
        raise ValueError(
            "Export requires at least one enabled template with unique names"
        )
    if settings["card_types"]:
        selected = [text(n, "card type name") for n in settings["card_types"]]
        by_name = {t["name"]: t for t in templates}
        if len(set(selected)) != len(selected) or any(
            n not in by_name for n in selected
        ):
            raise ValueError(
                "export.card_types must list unique enabled template names"
            )
        templates = [by_name[n] for n in selected]
    model["templates"] = templates
    css = file_text(settings["css"], settings["css_file"], "export.css")
    if css is not None:
        model["css"] = css
    model["sort_field"] = 0
    assets = []
    for filename in settings["assets"]:
        path = Path(text(filename, "export asset"))
        if not path.is_file() or not path.stat().st_size:
            raise ValueError(f"Missing or empty export asset: {path}")
        if any(c in path.name for c in '[]<>"\r\n\t'):
            raise ValueError(f"Invalid export asset filename: {path.name}")
        assets.append(path)
    if len({p.name for p in assets}) != len(assets):
        raise ValueError("Export asset filenames must be unique")
    model["assets"] = assets
    for tag in settings["tags"]:
        text(tag, "export tag")
        try:
            for _, field, spec, conversion in Formatter().parse(tag):
                if field is not None and (
                    field not in {"id", "word", "source", "language"}
                    or spec
                    or conversion
                ):
                    raise ValueError("Unsupported tag placeholder")
        except ValueError:
            raise ValueError(
                "Tags support only {id}, {word}, {source}, and {language}"
            ) from None
    return model


def render_fields(card, settings: dict, model: dict) -> dict[str, str]:
    """Map escaped canonical data or explicitly trusted literal HTML into fields."""
    data = fields(card)
    data.update(
        {
            "Translation": data["English"],
            "Comments": data["Notes"],
            "Sentence1 Audio": data["Media1"],
            "Sentence2 Audio": data["Media2"],
            "Word Audio": "",
            "Image": "",
            "FillForTwoWay": "1" if settings["two_way"] else "",
            "Language": escaped(settings.get("language", "")),
        }
    )
    result = {}
    for field in model["fields"]:
        if "source" in field:
            value = data[field["source"]]
        elif "format" in field:
            value = Template(field["format"]).substitute(data)
        else:
            value = (
                field["value"] if field.get("html", False) else escaped(field["value"])
            )
        result[field["name"]] = value
    return result


def note_tags(card, settings: dict) -> list[str]:
    """Render configured tags and retain a stable identity independent of fields."""
    tags = [
        re.sub(
            r"\s+",
            "_",
            tag.format(
                id=card.id,
                word=card.word,
                source=card.source,
                language=settings.get("language", ""),
            ),
        )
        for tag in settings["tags"]
    ]
    return list(
        dict.fromkeys(
            [
                *tags,
                "blitzline_id_" + card.id,
                "blitzline_source_" + quote(card.source, safe=""),
                *(
                    ["blitzline_language_" + settings["language"]]
                    if settings.get("language")
                    else []
                ),
            ]
        )
    )


def model_id(name: str, model: dict) -> int:
    """Keep model IDs stable across styling changes and distinguish schemas/cards."""
    names = [f["name"] for f in model["fields"]]
    identity = [name, names]
    if names != FIELDS or [t["name"] for t in model["templates"]] != ["Vocabulary"]:
        identity.append([t["name"] for t in model["templates"]])
    return int(digest(identity)[:8], 16) % (2**31 - 1) + 1


def export_dependencies(settings: dict) -> dict:
    """Include loaded template/style content and asset checksums in export caches."""
    model = resolve_model(settings)
    return {**model, "assets": {str(p): fingerprint(p) for p in model["assets"]}}


def export_media(cards, directory: Path, model: dict) -> list[Path]:
    """Combine validated generated clips and assets without filename collisions."""
    paths = media_paths(cards, directory)
    by_name = {p.name: p for p in paths}
    for path in model["assets"]:
        if path.name in by_name and path.resolve() != by_name[path.name].resolve():
            raise ValueError(f"Export asset collides with generated media: {path.name}")
        by_name[path.name] = path
    return sorted(by_name.values())
