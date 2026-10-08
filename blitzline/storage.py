"""Persist recoverable run artifacts and verify cache validity.

Scope statement: own local storage, hashes, and exclusive run locks.
Included: atomic UTF-8/JSON writes, file fingerprints, stage checksums,
manifest lifecycle, request caches, and failure records.
Excluded: stage policy (core.py), transcript transformations (transcript.py),
provider requests (integrations/), and export formatting (exports/).
Start here: RunStore.stage runs or reuses one explicitly named stage.
"""

import fcntl
import hashlib
import json
import os
import tempfile
from contextlib import contextmanager
from datetime import datetime, timezone
from pathlib import Path

from blitzline import __version__
from blitzline.records import SCHEMA_VERSION, PipelineError


def canonical(value) -> str:
    """Serialize JSON deterministically for hashing and request cache keys."""
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
        allow_nan=False,
    )


def digest(value) -> str:
    """Return the SHA-256 of a JSON-serializable value."""
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def fingerprint(path) -> str:
    """Hash file contents incrementally; return a marker for absent files."""
    path = Path(path)
    if not path.is_file():
        return "missing"
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def atomic_text(path: Path, value: str) -> None:
    """Replace a UTF-8 file atomically, removing unfinished temporary files."""
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(dir=path.parent, prefix=".writing-")
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            stream.write(value)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def write_json(path: Path, value) -> None:
    """Write readable JSON atomically with non-ASCII text preserved."""
    atomic_text(
        path, json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
    )


def read_json(path: Path):
    """Read UTF-8 JSON with a path-specific diagnostic on malformed content."""
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise PipelineError(f"Cannot read JSON {path}: {error}") from error


@contextmanager
def run_lock(directory: Path):
    """Hold a nonblocking OS lock released automatically even after a crash."""
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / ".lock").open("a+") as stream:
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise PipelineError(f"Another process is using run {directory}") from error
        try:
            yield
        finally:
            fcntl.flock(stream, fcntl.LOCK_UN)


class RunStore:
    """Own one run's manifest, stage artifacts, and verified cache state."""

    def __init__(self, directory: Path, metadata: dict, progress=None):
        """Open or create a run manifest; require its supported schema version."""
        self.directory = directory
        self.progress = progress or (lambda message: None)
        self.path = directory / "manifest.json"
        self.manifest = (
            read_json(self.path)
            if self.path.exists()
            else {
                "schema_version": SCHEMA_VERSION,
                "stages": {},
                "created": datetime.now(timezone.utc).isoformat(),
            }
        )
        if self.manifest.get("schema_version") != SCHEMA_VERSION:
            raise PipelineError("Unsupported run schema version")
        self.manifest.update(metadata)
        for name in ("artifacts", "media", "exports", "logs"):
            (directory / name).mkdir(parents=True, exist_ok=True)
        self.save()

    def save(self) -> None:
        """Persist the current manifest atomically."""
        write_json(self.path, self.manifest)

    def event(self, stage: str, state: str) -> None:
        """Append a small stage event without storing input contents or credentials."""
        value = {
            "time": datetime.now(timezone.utc).isoformat(),
            "stage": stage,
            "state": state,
        }
        with (self.directory / "logs" / "events.jsonl").open(
            "a", encoding="utf-8"
        ) as stream:
            stream.write(canonical(value) + "\n")

    def artifact(self, name: str) -> Path:
        """Return a stage artifact path under this run's artifacts directory."""
        return self.directory / "artifacts" / name

    def stage(self, name: str, inputs, action):
        """Reuse checksum-valid stage JSON or execute and commit its explicit action."""
        key = digest(
            {"schema": SCHEMA_VERSION, "version": __version__, "inputs": inputs}
        )
        path = self.artifact(name + ".json")
        previous = self.manifest["stages"].get(name, {})
        outputs = previous.get("outputs", {})
        valid = all(fingerprint(self.directory / p) == h for p, h in outputs.items())
        if (
            previous.get("key") == key
            and previous.get("state") in {"complete", "skipped"}
            and valid
            and outputs
        ):
            self.progress(f"Reusing {name}")
            self.event(name, "reused")
            return read_json(path)
        self.progress(f"Running {name}")
        if name[:2] < "07":
            stale = [stage for stage in self.manifest["stages"] if stage >= name]
            for stage in stale:
                del self.manifest["stages"][stage]
        self.manifest["stages"][name] = {"key": key, "state": "running"}
        self.save()
        self.event(name, "running")
        try:
            value, extra_paths = action()
            write_json(path, value)
            paths = [path, *extra_paths]
            hashes = {str(p.relative_to(self.directory)): fingerprint(p) for p in paths}
            state = (
                "skipped"
                if isinstance(value, dict) and value.get("skipped")
                else "complete"
            )
            self.manifest["stages"][name].update(state=state, outputs=hashes)
            self.save()
            self.event(name, state)
            return value
        except BaseException as error:
            self.manifest["stages"][name].update(state="failed", error=str(error))
            self.save()
            self.event(name, "failed")
            raise

    def needs_review(self, message: str) -> None:
        """Mark a run awaiting review without invalidating its completed stages."""
        self.manifest.update(state="needs-review", message=message)
        self.save()
