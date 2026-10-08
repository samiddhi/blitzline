"""Check cache recovery, locking, CLI behavior, and repository documentation.

Scope statement: test application boundaries and the requested code standards.
Included: atomic stage reuse, downstream invalidation, locks, help/exit codes,
and AST checks for scope headers and every function docstring.
Excluded: heavy ASR, paid APIs, and personal files.
Start here: test_stage_cache tests persistence; test_python_documentation audits
source and test modules for navigable headers and documented functions.
"""

import ast
from pathlib import Path

import pytest
from click.testing import CliRunner

from blitzline.cli import cli
from blitzline.records import PipelineError
from blitzline.storage import RunStore, read_json, run_lock


def test_stage_cache_and_invalidation(tmp_path):
    """Reuse checksum-valid stages and invalidate downstream work after input changes."""
    store = RunStore(tmp_path, {})
    calls = []

    def action():
        """Record stage execution and return one simple JSON artifact."""
        calls.append(1)
        return {"answer": 42}, []

    store.stage("01-first", "input", action)
    store.stage("02-next", "next", action)
    store.stage("01-first", "input", action)
    assert len(calls) == 2
    store.stage("01-first", "changed", action)
    assert "02-next" not in store.manifest["stages"]
    assert read_json(tmp_path / "artifacts/01-first.json") == {"answer": 42}


def test_run_lock(tmp_path):
    """Reject concurrent writers and release the lock after the owner exits."""
    with run_lock(tmp_path):
        with pytest.raises(PipelineError, match="Another process"):
            with run_lock(tmp_path):
                pass
    with run_lock(tmp_path):
        assert True


def test_failed_stage_receipt(tmp_path):
    """Keep failure state and avoid treating partial files as completed work."""
    store = RunStore(tmp_path, {})

    def failure():
        """Simulate an interrupted external stage."""
        raise PipelineError("external failure")

    with pytest.raises(PipelineError):
        store.stage("01-first", "input", failure)
    assert store.manifest["stages"]["01-first"]["state"] == "failed"


def test_cli_help_and_invalid_options():
    """Expose the four small commands and report invalid choices as usage errors."""
    runner = CliRunner()
    assert runner.invoke(cli, ["--help"]).exit_code == 0
    for name in ("run", "resume", "export", "check"):
        assert runner.invoke(cli, [name, "--help"]).exit_code == 0
    assert runner.invoke(cli, ["run", "--clips", "invalid"]).exit_code == 2


def test_python_documentation():
    """Require substantive scope headers and docstrings on all named functions."""
    root = Path(__file__).resolve().parents[1]
    missing = []
    for path in [*(root / "blitzline").rglob("*.py"), *(root / "tests").rglob("*.py")]:
        module = ast.parse(path.read_text())
        header = ast.get_docstring(module) or ""
        for label in ("Scope statement:", "Included:", "Excluded:", "Start here:"):
            if label not in header:
                missing.append(f"{path.name}: missing header {label}")
        for node in ast.walk(module):
            if isinstance(
                node, (ast.FunctionDef, ast.AsyncFunctionDef)
            ) and not ast.get_docstring(node):
                missing.append(f"{path.name}:{node.lineno} {node.name}")
    assert not missing, "\n".join(missing)
