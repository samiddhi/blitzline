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

from blitzline.exports.common import BACK, CSS, FRONT
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
        },
        "blitzer": {
            "executable": "bltzr",
            "config": "",
            "plugins_dir": "",
            "known_file": "",
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
            "model": "Blitzline v1",
            "anki_url": "http://127.0.0.1:8765",
            "existing": "skip",
            "front_template": FRONT,
            "back_template": BACK,
            "css": CSS,
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
        if section == "profiles":
            result[section] = deepcopy(values)
            continue
        unknown = values.keys() - result[section].keys()
        if unknown:
            raise ValueError(f"Unknown {section} setting: {', '.join(sorted(unknown))}")
        result[section].update(values)
    for section, name in (
        ("run", "output_dir"),
        ("blitzer", "config"),
        ("blitzer", "plugins_dir"),
        ("blitzer", "known_file"),
    ):
        value = result[section][name]
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
    return result


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
        if section != "profiles" and values.keys() != expected[section].keys():
            raise ValueError(f"Unknown settings in {section}")
    for section, names in {
        "run": ("proofread", "translate", "allow_partial"),
        "asr": ("relisten",),
        "blitzer": ("no_config",),
    }.items():
        for name in names:
            if type(settings[section][name]) is not bool:
                raise ValueError(f"{section}.{name} must be true or false")
    choices = (
        ("media", "kind", {"audio", "video"}),
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
    text(settings["export"]["deck"], "export.deck")
    text(settings["export"]["model"], "export.model")
    text(settings["export"]["front_template"], "export.front_template")
    text(settings["export"]["back_template"], "export.back_template")
    text(settings["export"]["css"], "export.css", empty=True)
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
