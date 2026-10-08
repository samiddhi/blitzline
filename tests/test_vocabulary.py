"""Check exact context mapping and structured vocabulary decisions.

Scope statement: test pure vocabulary rules with small canonical transcripts.
Included: repetitions, cross-cue examples, ambiguous lemmas, Unicode highlights,
review completeness, candidate ownership, and stable note IDs.
Excluded: real Blitzer subprocesses (test_pipeline.py) and provider requests.
Start here: test_repeated_and_cross_cue_mapping checks the critical timing handoff.
"""

import pytest

from blitzline.exports.common import fields
from blitzline.vocabulary import (
    build_cards,
    candidate_groups,
    map_candidates,
    note_id,
    review_template,
    validate_decisions,
)


def rows():
    """Return minimal Blitzer JSON with one repeated and one cross-cue context."""
    return [
        {
            "term": "mačka",
            "count": 2,
            "contexts": [
                {"text": "Mačka spi.", "highlight_start": 0, "highlight_end": 5}
            ],
        },
        {
            "term": "teči",
            "count": 1,
            "contexts": [
                {"text": "Pes teče.", "highlight_start": 4, "highlight_end": 8}
            ],
        },
    ]


def test_repeated_and_cross_cue_mapping(cues):
    """Choose earliest exact repetition and cover every cue in a longer sentence."""
    candidates = map_candidates(rows(), cues)
    repeated = candidates[0].contexts[0]
    assert repeated.matches == (0, 11)
    assert repeated.cue_ids == ("cue-000001",)
    spanning = candidates[1].contexts[0]
    assert spanning.cue_ids == ("cue-000003", "cue-000004")
    assert (spanning.start_ms, spanning.end_ms) == (1800, 2800)


def test_false_match_not_guessed(cues):
    """Leave altered or substring-only contexts unresolved rather than fuzzy matching."""
    raw = [
        {
            "term": "mačka",
            "count": 1,
            "contexts": [
                {"text": "mačka spi.", "highlight_start": 0, "highlight_end": 5}
            ],
        }
    ]
    candidate = map_candidates(raw, cues)[0]
    assert candidate.contexts[0].error
    review = review_template([candidate])
    row = review["decisions"][0]
    row.update(decision="keep", english="cat", context_ids=[candidate.contexts[0].id])
    assert validate_decisions(review, [candidate])[1]


def test_review_coverage_and_context_ownership(cues):
    """Reject missing/duplicate decisions, invented IDs, and foreign example IDs."""
    candidates = map_candidates(rows(), cues)
    template = review_template(candidates)
    template["decisions"][0].update(
        decision="keep",
        english="cat",
        reason="",
        context_ids=[candidates[1].contexts[0].id],
    )
    valid, errors = validate_decisions(template, candidates)
    assert len(valid) == 1 and len(errors) == 1
    assert validate_decisions({"decisions": []}, candidates)[1]
    duplicated = {"decisions": template["decisions"] * 2}
    assert len(validate_decisions(duplicated, candidates)[1]) == 2


def test_card_fields_and_frequency(cues):
    """Build exact HTML examples locally and keep the original frequency integer."""
    candidates = map_candidates(rows(), cues)
    review = review_template(candidates)
    row = review["decisions"][0]
    row.update(
        decision="keep",
        english="cat <script>",
        reason="",
        notes="",
        context_ids=[candidates[0].contexts[0].id],
    )
    valid, errors = validate_decisions(review, candidates)
    assert not errors
    cards = build_cards(valid, candidates, "sourcehash", "slv", "recording")
    values = fields(cards[0])
    assert values["Sentence1"] == "<b>Mačka</b> spi."
    assert values["Sentence2"] == ""
    assert values["English"] == "cat &lt;script&gt;"
    assert values["Frequency"] == "2"


def test_competing_candidates_grouped(cues):
    """Keep candidates for the same highlighted word together during batching."""
    raw = rows()
    raw.append({**raw[0], "term": "other"})
    groups = candidate_groups(map_candidates(raw, cues))
    assert sorted(map(len, groups)) == [1, 2]


def test_note_identity_unicode():
    """Normalize equivalent Unicode lemmas without involving generated translations."""
    assert note_id("a", "slv", "č") == note_id("a", "slv", "c\u030c")
    assert note_id("a", "slv", "word") != note_id("b", "slv", "word")


@pytest.mark.parametrize("count", [True, 0, -1, "2"])
def test_invalid_counts(count, cues):
    """Reject noninteger and nonpositive Blitzer frequencies."""
    raw = rows()
    raw[0]["count"] = count
    with pytest.raises(ValueError):
        map_candidates(raw, cues)
