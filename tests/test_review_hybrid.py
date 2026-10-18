"""Check focused second-pass review of competing lemma readings.

Scope statement: test review routing and corrected example selection.
Included: same-spelling rivals, restored candidates, and unambiguous bypass.
Excluded: live provider calls and linguistic claims beyond supplied fixtures.
Start here: test_second_pass_repairs_contexts_and_rejected_rival.
"""

import sqlite3

import pytest

from blitzline.records import Candidate, Context
from blitzline.review import (
    THINKING_INSTRUCTION,
    VOCABULARY_INSTRUCTION,
    ambiguous_review_groups,
    batches,
    review_vocabulary,
)
from blitzline.vocabulary import candidate_groups


def context(identity, sentence, form):
    """Build a timed example with one highlighted wordform."""
    start = sentence.index(form)
    return Context(identity, sentence, start, start + len(form), ("cue-1",), 0, 1000)


def decision(candidate, kind, examples):
    """Return one schema-valid vocabulary decision for the fake provider."""
    return {
        "candidate_id": candidate.id,
        "decision": kind,
        "reason": "supported" if kind == "keep" else "false match",
        "english": "inevitable" if kind == "keep" else "",
        "context_ids": examples,
        "notes": "",
    }


def test_second_pass_repairs_contexts_and_rejected_rival(settings, tmp_path):
    """Reconsider a rejected lemma and exclude a wrong-part-of-speech example."""
    adjective = Candidate(
        "adjective",
        "neizogiben",
        2,
        (
            context("adj-1", "neizogiben del", "neizogiben"),
            context("adj-2", "neizogibno povzroči", "neizogibno"),
        ),
    )
    adverb = Candidate(
        "adverb",
        "neizogibno",
        2,
        (
            context("adv-1", "neizogibno povzroči", "neizogibno"),
            context("adv-2", "neizogibno trpljenje", "neizogibno"),
        ),
    )
    plain = Candidate(
        "plain",
        "razpon",
        1,
        (context("plain-1", "razpon mnenj", "razpon"),),
    )
    settings["profiles"] = {
        "fast": {"provider": "openrouter", "model": "test", "reasoning_enabled": False},
        "thinking": {
            "provider": "openrouter",
            "model": "test",
            "reasoning_enabled": True,
        },
    }
    settings["llm"].update(
        review="fast",
        ambiguity_review="thinking",
        batch_size=20,
        ambiguity_batch_size=5,
    )
    calls = []

    def requester(profile, instruction, data, schema, cache):
        """Return deliberately flawed fast decisions and corrected thinking ones."""
        calls.append((profile["reasoning_enabled"], instruction, data))
        if profile["reasoning_enabled"]:
            assert data["first_pass_decisions"]
            return {
                "decisions": [
                    decision(adjective, "keep", ["adj-1"]),
                    decision(adverb, "keep", ["adv-1"]),
                ]
            }
        return {
            "decisions": [
                decision(adjective, "reject", []),
                decision(adverb, "keep", ["adv-1", "adv-2"]),
                decision(plain, "keep", ["plain-1"]),
            ]
        }

    result = review_vocabulary(
        [adjective, adverb, plain], "slv", settings, tmp_path, requester
    )
    assert not result["errors"]
    assert [
        (row["candidate_id"], row["context_ids"]) for row in result["decisions"]
    ] == [
        ("adjective", ["adj-1"]),
        ("adverb", ["adv-1"]),
        ("plain", ["plain-1"]),
    ]
    assert len(calls) == 2
    assert calls[0][1] == VOCABULARY_INSTRUCTION
    assert calls[1][1] == THINKING_INSTRUCTION
    assert set(calls[1][2]["requested_ids"]) == {"adjective", "adverb"}
    assert len(ambiguous_review_groups([adjective, adverb, plain])) == 1


def test_no_competing_candidate_skips_second_pass(settings, tmp_path):
    """A single candidate alone does not trigger the candidate-based heuristic."""
    candidate = Candidate(
        "adverb",
        "neizogibno",
        1,
        (context("adv-1", "neizogibno povzroči", "neizogibno"),),
    )
    settings["profiles"] = {
        "fast": {"provider": "openrouter", "model": "test"},
        "thinking": {"provider": "openrouter", "model": "test"},
    }
    settings["llm"].update(review="fast", ambiguity_review="thinking")
    calls = []

    def requester(profile, instruction, data, schema, cache):
        """Return one simple decision while recording which pass was invoked."""
        calls.append(instruction)
        return {"decisions": [decision(candidate, "keep", ["adv-1"])]}

    review_vocabulary([candidate], "slv", settings, tmp_path, requester)
    assert calls == [VOCABULARY_INSTRUCTION]


def test_same_form_in_different_sentences_stays_in_one_thinking_group():
    """Link rivals by spelling even when they never share a source occurrence."""
    adjective = Candidate(
        "adjective",
        "neizogiben",
        1,
        (context("adj-1", "neizogibno trpljenje", "neizogibno"),),
    )
    adverb = Candidate(
        "adverb",
        "neizogibno",
        1,
        (context("adv-1", "neizogibno povzroči", "neizogibno"),),
    )
    groups = ambiguous_review_groups([adjective, adverb])
    assert [[candidate.id for candidate in group] for group in groups] == [
        ["adjective", "adverb"]
    ]


def test_shared_forms_crossing_batch_boundary_stay_together(settings, tmp_path):
    """Use dictionary inflections to keep rivals together across a full batch."""
    ordinary = [
        Candidate(
            f"plain-{index}",
            f"word-{index}",
            1,
            (context(f"plain-{index}-1", f"word-{index} stoji", f"word-{index}"),),
        )
        for index in range(19)
    ]
    adjective = Candidate(
        "adjective",
        "moder",
        1,
        (context("adj-1", "moder človek", "moder"),),
    )
    adverb = Candidate(
        "adverb",
        "modro",
        1,
        (context("adv-1", "modro govori", "modro"),),
    )
    candidates = [*ordinary, adjective, adverb]
    lexicon = tmp_path / "lemmas.db"
    with sqlite3.connect(lexicon) as db:
        db.executescript(
            "CREATE TABLE lemmas (id INTEGER PRIMARY KEY, lemma TEXT);"
            "CREATE TABLE forms (lemma_id INTEGER, form_representation TEXT);"
            "INSERT INTO lemmas VALUES (1, 'moder'), (2, 'modro');"
            "INSERT INTO forms VALUES (1, 'moder'), (1, 'modro'), (2, 'modro');"
        )
    settings["profiles"] = {"fast": {"provider": "openrouter", "model": "test"}}
    settings["llm"].update(
        review="fast",
        ambiguity_review="",
        batch_size=20,
        form_lexicon=str(lexicon),
    )
    calls = []

    def requester(profile, instruction, data, schema, cache):
        """Record fast-review batches and return valid fixture decisions."""
        calls.append(data["requested_ids"])
        return {
            "decisions": [
                decision(candidate, "keep", [candidate.contexts[0].id])
                for candidate in candidates
                if candidate.id in data["requested_ids"]
            ]
        }

    result = review_vocabulary(candidates, "slv", settings, tmp_path, requester)
    assert not result["errors"]
    assert list(map(len, calls)) == [19, 2]
    assert calls[1] == ["adjective", "adverb"]


def test_shared_form_groups_are_transitive():
    """Candidates linked through different forms remain inseparable."""
    first = Candidate("first", "a", 1, (context("a-1", "a x", "a"),))
    bridge = Candidate(
        "bridge",
        "b",
        2,
        (context("b-1", "a y", "a"), context("b-2", "b y", "b")),
    )
    last = Candidate("last", "c", 1, (context("c-1", "b z", "b"),))
    assert [
        [c.id for c in group] for group in candidate_groups([first, bridge, last])
    ] == [["first", "bridge", "last"]]


def test_oversized_shared_form_group_is_not_split():
    """A hard 20-item cap fails explicitly if 21 rivals share one form."""
    group = [{"id": f"candidate-{index}"} for index in range(21)]
    with pytest.raises(ValueError, match="increase the batch size"):
        batches([group], 20, 24000)
