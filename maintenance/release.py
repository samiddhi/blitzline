"""Publish Blitzline through a resumable local release and GitHub Actions.

Scope statement: repository maintenance for one-command releases.
Included: patch bumps, tests, builds, commits, tags and GitHub publication.
Excluded: runtime pipeline execution, PyPI credentials and remote job execution.
Start here: main prepares or resumes a release; finish_release publishes it.
"""

import importlib.util
import json
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / "maintenance" / "work"
PENDING = WORK / "pending-release.json"
REPOSITORY = "samiddhi/blitzline"


def run(*arguments, capture=False, check=True):
    """Run an argument list in the checkout without a shell."""
    result = subprocess.run(
        list(arguments),
        cwd=ROOT,
        text=True,
        capture_output=capture,
        check=check,
    )
    return result.stdout.strip() if capture and check else result


def write_json(path, value):
    """Atomically write a JSON release checkpoint."""
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")
    temporary.replace(path)


def project_version():
    """Read the application version from project metadata."""
    config = tomllib.loads((ROOT / "pyproject.toml").read_text())
    return config["project"]["version"]


def next_patch(version):
    """Increment only the final component of a three-part version."""
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("Release versions must have the form 0.1.0")
    major, minor, patch = version.split(".")
    return f"{major}.{minor}.{int(patch) + 1}"


def source_changed(version):
    """Compare source with its tag, including uncommitted files."""
    if run("git", "status", "--porcelain", capture=True):
        return True
    tag = run(
        "git",
        "rev-parse",
        "--verify",
        f"refs/tags/v{version}",
        capture=True,
        check=False,
    )
    if tag.returncode:
        return True
    result = run("git", "diff", "--quiet", f"v{version}", "HEAD", check=False)
    if result.returncode not in (0, 1):
        result.check_returncode()
    return result.returncode == 1


def preflight():
    """Check branch, tools, identity and remote before preparation."""
    if run("git", "branch", "--show-current", capture=True) != "master":
        raise ValueError("Run releases from master")
    if not shutil.which("gh"):
        raise ValueError("Install GitHub CLI first: brew install gh")
    run("gh", "auth", "status")
    run("git", "fetch", "origin", "master", "--tags")
    run("git", "merge-base", "--is-ancestor", "origin/master", "HEAD")
    if run("git", "diff", "--name-only", "--diff-filter=U", capture=True):
        raise ValueError("Resolve merge conflicts before releasing")
    for key in ("user.name", "user.email"):
        run("git", "config", "--get", key, capture=True)
    if importlib.util.find_spec("bumpversion") is None:
        raise ValueError("Install tools: .venv/bin/python -m pip install -e '.[dev]'")


def prepare_release():
    """Test changed source and save a checkpoint before changing the version."""
    old_version = project_version()
    if not source_changed(old_version):
        print("No source changes; nothing to release.")
        return None
    run(sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider")
    state = {
        "version": next_patch(old_version),
        "previous_version": old_version,
        "head": None,
    }
    write_json(PENDING, state)
    return state


def commit_release(state):
    """Bump once, check distributions and commit non-ignored changes."""
    if project_version() == state["previous_version"]:
        run(
            sys.executable,
            "-m",
            "bumpversion",
            "bump",
            "patch",
        )
    if project_version() != state["version"]:
        raise ValueError("Project version no longer matches pending release")
    output = WORK / f"app-v{state['version']}"
    if output.exists():
        shutil.rmtree(output)
    run(sys.executable, "-m", "build", "--outdir", str(output))
    distributions = [
        str(path)
        for path in sorted(output.iterdir())
        if path.is_file() and path.name.endswith((".whl", ".tar.gz"))
    ]
    if not distributions:
        raise ValueError(f"Build produced no distributions in {output}")
    run(
        sys.executable,
        "-m",
        "twine",
        "check",
        *distributions,
    )
    run("git", "add", "-A")
    staged = run("git", "diff", "--cached", "--quiet", check=False)
    if staged.returncode not in (0, 1):
        staged.check_returncode()
    if staged.returncode == 1:
        run("git", "commit", "-m", f"Release {state['version']}")
    state["head"] = run("git", "rev-parse", "HEAD", capture=True)
    write_json(PENDING, state)


def push_release(state, tag):
    """Create or verify the tag and push it atomically with master."""
    if run("git", "rev-parse", "HEAD", capture=True) != state["head"]:
        raise ValueError("HEAD changed; restore the pending release commit")
    existing = run(
        "git",
        "rev-parse",
        "--verify",
        f"{tag}^{{commit}}",
        capture=True,
        check=False,
    )
    if existing.returncode and existing.returncode != 128:
        existing.check_returncode()
    if existing.returncode:
        run("git", "tag", "-a", tag, "-m", f"Release {state['version']}")
    if not existing.returncode and existing.stdout.strip() != state["head"]:
        raise ValueError(f"{tag} already names a different commit")
    run("git", "push", "--atomic", "origin", "master", f"refs/tags/{tag}")


def publish_github(state, tag):
    """Create a draft and publish it to trigger the PyPI workflow."""
    view = run(
        "gh",
        "release",
        "view",
        tag,
        "--repo",
        REPOSITORY,
        "--json",
        "isDraft",
        capture=True,
        check=False,
    )
    if view.returncode:
        run(
            "gh",
            "release",
            "create",
            tag,
            "--repo",
            REPOSITORY,
            "--draft",
            "--verify-tag",
            "--title",
            f"blitzline {state['version']}",
            "--notes",
            "Application release.",
        )
    elif not json.loads(view.stdout)["isDraft"]:
        return
    run(
        "gh",
        "release",
        "edit",
        tag,
        "--repo",
        REPOSITORY,
        "--draft=false",
        "--latest",
    )


def finish_release(state):
    """Resume versioning, Git and GitHub publication in order."""
    if state["head"] is None:
        commit_release(state)
    tag = f"v{state['version']}"
    push_release(state, tag)
    publish_github(state, tag)
    PENDING.unlink()
    print(f"{tag} published on GitHub. PyPI publication runs in Actions.")
    print("Follow it with: gh run list --workflow publish.yml")


def main():
    """Release this checkout, keeping checkpoints on failure."""
    try:
        preflight()
        state = (
            json.loads(PENDING.read_text()) if PENDING.exists() else prepare_release()
        )
        if state is not None:
            finish_release(state)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"Release stopped: {error}", file=sys.stderr)
        if PENDING.exists():
            print(
                "Run make release again to resume this version.",
                file=sys.stderr,
            )
        raise SystemExit(1) from error


if __name__ == "__main__":
    main()
