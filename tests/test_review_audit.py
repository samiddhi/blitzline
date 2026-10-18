"""Check one-request correction audits after vocabulary review.

Scope statement: verify sparse corrections and their safety boundary.
Included: full candidate coverage, corrected examples, and invalid patch rejection.
Excluded: provider transport, paid requests, and broad linguistic evaluation.
Start here: test_one_audit_call_applies_sparse_correction.
"""

import pytest

from blitzline.integrations.llm import RequestFailure
from blitzline.records import Candidate, Context
from blitzline.review import AUDIT_INSTRUCTION, audit_vocabulary, review_vocabulary


def candidate(identity, term, sentence):
    """Make a candidate whose entire one-word sentence is highlighted."""
    return Candidate(
        identity,
        term,
        1,
        (Context(identity + "-context", sentence, 0, len(sentence), (), 0, 1000),),
    )


def decision(item, kind):
    """Build one schema-valid first-pass or corrected decision."""
    return {
        "candidate_id": item.id,
        "decision": kind,
        "reason": "supported" if kind == "keep" else "wrong reading",
        "english": "painfully" if kind == "keep" else "",
        "context_ids": [item.contexts[0].id] if kind == "keep" else [],
        "notes": "",
    }


def test_one_audit_call_applies_sparse_correction(settings, tmp_path):
    """Send all candidates once and replace only explicitly corrected decisions."""
    wrong = candidate("wrong", "boleče", "boleče")
    right = candidate("right", "miza", "miza")
    settings["profiles"] = {
        "fast": {"provider": "openrouter", "model": "test"},
        "thinking": {"provider": "openrouter", "model": "test"},
    }
    settings["llm"].update(review="fast", audit_review="thinking")
    calls = []

    def requester(profile, instruction, data, schema, cache):
        """Return first-pass decisions followed by one sparse audit patch."""
        calls.append((instruction, data))
        if instruction == AUDIT_INSTRUCTION:
            assert data["candidate_count"] == 2
            assert len(data["candidates"]) == 2
            assert data["candidates"][0]["current"]["decision"] == "reject"
            return {
                "corrections": [
                    {
                        "index": 0,
                        "decision": "keep",
                        "reason": "adverb in this context",
                        "english": "painfully",
                        "context_indices": [0],
                        "notes": "",
                    }
                ]
            }
        return {"decisions": [decision(wrong, "reject"), decision(right, "keep")]}

    result = review_vocabulary([wrong, right], "slv", settings, tmp_path, requester)
    assert len(calls) == 2
    assert [row["decision"] for row in result["decisions"]] == ["keep", "keep"]
    assert result["audit_corrections"] == [result["decisions"][0]]
    assert result["decisions"][1] == decision(right, "keep")


@pytest.mark.parametrize(
    "patch",
    [
        {
            "index": 1,
            "decision": "keep",
            "reason": "x",
            "english": "x",
            "context_indices": [0],
            "notes": "",
        },
        {
            "index": 0,
            "decision": "keep",
            "reason": "x",
            "english": "x",
            "context_indices": [2],
            "notes": "",
        },
    ],
)
def test_audit_rejects_unknown_candidate_or_example(tmp_path, patch):
    """Never apply a correction to a foreign candidate or example."""
    item = candidate("word", "boleče", "boleče")

    def requester(profile, instruction, data, schema, cache):
        """Supply an invalid sparse correction for semantic validation."""
        return {"corrections": [patch]}

    with pytest.raises(RequestFailure, match="invalid|unknown"):
        audit_vocabulary(
            [item], [decision(item, "reject")], "slv", {}, tmp_path, requester
        )
