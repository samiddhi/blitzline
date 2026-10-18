"""Validate vocabulary review and link exact examples to source intervals.

Scope statement: own pure candidate mapping and card construction.
Included: the Blitzer JSON contract, exact context matching, ambiguity-aware
batching, review schemas, decision checks, and stable note identities.
Excluded: running Blitzer or an LLM (integrations/), cutting media (media.py),
file writes (storage.py), and HTML/Anki rendering (exports/common.py).
Start here: map_candidates links Blitzer output; validate_decisions checks
manual or API review; build_cards assembles deterministic learner records.
"""

import unicodedata

from blitzline.records import Candidate, Card, Context, integer, text
from blitzline.storage import digest
from blitzline.transcript import plaintext, word_spans

REVIEW_FIELDS = {
    "candidate_id",
    "decision",
    "reason",
    "english",
    "context_ids",
    "notes",
}


def object_schema(properties: dict) -> dict:
    """Build a closed JSON object schema requiring all listed properties."""
    return {
        "type": "object",
        "properties": properties,
        "required": list(properties),
        "additionalProperties": False,
    }


def review_schema() -> dict:
    """Return the provider-independent schema for vocabulary decisions."""
    item = object_schema(
        {
            "candidate_id": {"type": "string"},
            "decision": {"type": "string", "enum": ["keep", "reject", "needs-review"]},
            "reason": {"type": "string"},
            "english": {"type": "string"},
            "context_ids": {
                "type": "array",
                "items": {"type": "string"},
                "maxItems": 2,
            },
            "notes": {"type": "string"},
        }
    )
    return object_schema({"decisions": {"type": "array", "items": item}})


def all_matches(source: str, context: str) -> tuple[int, ...]:
    """Find exact sentence substrings at text boundaries, including repetitions."""
    matches = []
    cursor = 0
    while True:
        start = source.find(context, cursor)
        if start < 0:
            return tuple(matches)
        end = start + len(context)
        left_ok = start == 0 or not (
            source[start - 1].isalnum() and context[0].isalnum()
        )
        right_ok = end == len(source) or not (
            source[end].isalnum() and context[-1].isalnum()
        )
        if left_ok and right_ok:
            matches.append(start)
        cursor = start + 1


def context_interval(
    cues, spans, left: int, right: int
) -> tuple[tuple[str, ...], int, int]:
    """Recover intersecting cues and trustworthy word timing for an exact span."""
    by_id = {cue.id: cue for cue in cues}
    selected = [span for span in spans if span.start < right and span.end > left]
    intervals = []
    for span in selected:
        cue = by_id[span.cue_id]
        positions = word_spans(" ".join(cue.text.split()), cue.words)
        words = [
            word
            for word, (start, end) in zip(cue.words, positions)
            if span.start + start < right and span.start + end > left
        ]
        intervals.append(
            (words[0].start_ms, words[-1].end_ms)
            if words
            else (cue.start_ms, cue.end_ms)
        )
    return (
        tuple(span.cue_id for span in selected),
        min(start for start, _ in intervals),
        max(end for _, end in intervals),
    )


def map_context(
    row: dict, candidate_id: str, index: int, cues, source, spans
) -> Context:
    """Validate one highlight and map its earliest exact source-text occurrence."""
    if not isinstance(row, dict):
        raise ValueError("Blitzer context must be an object")
    value = text(row.get("text"), "Blitzer context text")
    start = integer(row.get("highlight_start"), "highlight_start")
    end = integer(row.get("highlight_end"), "highlight_end", 1)
    if not start < end <= len(value):
        raise ValueError("Blitzer highlight is outside its context")
    matches = all_matches(source, value)
    identity = f"{candidate_id}-context-{index}"
    if not matches:
        return Context(
            identity, value, start, end, (), 0, 0, (), "No exact transcript match"
        )
    ids, first, last = context_interval(
        cues, spans, matches[0], matches[0] + len(value)
    )
    return Context(identity, value, start, end, ids, first, last, matches)


def map_candidates(rows, cues) -> list[Candidate]:
    """Validate Blitzer JSON and attach timing without changing counts or text."""
    if not isinstance(rows, list):
        raise ValueError("Blitzer JSON must be an array")
    source, spans = plaintext(cues)
    candidates, seen = [], set()
    for row in rows:
        if not isinstance(row, dict) or not isinstance(row.get("contexts"), list):
            raise ValueError("Blitzer entries require term, count, and contexts")
        term = text(row.get("term"), "Blitzer term")
        normalized = unicodedata.normalize("NFC", term)
        if normalized in seen:
            raise ValueError(f"Duplicate Blitzer term: {term}")
        seen.add(normalized)
        count = integer(row.get("count"), "Blitzer count", 1)
        identity = "candidate-" + digest(term)[:16]
        contexts = tuple(
            map_context(context, identity, index, cues, source, spans)
            for index, context in enumerate(row["contexts"], 1)
        )
        candidates.append(Candidate(identity, term, count, contexts))
    return candidates


def candidate_groups(candidates, lemma_forms=None) -> list[list[Candidate]]:
    """Keep candidates with any shared observed or supplied form in one group."""
    candidates = list(candidates)
    parent = list(range(len(candidates)))

    def root(index):
        """Find a candidate's shared-form component with path compression."""
        while parent[index] != index:
            parent[index] = parent[parent[index]]
            index = parent[index]
        return index

    first_by_form = {}
    for index, candidate in enumerate(candidates):
        forms = {highlighted_form(context) for context in candidate.contexts}
        forms.update(lemma_forms.get(candidate.term, ()) if lemma_forms else ())
        for raw_form in forms:
            form = unicodedata.normalize("NFC", raw_form).casefold()
            if form in first_by_form:
                parent[root(index)] = root(first_by_form[form])
            else:
                first_by_form[form] = index
    groups = {}
    for index, candidate in enumerate(candidates):
        groups.setdefault(root(index), []).append(candidate)
    return list(groups.values())


def highlighted_form(context: Context) -> str:
    """Normalize the wordform selected by Blitzer for cross-sentence grouping."""
    return unicodedata.normalize(
        "NFC", context.text[context.highlight_start : context.highlight_end]
    ).casefold()


def review_template(candidates) -> dict:
    """Build an editable review file with every candidate explicitly unresolved."""
    return {
        "decisions": [
            {
                "candidate_id": c.id,
                "decision": "needs-review",
                "reason": "Awaiting review",
                "english": "",
                "context_ids": [],
                "notes": "",
            }
            for c in candidates
        ]
    }


def validate_decision(row, candidate: Candidate) -> dict:
    """Validate one decision, including field types and context ownership."""
    if not isinstance(row, dict) or row.keys() != REVIEW_FIELDS:
        raise ValueError("Decision fields must be " + ", ".join(sorted(REVIEW_FIELDS)))
    for name in ("candidate_id", "decision", "reason", "english", "notes"):
        text(row[name], name, empty=name in {"reason", "english", "notes"})
    decision = row["decision"]
    if decision not in {"keep", "reject", "needs-review"}:
        raise ValueError("decision must be keep, reject, or needs-review")
    ids = row["context_ids"]
    if not isinstance(ids, list) or any(not isinstance(i, str) for i in ids):
        raise ValueError("context_ids must be an array of strings")
    available = {c.id: c for c in candidate.contexts}
    if (
        len(ids) > 2
        or len(set(ids)) != len(ids)
        or any(i not in available for i in ids)
    ):
        raise ValueError("Select up to two distinct contexts owned by this candidate")
    if decision != "keep":
        text(row["reason"], "Rejected/unresolved reason")
        return row
    text(row["english"], "English meaning")
    if not ids:
        raise ValueError("Kept candidates need at least one context")
    if any(available[i].error for i in ids):
        raise ValueError("Selected context has unresolved timing")
    return row


def validate_decisions(data, candidates) -> tuple[list[dict], list[dict]]:
    """Return valid decisions and item-specific errors; never drop missing IDs."""
    if (
        not isinstance(data, dict)
        or set(data) != {"decisions"}
        or not isinstance(data["decisions"], list)
    ):
        return [], [
            {
                "candidate_id": c.id,
                "error": "Response must contain only a decisions array",
            }
            for c in candidates
        ]
    expected = {c.id: c for c in candidates}
    buckets = {}
    errors = []
    for row in data["decisions"]:
        identity = row.get("candidate_id") if isinstance(row, dict) else None
        if not isinstance(identity, str) or identity not in expected:
            errors.append(
                {"candidate_id": "", "error": "Unknown/malformed candidate ID"}
            )
            continue
        buckets.setdefault(identity, []).append(row)
    valid = []
    for identity, candidate in expected.items():
        rows = buckets.get(identity, [])
        if len(rows) != 1:
            errors.append(
                {"candidate_id": identity, "error": "Expected exactly one decision"}
            )
            continue
        try:
            valid.append(validate_decision(rows[0], candidate))
        except ValueError as error:
            errors.append({"candidate_id": identity, "error": str(error)})
    return valid, errors


def note_id(source_hash: str, language: str, term: str) -> str:
    """Derive a stable note ID from source contents, language, and NFC lemma."""
    return (
        "blitzline-"
        + digest([source_hash, language, unicodedata.normalize("NFC", term)])[:32]
    )


def build_cards(
    decisions, candidates, source_hash: str, language: str, source: str
) -> list[Card]:
    """Assemble only kept decisions from canonical fields and owned contexts."""
    by_id = {c.id: c for c in candidates}
    cards = []
    for decision in decisions:
        if decision["decision"] != "keep":
            continue
        candidate = by_id[decision["candidate_id"]]
        contexts = {c.id: c for c in candidate.contexts}
        cards.append(
            Card(
                note_id(source_hash, language, candidate.term),
                candidate.term,
                decision["english"],
                decision["notes"],
                candidate.source_frequency,
                tuple(contexts[i] for i in decision["context_ids"]),
                source,
            )
        )
    return cards
