"""Deliver notes through AnkiConnect with recoverable per-note outcomes.

Scope statement: own the supported local AnkiConnect boundary.
Included: version/model preflight, media upload, stable-ID lookup, skip/update
policy, note creation, and receipts saved after every successful operation.
Excluded: editing live SQLite/profile files, exporting packages (package.py),
choosing vocabulary (vocabulary.py), or modifying scheduling and unrelated fields.
Start here: deliver verifies prerequisites and then uploads media and notes.
"""

import base64
from pathlib import Path

from blitzline.exports.model import (
    export_media,
    note_tags,
    render_fields,
    resolve_model,
)
from blitzline.integrations.llm import post_json
from blitzline.records import PipelineError
from blitzline.storage import read_json, write_json


def invoke(url: str, action: str, params=None, transport=post_json):
    """Call AnkiConnect version 6 and require an explicit successful response."""
    response = transport(
        url, {"action": action, "version": 6, "params": params or {}}, {}, 30
    )
    if not isinstance(response, dict) or set(response) != {"result", "error"}:
        raise PipelineError("Malformed AnkiConnect response")
    if response["error"]:
        raise PipelineError(f"AnkiConnect {action}: {response['error']}")
    return response["result"]


def preflight(settings: dict, transport=post_json) -> None:
    """Verify API and model compatibility before creating any notes."""
    url, model = settings["anki_url"], settings["model"]
    version = invoke(url, "version", transport=transport)
    if not isinstance(version, int) or version < 6:
        raise PipelineError("AnkiConnect API version 6 or newer is required")
    models = invoke(url, "modelNames", transport=transport)
    if model not in models:
        return
    existing = invoke(url, "modelFieldNames", {"modelName": model}, transport)
    definition = resolve_model(settings)
    if existing != [f["name"] for f in definition["fields"]]:
        raise PipelineError(
            f"Anki model {model!r} has incompatible fields; select a new model name"
        )
    if settings["update_model"]:
        templates = invoke(url, "modelTemplates", {"modelName": model}, transport)
        if list(templates) != [t["name"] for t in definition["templates"]]:
            raise PipelineError(
                "Existing card templates differ; select a new model name"
            )


def ensure_model(settings: dict, transport=post_json) -> None:
    """Create the deck and shared note type only after compatibility preflight."""
    url = settings["anki_url"]
    invoke(url, "createDeck", {"deck": settings["deck"]}, transport)
    definition = resolve_model(settings)
    templates = [
        {"Name": t["name"], "Front": t["front"], "Back": t["back"]}
        for t in definition["templates"]
    ]
    if settings["model"] in invoke(url, "modelNames", transport=transport):
        if settings["update_model"]:
            invoke(
                url,
                "updateModelTemplates",
                {
                    "model": {
                        "name": settings["model"],
                        "templates": {
                            t["Name"]: {"Front": t["Front"], "Back": t["Back"]}
                            for t in templates
                        },
                    }
                },
                transport,
            )
            invoke(
                url,
                "updateModelStyling",
                {"model": {"name": settings["model"], "css": definition["css"]}},
                transport,
            )
        return
    invoke(
        url,
        "createModel",
        {
            "modelName": settings["model"],
            "inOrderFields": [f["name"] for f in definition["fields"]],
            "css": definition["css"],
            "isCloze": False,
            "cardTemplates": templates,
        },
        transport,
    )


def deliver(
    cards, media_dir: Path, destination: Path, settings: dict, transport=post_json
) -> list[Path]:
    """Deliver idempotently by stable note IDs and record partial successes."""
    definition = resolve_model(settings)
    paths = export_media(cards, media_dir, definition)
    preflight(settings, transport)
    ensure_model(settings, transport)
    destination.mkdir(parents=True, exist_ok=True)
    receipt = destination / "anki-delivery.json"
    previous = read_json(receipt) if receipt.is_file() else {}
    state = {
        "endpoint": settings["anki_url"],
        "deck": settings["deck"],
        "model": settings["model"],
        "language": settings.get("language", ""),
        "deck_profile": settings.get("deck_profile", ""),
        "notes": previous.get("notes", {}),
        "state": "running",
    }
    write_json(receipt, state)
    try:
        for path in paths:
            returned = invoke(
                settings["anki_url"],
                "storeMediaFile",
                {
                    "filename": path.name,
                    "data": base64.b64encode(path.read_bytes()).decode(),
                },
                transport,
            )
            if returned != path.name:
                raise PipelineError(f"Anki did not acknowledge media {path.name}")
        for card in cards:
            result = deliver_note(card, settings, transport, definition)
            state["notes"][card.id] = result
            write_json(receipt, state)
        state["state"] = "complete"
        write_json(receipt, state)
    except BaseException as error:
        state.update(state="failed", error=str(error))
        write_json(receipt, state)
        raise
    return [receipt]


def deliver_note(card, settings: dict, transport=post_json, definition=None) -> dict:
    """Find a note by exact identity before skipping, updating, or creating it."""
    url, model = settings["anki_url"], settings["model"]
    escaped_model = model.replace("\\", "\\\\").replace('"', '\\"')
    definition = definition or resolve_model(settings)
    identity = "blitzline_id_" + card.id
    query = f'"note:{escaped_model}" "tag:{identity}"'
    existing = invoke(url, "findNotes", {"query": query}, transport)
    if not existing and any(f["name"] == "BlitzlineId" for f in definition["fields"]):
        query = f'"note:{escaped_model}" "BlitzlineId:{card.id}"'
        existing = invoke(url, "findNotes", {"query": query}, transport)
    if len(existing) > 1:
        raise PipelineError(f"Multiple Anki notes have identity {card.id}")
    if existing and settings["existing"] == "skip":
        return {"note_id": existing[0], "action": "skipped"}
    if existing:
        invoke(
            url,
            "updateNoteFields",
            {
                "note": {
                    "id": existing[0],
                    "fields": render_fields(card, settings, definition),
                }
            },
            transport,
        )
        return {"note_id": existing[0], "action": "updated"}
    note = {
        "deckName": settings["deck"],
        "modelName": model,
        "fields": render_fields(card, settings, definition),
        "tags": note_tags(card, settings),
        "options": {"allowDuplicate": True},
    }
    identity = invoke(url, "addNote", {"note": note}, transport)
    if type(identity) is not int:
        raise PipelineError("Anki returned no note ID after creation")
    return {"note_id": identity, "action": "added"}
