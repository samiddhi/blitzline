"""Extract vocabulary through the installed Blitzer command.

Scope statement: own the subprocess contract and its external input hashes.
Included: mandatory flags, safe JSON extraction, CLI version checks,
and config/skip-list/pack fingerprints for invalidating cached extraction.
Excluded: importing Blitzer internals, changing its user data, building packs,
context mapping (vocabulary.py), and pipeline stage order (core.py).
Start here: extract runs the CLI; dependencies describes its read-only inputs.
"""

import json
import os
import shutil
import subprocess
import tempfile
import tomllib
from pathlib import Path

from platformdirs import user_config_dir, user_data_dir

from blitzline.config import resolve_path
from blitzline.media import run_process
from blitzline.records import PipelineError
from blitzline.storage import fingerprint


def arguments(language: str, path: Path, settings: dict) -> list[str]:
    """Build the invariant automation flags with optional explicit Blitzer paths."""
    args = [
        settings["executable"],
        "blitz",
        "--language",
        language,
        "--file",
        str(path),
        "--freq",
        "--lemmatize",
        "--context",
        "--bold",
        "html",
        "--format",
        "json",
        "--no-prompt",
        "--no-src",
        "--no-update-known",
        "--no-save-context",
    ]
    if settings["no_config"]:
        args.append("--no-config")
    for key, flag in (
        ("config", "--config"),
        ("plugins_dir", "--plugins-dir"),
        ("known_file", "--known-file"),
        ("skip_exact_words_file", "--skip-exact-words-file"),
        ("skip_word_families_file", "--skip-word-families-file"),
    ):
        if settings.get(key):
            args.extend([flag, settings[key]])
    return args


def extract(language: str, path: Path, settings: dict, runner=subprocess.run):
    """Return parsed Blitzer JSON, keeping diagnostic output out of the data."""
    raw = run_process(arguments(language, path, settings), settings["timeout"], runner)
    try:
        return json.loads(raw)
    except ValueError as error:
        raise PipelineError(
            "Blitzer did not return valid JSON; check CLI compatibility"
        ) from error


def preflight(language: str, settings: dict, runner=subprocess.run) -> None:
    """Check the language/config with token-free input and all mutation flags disabled."""
    with tempfile.TemporaryDirectory(prefix="blitzline-check-") as directory:
        path = Path(directory) / "empty-vocabulary.txt"
        path.write_text(".", encoding="utf-8")
        result = extract(language, path, settings, runner)
        if not isinstance(result, list):
            raise PipelineError("Blitzer JSON contract changed; expected an array")


def selected_config(settings: dict) -> Path | None:
    """Mirror documented Blitzer config selection without reading its Python API."""
    if settings["no_config"]:
        return None
    explicit = settings["config"] or os.environ.get("BLITZER_CONFIG")
    if explicit:
        return Path(resolve_path(explicit, Path.cwd()))
    root = os.environ.get("XDG_CONFIG_HOME")
    native = Path(root) / "bltzr" if root else Path(user_config_dir("bltzr"))
    home = Path.home() / ".config" / "bltzr" / "bltzr.toml"
    paths = [native / "bltzr.toml", home] if root else [home, native / "bltzr.toml"]
    return next((p for p in paths if p.is_file()), None)


def filtering_paths(options: dict, settings: dict, base: Path) -> list[Path]:
    """Track only the lists read by Blitzer after explicit file overrides."""
    skip_keys = ("skip_exact_words_file", "skip_word_families_file")
    modern = any(settings.get(key) or key in options for key in skip_keys)
    if modern:
        values = [settings.get(key) or options.get(key) for key in skip_keys]
    else:
        values = [settings.get("known_file") or options.get("known_file")]
        values.extend(options.get("exclusions", []))
        values.extend(options.get("forms_only", []))
    return [Path(resolve_path(value, base)) for value in values if value]


def dependencies(language: str, settings: dict, runner=subprocess.run) -> dict:
    """Fingerprint selected config, filtering inputs, pack files, and executable."""
    executable = shutil.which(settings["executable"]) or settings["executable"]
    version = run_process(
        [executable, "--version"], settings["timeout"], runner
    ).strip()
    path = selected_config(settings)
    raw = tomllib.loads(path.read_text()) if path and path.is_file() else {}
    base = path.parent if path else Path.cwd()
    data_root = os.environ.get("XDG_DATA_HOME")
    data = Path(data_root) / "bltzr" if data_root else Path(user_data_dir("bltzr"))
    pack = (
        settings["plugins_dir"]
        or raw.get("locations", {}).get("plugins_dir")
        or str(data / "languages")
    )
    pack = Path(resolve_path(pack, base)) / language
    paths = [Path(executable)]
    if path:
        paths.append(path)
    options = {**raw.get("defaults", {}), **raw.get("languages", {}).get(language, {})}
    paths.extend(filtering_paths(options, settings, base))
    if options.get("sort") == "custom" and options.get("custom_order"):
        paths.append(Path(resolve_path(options["custom_order"], base)))
    if pack.is_dir():
        paths.extend(p for p in pack.rglob("*") if p.is_file())
    # Bundled English is tied to the reported CLI version when no user pack exists.
    return {
        "version": version,
        "pack": str(pack),
        "files": {str(p): fingerprint(p) for p in paths},
        "settings": settings,
    }
