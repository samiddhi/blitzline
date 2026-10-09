# One-command GitHub/PyPI releases

Run `make release` from the project root on `master`. It tests the checkout,
increments the patch version in both `pyproject.toml` and
`blitzline/__init__.py`, builds and checks distributions, commits all
non-ignored changes, and atomically pushes `master` and its version tag.
It then publishes a GitHub release. The `publish.yml` workflow tests and
builds the tagged source and uploads it to PyPI using Trusted Publishing.

No manual version change or local PyPI token is needed. Starting at `0.1.0`,
the first fresh release is `0.1.1`. If the current version's tag already
matches the checkout and there are no changes, the helper does nothing.
There are no language-pack assets in this repository.

## 1. Install local tools

```sh
cd ~/dev/blitzline
python3 -m venv .venv  # only if the project environment does not exist
brew install gh      # only if GitHub CLI is not installed
.venv/bin/python -m pip install -e '.[dev]'
```

## 2. Authenticate GitHub

```sh
gh auth login --hostname github.com --git-protocol https --web --scopes workflow
gh auth setup-git
gh auth status
```

Use the account with write access to `samiddhi/blitzline`. If already signed
in but missing workflow permission, run:

```sh
gh auth refresh --hostname github.com --scopes workflow
```

Keep your normal Git identity. Configure your own `user.name` and
GitHub-verified `user.email` if either is missing. The helper fetches
`origin/master` and tags and requires your checkout to include the remote
branch; resolve conflicts or synchronization problems before releasing.

## 3. Create the publishing environment

Open <https://github.com/samiddhi/blitzline/settings/environments> and create
an environment named **pypi**. Ensure GitHub Actions is enabled for the
repository. Alternatively, create the environment through GitHub CLI:

```sh
gh api --method PUT repos/samiddhi/blitzline/environments/pypi --input - <<<'{}'
```

## 4. Configure PyPI Trusted Publishing

If you already own the `blitzline` project on PyPI, open
<https://pypi.org/manage/project/blitzline/settings/publishing/> and add a
GitHub publisher. If it has not been created, open
<https://pypi.org/manage/account/publishing/> and add a pending GitHub publisher
with project name **blitzline**. A pending publisher creates the project on
its first successful upload; it does not reserve the package name.

| Field | Value |
| --- | --- |
| PyPI project name (pending publisher only) | `blitzline` |
| Owner | `samiddhi` |
| Repository name | `blitzline` |
| Workflow filename | `publish.yml` |
| Environment name | `pypi` |

The filename is `publish.yml`, replacing the old `python-publish.yml`
workflow. Update an existing publisher registration if it uses the old
filename. No PyPI token or password belongs in GitHub secrets.

See PyPI's instructions for [existing projects](https://docs.pypi.org/trusted-publishers/adding-a-publisher/)
and [new projects](https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/).

## 5. Review and publish

Preview the command and version bump without publishing:

```sh
make -n release
.venv/bin/python -m bumpversion bump --dry-run patch
git status
git diff
```

Then publish:

```sh
make release
```

The helper includes **all non-ignored changes**, including new files and
deletions, in its release commit. The workflow and helper can be included
in this first commit; a separate initial commit/push is unnecessary.
It tests before bumping and builds and runs `twine check` before pushing.
Publishing the GitHub release triggers PyPI publication; pushing a tag
alone does not. The workflow requires a `v` tag matching `project.version`.

## 6. Verify publication and retry failures

```sh
gh run list --repo samiddhi/blitzline --workflow publish.yml
gh run watch RUN_ID --repo samiddhi/blitzline --exit-status
```

Replace `RUN_ID` with the numeric ID of the new run. Local command success
means the GitHub release is published; check the asynchronous workflow for
PyPI success.

If the local helper stops, fix its error and rerun `make release`. The ignored
`maintenance/work/pending-release.json` checkpoint preserves the version.
Do not delete it or change the release commit while resuming. The GitHub
release is drafted before publication, and a retry reuses an existing draft.

If only the PyPI job failed, fix the account/workflow problem and rerun it:

```sh
gh run rerun RUN_ID --repo samiddhi/blitzline --failed
```

Do not create another release just to retry PyPI. The workflow skips already
uploaded files when recovering a partially completed upload.
