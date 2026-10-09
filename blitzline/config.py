"""Load explicit settings without executing the pipeline.

Scope statement: own configuration defaults, selection, and validation.
Included: TOML loading, path resolution, stage-specific LLM profiles,
provider endpoints, environment-variable names, and CLI overrides.
Excluded: resolving secret values (integrations/llm.py), creating files
(storage.py), transcription, and terminal presentation (cli.py).
Start here: get_config returns a new validated settings dictionary;
llm_profile selects a shared or stage-specific provider profile.
"""

import os
import re
import tomllib
from copy import deepcopy
from pathlib import Path
from urllib.parse import urlparse

from platformdirs import user_config_dir, user_data_dir

from blitzline.exports.model import resolve_model
from blitzline.records import integer, text

PROVIDERS = {
    "openrouter": ("https://openrouter.ai/api/v1", "OPENROUTER_API_KEY"),
    "deepseek": ("https://api.deepseek.com", "DEEPSEEK_API_KEY"),
    "compatible": ("", "LLM_API_KEY"),
}


def defaults() -> dict:
    """Return independent defaults without reading credentials or making files."""
    return {
        "run": {
            "output_dir": str(Path(user_data_dir("blitzline")) / "runs"),
            "proofread": False,
            "translate": False,
            "allow_partial": False,
        },
        "asr": {
            "backend": "local",
            "executable": "whisperx",
            "model": "small",
            "device": "cpu",
            "compute_type": "int8",
            "language": "",
            "timeout": 7200,
            "max_cue_ms": 8000,
            "line_length": 42,
            "pause_ms": 800,
            "max_chars_per_second": 25,
            "relisten": False,
            "endpoint": "",
            "api_key_env": "",
            "modal_app": "blitzline-asr",
            "modal_function": "transcribe",
            "modal_environment": "",
            "remote_compute_type": "float16",
            "chunk_seconds": 600,
            "retries": 2,
            "options": {},
        },
        "blitzer": {
            "executable": "bltzr",
            "config": "",
            "plugins_dir": "",
            "known_file": "",
            "skip_exact_words_file": "",
            "skip_word_families_file": "",
            "no_config": False,
            "timeout": 120,
        },
        "llm": {
            "default": "",
            "proofread": "",
            "translate": "",
            "review": "",
            "batch_size": 20,
            "max_chars": 24000,
            "repair_attempts": 2,
        },
        "profiles": {},
        "decks": {},
        "media": {
            "kind": "audio",
            "padding_ms": 200,
            "max_clip_ms": 45000,
            "ffmpeg": "ffmpeg",
            "ffprobe": "ffprobe",
            "timeout": 300,
            "audio_codec": "libmp3lame",
            "video_codec": "libx264",
        },
        "export": {
            "format": "tsv",
            "deck": "Blitzline",
            "model": "Blitzer Basic",
            "anki_url": "http://127.0.0.1:8765",
            "existing": "skip",
            "preset": "blitzer",
            "deck_profile": "",
            "card_types": [],
            "fields": [],
            "templates": [],
            "front_template": None,
            "back_template": None,
            "css": None,
            "css_file": "",
            "assets": [],
            "tags": ["blitzline"],
            "two_way": False,
            "update_model": False,
        },
    }


def resolve_path(value: str, base: Path) -> str:
    """Expand a configured path relative to the configuration file directory."""
    path = Path(os.path.expandvars(value)).expanduser()
    return str((base / path).resolve() if not path.is_absolute() else path.resolve())


def config_path(explicit=None) -> Path | None:
    """Select explicit, environment, or platform TOML without creating it."""
    selected = explicit or os.environ.get("BLITZLINE_CONFIG")
    if selected:
        path = Path(selected).expanduser().resolve()
        if not path.is_file():
            raise ValueError(f"Configuration does not exist: {path}")
        return path
    root = os.environ.get("XDG_CONFIG_HOME")
    directory = Path(root) / "blitzline" if root else Path(user_config_dir("blitzline"))
    path = directory / "config.toml"
    return path if path.is_file() else None


def merge_config(raw: dict, base: Path) -> dict:
    """Merge recognized TOML tables and resolve file paths, rejecting typos."""
    result = defaults()
    for section, values in raw.items():
        if section not in result or not isinstance(values, dict):
            raise ValueError(f"Unknown or invalid configuration table: {section}")
        if section in {"profiles", "decks"}:
            result[section] = deepcopy(values)
            continue
        unknown = values.keys() - result[section].keys()
        if unknown:
            raise ValueError(f"Unknown {section} setting: {', '.join(sorted(unknown))}")
        result[section].update(deepcopy(values))
    if (
        raw.get("export", {}).get("model") == "Blitzline v1"
        and "preset" not in raw["export"]
    ):
        result["export"]["preset"] = "legacy"
    for section, name in (
        ("run", "output_dir"),
        ("blitzer", "config"),
        ("blitzer", "plugins_dir"),
        ("blitzer", "known_file"),
        ("blitzer", "skip_exact_words_file"),
        ("blitzer", "skip_word_families_file"),
    ):
        value = text(result[section][name], f"{section}.{name}", empty=True)
        if value:
            result[section][name] = resolve_path(text(value, f"{section}.{name}"), base)
    for section, name in (
        ("asr", "executable"),
        ("blitzer", "executable"),
        ("media", "ffmpeg"),
        ("media", "ffprobe"),
    ):
        value = text(result[section][name], f"{section}.{name}")
        if "/" in value or "\\" in value:
            result[section][name] = resolve_path(value, base)
    resolve_export_paths(result["export"], base)
    for profile in result["decks"].values():
        if not isinstance(profile, dict):
            raise ValueError("Deck profiles must be tables")
        resolve_export_paths(profile, base)
    return result


def resolve_export_paths(export: dict, base: Path) -> None:
    """Resolve template, stylesheet, and asset paths in base or deck-specific options."""
    if export.get("css_file"):
        export["css_file"] = resolve_path(text(export["css_file"], "css_file"), base)
    if not isinstance(export.get("assets", []), list) or not isinstance(
        export.get("templates", []), list
    ):
        raise ValueError("export.assets and templates must be arrays")
    if "assets" in export:
        export["assets"] = [
            resolve_path(text(p, "export asset"), base) for p in export["assets"]
        ]
    for template in export.get("templates", []):
        if not isinstance(template, dict):
            raise ValueError("Invalid export.templates entry")
        for side in ("front_file", "back_file"):
            if template.get(side):
                template[side] = resolve_path(text(template[side], side), base)


def deck_export(settings: dict, name: str) -> dict:
    """Apply a named deck's export overrides while isolating its note type identity."""
    profile = settings["decks"][name]
    result = deepcopy(settings["export"])
    result.update({k: deepcopy(v) for k, v in profile.items() if k != "language"})
    result["deck_profile"] = name
    result["language"] = profile["language"]
    if "two_way" not in profile and "Recall" in profile["card_types"]:
        result["two_way"] = True
    if "model" not in profile:
        result["model"] += " [" + name + "]"
    return result


def resolve_export(settings: dict, language: str | None) -> dict:
    """Select an explicitly named deck or the single deck matching the run language."""
    selected = settings["export"]["deck_profile"]
    profiles = settings["decks"]
    if selected:
        if selected not in profiles:
            raise ValueError(f"Unknown deck profile: {selected}")
        if language is not None and profiles[selected]["language"] != language:
            raise ValueError(
                f"Deck profile {selected!r} is for {profiles[selected]['language']}, not {language}"
            )
        return deck_export(settings, selected)
    if not profiles:
        return {**deepcopy(settings["export"]), "language": language or ""}
    matches = [
        name for name, profile in profiles.items() if profile["language"] == language
    ]
    if not matches:
        raise ValueError(
            f"No deck profile configured for {language}; configure its language and card_types"
        )
    if len(matches) > 1:
        raise ValueError(
            f"Multiple deck profiles configured for {language}; select --deck-profile"
        )
    return deck_export(settings, matches[0])


def resolve_language(settings: dict, language: str | None) -> str:
    """Infer a run's language only from an explicitly selected deck profile."""
    if language:
        resolve_export(settings, language)
        return language
    selected = settings["export"]["deck_profile"]
    if not selected:
        raise ValueError("Specify --language or select a configured --deck-profile")
    return settings["decks"][selected]["language"]


def upgrade_settings(settings: dict) -> dict:
    """Fill newly added settings when reading a saved run, preserving legacy notes."""
    saved = deepcopy(settings)
    if "preset" not in saved.get("export", {}):
        saved["export"]["preset"] = "legacy"
    return merge_config(saved, Path.cwd())


def get_config(path=None, overrides=None) -> dict:
    """Return defaults plus selected TOML and explicit non-None CLI overrides."""
    selected = config_path(path)
    raw = tomllib.loads(selected.read_text()) if selected else {}
    result = merge_config(raw, selected.parent if selected else Path.cwd())
    for section, values in (overrides or {}).items():
        result[section].update({k: v for k, v in values.items() if v is not None})
    validate_config(result)
    return result


def validate_config(settings: dict) -> None:
    """Reject invalid settings before a stage spends time or makes requests."""
    expected = defaults()
    if settings.keys() != expected.keys():
        raise ValueError("Configuration must contain all supported tables")
    for section, values in settings.items():
        if section not in expected or not isinstance(values, dict):
            raise ValueError(f"Invalid configuration section: {section}")
        if (
            section not in {"profiles", "decks"}
            and values.keys() != expected[section].keys()
        ):
            raise ValueError(f"Unknown settings in {section}")
    for section, names in {
        "run": ("proofread", "translate", "allow_partial"),
        "asr": ("relisten",),
        "blitzer": ("no_config",),
        "export": ("two_way", "update_model"),
    }.items():
        for name in names:
            if type(settings[section][name]) is not bool:
                raise ValueError(f"{section}.{name} must be true or false")
    choices = (
        ("media", "kind", {"audio", "video"}),
        ("asr", "backend", {"local", "modal", "http"}),
        ("export", "format", {"tsv", "apkg", "anki"}),
        ("export", "existing", {"skip", "update"}),
    )
    for section, name, allowed in choices:
        if settings[section][name] not in allowed:
            raise ValueError(f"{section}.{name} must be one of {sorted(allowed)}")
    for section, names in {
        "asr": (
            "timeout",
            "max_cue_ms",
            "line_length",
            "pause_ms",
            "max_chars_per_second",
            "chunk_seconds",
        ),
        "blitzer": ("timeout",),
        "llm": ("batch_size", "max_chars"),
        "media": ("timeout", "max_clip_ms"),
    }.items():
        for name in names:
            integer(settings[section][name], f"{section}.{name}", 1)
    integer(settings["media"]["padding_ms"], "media.padding_ms")
    integer(settings["llm"]["repair_attempts"], "llm.repair_attempts")
    for name, profile in settings["profiles"].items():
        validate_profile(name, profile)
    for stage in ("default", "proofread", "translate", "review"):
        name = settings["llm"][stage]
        if name and name not in settings["profiles"]:
            raise ValueError(f"llm.{stage} references missing profile {name!r}")
    if settings["blitzer"]["no_config"] and settings["blitzer"]["config"]:
        raise ValueError("blitzer.config and no_config are mutually exclusive")
    blitzer = settings["blitzer"]
    for name in ("known_file", "skip_exact_words_file", "skip_word_families_file"):
        text(blitzer[name], f"blitzer.{name}", empty=True)
    if blitzer["known_file"] and (
        blitzer["skip_exact_words_file"] or blitzer["skip_word_families_file"]
    ):
        raise ValueError(
            "Use blitzer.skip_exact_words_file and blitzer.skip_word_families_file "
            "without the legacy blitzer.known_file setting"
        )
    text(settings["export"]["deck"], "export.deck")
    text(settings["export"]["model"], "export.model")
    resolve_model(settings["export"])
    selected_deck = text(
        settings["export"]["deck_profile"], "export.deck_profile", empty=True
    )
    if selected_deck and selected_deck not in settings["decks"]:
        raise ValueError(f"Unknown deck profile: {selected_deck}")
    assigned_languages = {}
    shared_models = {}
    allowed = (expected["export"].keys() - {"format", "deck_profile"}) | {"language"}
    for name, profile in settings["decks"].items():
        text(name, "deck profile name")
        if not isinstance(profile, dict) or profile.keys() - allowed:
            raise ValueError(f"Invalid deck profile {name!r}")
        language = text(profile.get("language"), f"Deck {name} language")
        if not re.fullmatch(r"[a-z]{3}", language):
            raise ValueError(
                f"Deck {name}: language must be a three-letter Blitzer code"
            )
        deck = text(profile.get("deck"), f"Deck {name} deck")
        if deck in assigned_languages and assigned_languages[deck] != language:
            raise ValueError(f"Deck {deck!r} is assigned to multiple languages")
        assigned_languages[deck] = language
        if not isinstance(profile.get("card_types"), list) or not profile["card_types"]:
            raise ValueError(
                f"Deck {name}: explicitly configure a nonempty card_types array"
            )
        options = deck_export(settings, name)
        text(options["model"], f"Deck {name} model")
        for flag in ("two_way", "update_model"):
            if type(options[flag]) is not bool:
                raise ValueError(f"Deck {name} {flag} must be true or false")
        if options["existing"] not in {"skip", "update"}:
            raise ValueError(f"Deck {name}: existing must be skip or update")
        validate_url(options["anki_url"])
        definition = resolve_model(options)
        signature = {
            "fields": [f["name"] for f in definition["fields"]],
            "templates": definition["templates"],
            "css": definition["css"],
        }
        if (
            options["model"] in shared_models
            and shared_models[options["model"]] != signature
        ):
            raise ValueError(
                f"Deck {name}: shared model {options['model']!r} has conflicting definitions; use distinct model names"
            )
        shared_models[options["model"]] = signature
    asr = settings["asr"]
    integer(asr["retries"], "asr.retries")
    if not isinstance(asr["options"], dict):
        raise ValueError("asr.options must be a table")
    if asr["api_key_env"] and not re.fullmatch(
        r"[A-Za-z_][A-Za-z0-9_]*", asr["api_key_env"]
    ):
        raise ValueError("asr.api_key_env must name an environment variable")
    if asr["backend"] == "http":
        validate_url(asr["endpoint"])
    if asr["backend"] == "modal":
        for name in ("modal_app", "modal_function"):
            text(asr[name], "asr." + name)
    text(asr["remote_compute_type"], "asr.remote_compute_type")
    text(asr["modal_environment"], "asr.modal_environment", empty=True)
    if asr["chunk_seconds"] > 3600:
        raise ValueError("asr.chunk_seconds must be <= 3600")
    for section, names in {
        "asr": ("executable", "model", "device", "compute_type"),
        "blitzer": ("executable",),
        "media": ("ffmpeg", "ffprobe", "audio_codec", "video_codec"),
        "run": ("output_dir",),
    }.items():
        for name in names:
            text(settings[section][name], f"{section}.{name}")
    validate_url(settings["export"]["anki_url"])


def validate_url(value: str) -> None:
    """Require an HTTP(S) URL without embedded credentials or query secrets."""
    parsed = urlparse(text(value, "endpoint URL"))
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        raise ValueError("Endpoint must be an absolute HTTP(S) URL")
    if parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise ValueError(
            "Endpoint URLs must not contain credentials, queries, or fragments"
        )


def validate_profile(name: str, profile: dict) -> None:
    """Check named LLM profiles and prohibit storing literal credentials."""
    allowed = {
        "provider",
        "model",
        "base_url",
        "api_key_env",
        "response_format",
        "temperature",
        "max_tokens",
        "timeout",
        "retries",
        "referer",
    }
    if not isinstance(profile, dict) or profile.keys() - allowed:
        raise ValueError(
            f"Invalid profile {name!r}; use supported fields and api_key_env"
        )
    if profile.get("provider") not in PROVIDERS:
        raise ValueError(f"Profile {name}: provider must be {sorted(PROVIDERS)}")
    text(profile.get("model"), f"Profile {name} model")
    endpoint, env = PROVIDERS[profile["provider"]]
    validate_url(profile.get("base_url", endpoint))
    if profile.get("referer"):
        validate_url(profile["referer"])
    if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", profile.get("api_key_env", env)):
        raise ValueError(
            f"Profile {name}: api_key_env must name an environment variable"
        )
    if profile.get("response_format", "json_object") not in {
        "json_object",
        "json_schema",
        "text",
    }:
        raise ValueError(f"Profile {name}: invalid response_format")
    for key, fallback, minimum in (
        ("timeout", 120, 1),
        ("retries", 2, 0),
        ("max_tokens", 4096, 1),
    ):
        integer(profile.get(key, fallback), f"Profile {name} {key}", minimum)
    temperature = profile.get("temperature", 0)
    if type(temperature) not in (int, float) or not 0 <= temperature <= 2:
        raise ValueError(f"Profile {name}: temperature must be between 0 and 2")


def llm_profile(settings: dict, stage: str) -> dict | None:
    """Resolve a stage profile with provider defaults but no secret value."""
    name = settings["llm"][stage] or settings["llm"]["default"]
    if not name:
        return None
    profile = deepcopy(settings["profiles"][name])
    endpoint, env = PROVIDERS[profile["provider"]]
    return {
        "base_url": endpoint,
        "api_key_env": env,
        "response_format": "json_object",
        "temperature": 0,
        "max_tokens": 4096,
        "timeout": 120,
        "retries": 2,
        **profile,
    }
