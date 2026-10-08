"""Check configuration and provider protocols without network access.

Scope statement: verify API options, secret handling, and bounded repair/retries.
Included: OpenRouter headers/routing, DeepSeek defaults, custom endpoints,
TOML precedence, cache reuse, truncation rejection, and structured validation.
Excluded: paid model calls and linguistic quality judgments.
Start here: test_openrouter_request inspects the actual adapter's HTTP boundary.
"""

import json

import pytest
from conftest import configure_llm
from test_vocabulary import rows

from blitzline.config import get_config, llm_profile, validate_config
from blitzline.integrations.llm import (
    RequestFailure,
    decode_response,
    request_json,
    request_payload,
)
from blitzline.review import repair_vocabulary, validate_cue_response
from blitzline.vocabulary import map_candidates, review_schema


def test_config_precedence_and_paths(tmp_path, monkeypatch):
    """Resolve TOML-relative paths and give explicit overrides highest priority."""
    path = tmp_path / "settings.toml"
    path.write_text('[media]\nkind="video"\n[blitzer]\nknown_file="known.txt"\n')
    monkeypatch.setenv("BLITZLINE_CONFIG", str(path))
    settings = get_config(overrides={"media": {"kind": "audio"}})
    assert settings["media"]["kind"] == "audio"
    assert settings["blitzer"]["known_file"] == str(tmp_path / "known.txt")
    path.write_text('[media]\nkins="video"')
    with pytest.raises(ValueError, match="Unknown"):
        get_config(path)


def test_openrouter_request(settings, tmp_path, monkeypatch):
    """Use OpenRouter's endpoint, bearer key, app title, and JSON parameter routing."""
    configure_llm(settings)
    settings["profiles"]["router"]["response_format"] = "json_schema"
    profile = llm_profile(settings, "review")
    monkeypatch.setenv("OPENROUTER_API_KEY", "secret-test-value")
    calls = []

    def transport(url, payload, headers, timeout):
        """Capture a request and return a valid chat completion."""
        calls.append((url, payload, headers))
        return {
            "choices": [
                {"finish_reason": "stop", "message": {"content": '{"decisions": []}'}}
            ]
        }

    assert request_json(
        profile, "Review", [], review_schema(), tmp_path, transport
    ) == {"decisions": []}
    url, payload, headers = calls[0]
    assert url == "https://openrouter.ai/api/v1/chat/completions"
    assert headers["Authorization"] == "Bearer secret-test-value"
    assert headers["X-OpenRouter-Title"] == "Blitzline"
    assert payload["provider"] == {"require_parameters": True}
    assert payload["response_format"]["json_schema"]["strict"] is True
    monkeypatch.delenv("OPENROUTER_API_KEY")
    request_json(profile, "Review", [], review_schema(), tmp_path, transport)
    assert len(calls) == 1
    assert all(
        "secret-test-value" not in path.read_text() for path in tmp_path.glob("*.json")
    )


def test_deepseek_and_compatible(settings):
    """Support named DeepSeek and arbitrary compatible servers independently per stage."""
    settings["profiles"] = {
        "ds": {"provider": "deepseek", "model": "chosen-model"},
        "local": {
            "provider": "compatible",
            "model": "local-model",
            "base_url": "http://localhost:1234/v1",
        },
    }
    settings["llm"].update(default="ds", review="local")
    validate_config(settings)
    assert llm_profile(settings, "translate")["base_url"] == "https://api.deepseek.com"
    assert llm_profile(settings, "review")["base_url"].endswith("/v1")
    profile = llm_profile(settings, "translate")
    assert "provider" not in request_payload(profile, "Review", {}, review_schema())


def test_retry_and_truncated_response(settings, tmp_path, monkeypatch):
    """Retry rate-limit failures with a bound and reject incomplete model JSON."""
    configure_llm(settings)
    profile = llm_profile(settings, "review")
    monkeypatch.setenv("OPENROUTER_API_KEY", "test")
    attempts = []

    def transport(url, payload, headers, timeout):
        """Simulate repeated rate limits for retry-count verification."""
        attempts.append(1)
        raise RequestFailure("HTTP 429", True)

    waits = []
    with pytest.raises(RequestFailure):
        request_json(
            profile, "Review", [], review_schema(), tmp_path, transport, waits.append
        )
    assert len(attempts) == 3 and waits == [1, 2]
    with pytest.raises(RequestFailure):
        decode_response(
            {"choices": [{"finish_reason": "length", "message": {"content": "{}"}}]}
        )


def test_item_specific_repair(settings, tmp_path, cues):
    """Repair only missing decisions while retaining valid items and competing evidence."""
    configure_llm(settings)
    candidates = map_candidates(rows(), cues)
    chunks = [
        json.loads(json.dumps(__import__("dataclasses").asdict(c))) for c in candidates
    ]
    requested = []

    def requester(profile, instruction, data, schema, cache):
        """Omit one item initially, then provide only the requested missing item."""
        ids = data["requested_ids"]
        requested.append(ids)
        selected = ids[:1]
        return {
            "decisions": [
                {
                    "candidate_id": i,
                    "decision": "reject",
                    "reason": "proper name",
                    "english": "",
                    "context_ids": [],
                    "notes": "",
                }
                for i in selected
            ]
        }

    valid, errors = repair_vocabulary(
        chunks,
        candidates,
        "slv",
        llm_profile(settings, "review"),
        settings,
        tmp_path,
        requester,
    )
    assert not errors and len(valid) == 2
    assert requested[1] == [candidates[1].id]


def test_cue_validation():
    """Reject merged, duplicate, or empty translated cues."""
    expected = {"a": {}, "b": {}}
    assert validate_cue_response(
        {"cues": [{"cue_id": "a", "text": "One"}]}, expected, False
    )[1]
    assert validate_cue_response(
        {"cues": [{"cue_id": "a", "text": ""}, {"cue_id": "b", "text": "Two"}]},
        expected,
        False,
    )[1]


@pytest.mark.parametrize(
    "profile",
    [
        {"provider": "openrouter", "model": "a", "api_key": "secret"},
        {
            "provider": "compatible",
            "model": "a",
            "base_url": "https://secret:pass@example.org",
        },
    ],
)
def test_literal_credentials_rejected(settings, profile):
    """Prevent configuration and manifests from containing raw key fields or URL secrets."""
    settings["profiles"] = {"bad": profile}
    with pytest.raises(ValueError):
        validate_config(settings)
