"""Remove completed run intermediates only after an explicit user action.

Scope statement: plan and apply cleanup of a portable exported run.
Included: export verification, safe preview, removal of known process directories,
and a small cleaned-run receipt.
Excluded: automatic post-export cleanup, deleting source media, and Anki imports.
Start here: cleanup_plan validates a run; clean_run applies the same plan under lock.
"""

import shutil
from datetime import datetime, timezone
from pathlib import Path

from blitzline.records import SCHEMA_VERSION, PipelineError
from blitzline.storage import fingerprint, read_json, run_lock, write_json

PROCESS_DIRS = ("artifacts", "media", "logs", ".cache")


def cleanup_plan(directory: Path) -> dict:
    """Describe removable intermediates after verifying a portable export."""
    directory = Path(directory).expanduser().resolve()
    manifest_path = directory / "manifest.json"
    if not manifest_path.is_file():
        raise PipelineError(f"No run manifest in {directory}")
    if manifest_path.is_symlink() or (directory / "exports").is_symlink():
        raise PipelineError(
            "Refusing to clean a run with symlinked manifest or exports"
        )
    manifest = read_json(manifest_path)
    if manifest.get("state") == "cleaned":
        raise PipelineError("This run was already cleaned")
    if manifest.get("state") != "complete":
        raise PipelineError("Finish the run before cleaning its intermediates")
    stages = manifest.get("stages", {})
    kinds = [
        kind
        for kind in ("apkg", "tsv")
        if stages.get("07-export-" + kind, {}).get("state") == "complete"
    ]
    if not kinds:
        raise PipelineError("Cleanup requires a completed portable APKG or TSV export")
    exports = []
    for kind in kinds:
        kind_exports = []
        for name, checksum in stages["07-export-" + kind].get("outputs", {}).items():
            relative = Path(name)
            if relative.is_absolute() or ".." in relative.parts:
                raise PipelineError(f"Invalid export path in manifest: {name}")
            path = directory / relative
            if path.is_relative_to(directory / "exports"):
                if (
                    path.is_symlink()
                    or not path.resolve().is_relative_to(directory / "exports")
                    or fingerprint(path) != checksum
                ):
                    raise PipelineError(f"Export changed or is missing: {path}")
                kind_exports.append(name)
        if not kind_exports or (
            kind == "apkg" and not any(p.endswith(".apkg") for p in kind_exports)
        ):
            raise PipelineError(f"No verified {kind} export file was found")
        exports.extend(kind_exports)
    targets = []
    files = 0
    bytes_total = 0
    for name in PROCESS_DIRS:
        path = directory / name
        if path.is_symlink():
            raise PipelineError(f"Refusing to clean symlinked directory: {path}")
        if not path.exists():
            continue
        if not path.is_dir():
            raise PipelineError(f"Expected a directory: {path}")
        targets.append(name)
        for item in path.rglob("*"):
            if item.is_file() and not item.is_symlink():
                files += 1
                bytes_total += item.stat().st_size
    return {
        "run": str(directory),
        "remove": targets,
        "files": files,
        "bytes": bytes_total,
        "keep": exports,
        "formats": kinds,
        "created": manifest.get("created"),
    }


def clean_run(directory: Path) -> dict:
    """Remove planned process directories and leave a nonresumable receipt."""
    directory = Path(directory).expanduser().resolve()
    with run_lock(directory):
        plan = cleanup_plan(directory)
        for name in plan["remove"]:
            shutil.rmtree(directory / name)
        write_json(
            directory / "manifest.json",
            {
                "schema_version": SCHEMA_VERSION,
                "state": "cleaned",
                "created": plan["created"],
                "cleaned": datetime.now(timezone.utc).isoformat(),
                "export_formats": plan["formats"],
                "exports": plan["keep"],
            },
        )
        return plan
