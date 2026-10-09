"""Supply small isolated records and media for pipeline tests.

Scope statement: own reusable test data and fake responses.
Included: fresh configuration, timed cues, reviewed decisions, and generated
short recordings. All persistent resources live in pytest temporary paths.
Excluded: real credentials, personal media, paid API calls, and production logic.
Start here: settings and cues are basic fixtures; approve_request is a fake LLM.
"""

import shutil
import sqlite3
import subprocess

import pytest

from blitzline.config import defaults
from blitzline.records import Cue


@pytest.fixture
def settings(tmp_path):
    """Return independent defaults with local output and isolated Blitzer settings."""
    value = defaults()
    value["run"]["output_dir"] = str(tmp_path / "runs")
    value["blitzer"]["no_config"] = True
    pack = tmp_path / "packs" / "eng"
    pack.mkdir(parents=True)
    (pack / "config.toml").write_text(
        """format_version = 1
[metadata]
language_name = "Test English"
language_code = "eng"
version = "1"
author = "Tests"
[normalization]
lowercase = true
substitutions = []
"""
    )
    with sqlite3.connect(pack / "lemmas.db") as database:
        database.executescript(
            """CREATE TABLE lemmas (id INTEGER PRIMARY KEY, lemma TEXT NOT NULL UNIQUE);
CREATE TABLE forms (id INTEGER PRIMARY KEY, form_representation TEXT NOT NULL,
lemma_id INTEGER NOT NULL REFERENCES lemmas(id), UNIQUE(form_representation, lemma_id));
CREATE INDEX idx_forms_repr ON forms(form_representation);
CREATE TABLE frequencies (term TEXT PRIMARY KEY, frequency REAL NOT NULL CHECK(frequency >= 0));
PRAGMA user_version = 1;"""
        )
        for index, (lemma, forms) in enumerate(
            (
                ("cat", ("cat", "cats")),
                ("dog", ("dog",)),
                ("run", ("run", "runs")),
                ("bird", ("bird",)),
                ("fly", ("flies",)),
                ("be", ("be", "am", "is", "are", "was", "were", "being", "been")),
            ),
            1,
        ):
            database.execute("INSERT INTO lemmas VALUES (?, ?)", (index, lemma))
            database.executemany(
                "INSERT INTO forms(form_representation, lemma_id) VALUES (?, ?)",
                [(form, index) for form in forms],
            )
    value["blitzer"]["plugins_dir"] = str(pack.parent)
    return value


@pytest.fixture
def cues():
    """Return repeated sentences and one sentence crossing a cue boundary."""
    return [
        Cue("cue-000001", 100, 800, "Mačka spi."),
        Cue("cue-000002", 900, 1700, "Mačka spi."),
        Cue("cue-000003", 1800, 2200, "Pes"),
        Cue("cue-000004", 2300, 2800, "teče."),
    ]


@pytest.fixture
def recording(tmp_path):
    """Generate four seconds of small video/audio with the real FFmpeg when present."""
    if not shutil.which("ffmpeg") or not shutil.which("ffprobe"):
        pytest.skip("FFmpeg/FFprobe required for media integration tests")
    path = tmp_path / "recording.mp4"
    subprocess.run(
        [
            "ffmpeg",
            "-nostdin",
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=160x120:r=10:d=4",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=4",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
            str(path),
        ],
        check=True,
    )
    return path


def approve_request(profile, instruction, data, schema, cache):
    """Return valid deterministic review/translation/proofreading responses."""
    if "candidates" in data:
        requested = set(data["requested_ids"])
        return {
            "decisions": [
                {
                    "candidate_id": row["id"],
                    "decision": "keep",
                    "reason": "",
                    "english": "test meaning",
                    "context_ids": [row["contexts"][0]["id"]],
                    "notes": "",
                }
                for row in data["candidates"]
                if row["id"] in requested
            ]
        }
    proofread = "uncertain" in schema["properties"]["cues"]["items"]["properties"]
    result = []
    for row in data["requested"]:
        extra = {"uncertain": False, "reason": ""} if proofread else {}
        result.append(
            {
                "cue_id": row["cue_id"],
                "text": row["text"] if proofread else "English example.",
                **extra,
            }
        )
    return {"cues": result}


def configure_llm(settings):
    """Configure a fakeable OpenRouter profile without requiring a real secret."""
    settings["profiles"]["router"] = {"provider": "openrouter", "model": "vendor/model"}
    settings["llm"]["default"] = "router"
    return settings
