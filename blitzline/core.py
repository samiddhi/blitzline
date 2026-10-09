"""Expose the explicit recording-to-cards pipeline to CLI and Python callers.

Scope statement: coordinate stages, prerequisites, and resume decisions.
Included: source/transcript/review/media/export stage order, effective run
metadata, stopping for review, and recovering completed stage artifacts.
Excluded: text algorithms (transcript.py/vocabulary.py), HTTP/subprocess details
(integrations/), file primitives (storage.py), and presentation (cli.py).
Start here: run_pipeline starts a run; resume_pipeline reuses its saved request;
export_run validates completed cards before choosing a delivery adapter.
"""

import re
from copy import deepcopy
from dataclasses import replace
from importlib.util import find_spec
from pathlib import Path

from blitzline import __version__
from blitzline.config import (
    llm_profile,
    resolve_export,
    upgrade_settings,
    validate_config,
)
from blitzline.exports.anki import deliver
from blitzline.exports.model import export_dependencies
from blitzline.exports.package import export_package
from blitzline.exports.tsv import export_tsv
from blitzline.integrations import asr, blitzer
from blitzline.integrations.llm import request_json
from blitzline.media import clip_plan, extract_clip, probe, tool_identity
from blitzline.records import (
    NeedsReview,
    PipelineError,
    encode,
    load_candidates,
    load_cards,
    load_cues,
)
from blitzline.review import review_cues, review_vocabulary
from blitzline.storage import (
    RunStore,
    atomic_text,
    digest,
    fingerprint,
    read_json,
    run_lock,
    write_json,
)
from blitzline.transcript import (
    normalize_asr,
    parse_srt,
    plaintext,
    readability_issues,
    render_srt,
    replace_text,
    validate_cues,
)
from blitzline.vocabulary import (
    build_cards,
    map_candidates,
    review_template,
    validate_decisions,
)

STAGES = (
    "transcript",
    "proofread",
    "translate",
    "vocabulary",
    "review",
    "media",
    "export",
)


def run_pipeline(
    source,
    language: str,
    settings: dict,
    *,
    directory=None,
    srt=None,
    asr_json=None,
    review_file=None,
    stop_after=None,
    progress=None,
    requester=request_json,
) -> Path:
    """Run or reuse stages for local media, raising NeedsReview before unsafe delivery."""
    validate_config(settings)
    if not re.fullmatch(r"[a-z]{3}", language):
        raise ValueError(
            "Use a three-letter lowercase Blitzer language code; base cannot lemmatize"
        )
    resolve_export(settings, language)
    if srt and asr_json:
        raise ValueError("Choose either an SRT or ASR JSON input")
    if stop_after and stop_after not in STAGES:
        raise ValueError(f"Unknown stop stage: {stop_after}")
    source = Path(source).expanduser().resolve()
    source_hash = fingerprint(source)
    if source_hash == "missing":
        raise ValueError(f"Media file not found: {source}")
    request = {
        "source": str(source),
        "language": language,
        "srt": str(Path(srt).expanduser().resolve()) if srt else None,
        "asr_json": str(Path(asr_json).expanduser().resolve()) if asr_json else None,
        "review_file": (
            str(Path(review_file).expanduser().resolve()) if review_file else None
        ),
    }
    directory = (
        Path(directory).expanduser().resolve()
        if directory
        else default_directory(source, language, source_hash, settings)
    )
    with run_lock(directory):
        previous = (
            read_json(directory / "manifest.json")
            if (directory / "manifest.json").is_file()
            else {}
        )
        if previous and (
            previous.get("source_hash") != source_hash
            or previous.get("request", {}).get("language") != language
        ):
            raise ValueError(
                "Run belongs to different media/language; choose a new output directory"
            )
        store = RunStore(
            directory,
            {
                "source_hash": source_hash,
                "request": request,
                "settings": settings,
                "version": __version__,
                "state": "running",
            },
            progress,
        )
        try:
            execute_pipeline(
                store, source, language, settings, request, stop_after, requester
            )
        except NeedsReview as error:
            store.needs_review(str(error))
            raise NeedsReview(f"{error}\nRun: {directory}") from error
        except BaseException as error:
            store.manifest.update(state="failed", message=str(error))
            store.save()
            raise
    return directory


def default_directory(
    source: Path, language: str, source_hash: str, settings: dict
) -> Path:
    """Choose a predictable source/language run directory outside the repository."""
    stem = re.sub(r"[^\w.-]+", "_", source.stem)[:64] or "recording"
    return (
        Path(settings["run"]["output_dir"]).expanduser().resolve()
        / f"{stem}-{language}-{source_hash[:12]}"
    )


def completed_stop(store: RunStore, stage: str, stop_after) -> bool:
    """Mark an explicitly requested stop as paused without prompting the user."""
    if stage != stop_after:
        return False
    store.manifest.update(state="stopped", stop_after=stage)
    store.save()
    return True


def execute_pipeline(
    store, source, language, settings, request, stop_after, requester
) -> None:
    """Run each named stage in order with independent input/cache dependencies."""
    info = store.stage(
        "00-source",
        [store.manifest["source_hash"], tool_identity(settings["media"]["ffprobe"])],
        lambda: (probe(source, settings["media"]), []),
    )
    if settings["media"]["kind"] == "video" and info["video_index"] is None:
        raise ValueError("Video clips requested for an audio-only source")
    if stop_after not in {"transcript", "proofread", "translate"}:
        blitzer.preflight(language, settings["blitzer"])
    if (
        not stop_after
        and settings["export"]["format"] == "apkg"
        and not find_spec("genanki")
    ):
        raise PipelineError("APKG export requires: pip install 'blitzline[apkg]'")
    transcript_input = request["srt"] or request["asr_json"]
    input_hash = fingerprint(transcript_input) if transcript_input else None
    if transcript_input and input_hash == "missing":
        raise ValueError(f"Transcript input is missing: {transcript_input}")
    raw = store.stage(
        "01-transcript",
        [
            store.manifest["source_hash"],
            transcript_input,
            input_hash,
            settings["asr"],
            (
                tool_identity(settings["asr"]["executable"])
                if not transcript_input and settings["asr"]["backend"] == "local"
                else None
            ),
            (
                tool_identity(settings["media"]["ffmpeg"])
                if not transcript_input
                else None
            ),
        ],
        lambda: prepare_transcript(store, source, language, request, info, settings),
    )
    cues = validate_cues(load_cues(raw), info["duration_ms"])
    if completed_stop(store, "transcript", stop_after):
        return
    proof = store.stage(
        "02-proofread",
        [
            raw,
            settings["run"]["proofread"],
            settings["asr"]["relisten"],
            settings["asr"],
            llm_profile(settings, "proofread"),
            settings["llm"],
        ],
        lambda: prepare_proofread(store, source, language, cues, settings, requester),
    )
    if proof["errors"]:
        store.artifact("source.srt").unlink(missing_ok=True)
        raise NeedsReview(
            "Proofreading needs review; inspect artifacts/02-proofread-report.json. Resume with corrected --srt and --no-proofread."
        )
    cues = validate_cues(load_cues(proof["cues"]), info["duration_ms"])
    if completed_stop(store, "proofread", stop_after):
        return
    translated = store.stage(
        "03-translate",
        [
            encode(cues),
            settings["run"]["translate"],
            llm_profile(settings, "translate"),
            settings["llm"],
        ],
        lambda: prepare_translation(store, cues, language, settings, requester),
    )
    if translated["errors"]:
        raise NeedsReview(
            "Translation needs review; inspect artifacts/03-translation-report.json or resume with --no-translate."
        )
    if completed_stop(store, "translate", stop_after):
        return
    dependency = blitzer.dependencies(language, settings["blitzer"])
    candidates_raw = store.stage(
        "04-vocabulary",
        [encode(cues), dependency],
        lambda: prepare_vocabulary(store, cues, language, settings),
    )
    candidates = load_candidates(candidates_raw)
    if completed_stop(store, "vocabulary", stop_after):
        return
    review_path = request["review_file"]
    review_hash = fingerprint(review_path) if review_path else None
    if review_path and review_hash == "missing":
        raise ValueError(f"Review file is missing: {review_path}")
    reviewed = store.stage(
        "05-review",
        [candidates_raw, review_hash, llm_profile(settings, "review"), settings["llm"]],
        lambda: prepare_review(
            store, candidates, language, settings, review_path, requester
        ),
    )
    unresolved = [
        row for row in reviewed["decisions"] if row["decision"] == "needs-review"
    ]
    if (reviewed["errors"] or unresolved) and not settings["run"]["allow_partial"]:
        raise NeedsReview(
            "Vocabulary needs review; edit artifacts/review.json and resume with --review FILE. See artifacts/05-review-report.json."
        )
    if completed_stop(store, "review", stop_after):
        return
    cards = build_cards(
        reviewed["decisions"],
        candidates,
        store.manifest["source_hash"],
        language,
        source.name,
    )
    clipped = store.stage(
        "06-media",
        [
            encode(cards),
            settings["media"],
            info,
            tool_identity(settings["media"]["ffmpeg"]),
        ],
        lambda: prepare_media(store, cards, source, info, settings),
    )
    store.manifest["delivery_inputs"] = {
        "request": request,
        "files": {str(source): store.manifest["source_hash"]},
        "blitzer": dependency,
    }
    if transcript_input:
        store.manifest["delivery_inputs"]["files"][transcript_input] = input_hash
    if review_path:
        store.manifest["delivery_inputs"]["files"][review_path] = review_hash
    for path, checksum in store.manifest["delivery_inputs"]["files"].items():
        if fingerprint(path) != checksum:
            raise PipelineError(
                "An input changed during processing; resume with stable inputs"
            )
    store.save()
    if clipped["errors"] and not settings["run"]["allow_partial"]:
        raise NeedsReview(
            "Some clips need review; see artifacts/06-media-report.json. Adjust media limits or review examples before resuming."
        )
    if completed_stop(store, "media", stop_after):
        return
    export_stage(store, load_cards(clipped["cards"]), settings)
    store.manifest.update(
        state="complete",
        cards=len(clipped["cards"]),
        omitted=len(reviewed["errors"]) + len(unresolved) + len(clipped["errors"]),
    )
    store.save()


def prepare_transcript(store, source, language, request, info, settings):
    """Import or transcribe source media and retain normalized/raw subtitle artifacts."""
    paths = []
    if request["srt"]:
        original = Path(request["srt"]).read_text(encoding="utf-8-sig")
        raw_path = store.artifact("original.srt")
        atomic_text(raw_path, original)
        cues = parse_srt(original, info["duration_ms"])
    else:
        data = (
            read_json(Path(request["asr_json"]))
            if request["asr_json"]
            else asr.transcribe(
                source,
                language,
                settings["asr"],
                media_settings=settings["media"],
                info=info,
                cache_dir=store.directory / ".cache" / "asr",
            )
        )
        raw_path = store.artifact("raw-asr.json")
        write_json(raw_path, data)
        cues = normalize_asr(data, info["duration_ms"], settings["asr"])
    paths.append(raw_path)
    srt_path = store.artifact("unproofread.srt")
    atomic_text(srt_path, render_srt(cues, settings["asr"]["line_length"]))
    readability = store.artifact("01-readability.json")
    write_json(readability, readability_issues(cues, settings["asr"]))
    return encode(cues), [*paths, srt_path, readability]


def prepare_proofread(store, source, language, cues, settings, requester):
    """Apply configured proofreading and optional second-ASR evidence, preserving times."""
    if not settings["run"]["proofread"] or not cues:
        path = store.artifact("source.srt")
        atomic_text(path, render_srt(cues, settings["asr"]["line_length"]))
        return {"cues": encode(cues), "errors": [], "skipped": True}, [path]
    if not llm_profile(settings, "proofread"):
        raise ValueError("Proofreading requires an LLM profile")
    cache = store.directory / ".cache" / "requests"
    result = review_cues(
        cues, language, "proofread", settings, cache, requester=requester
    )
    uncertain = {row["cue_id"] for row in result["cues"] if row["uncertain"]}
    if uncertain and settings["asr"]["relisten"]:
        selected = [cue for cue in cues if cue.id in uncertain]
        evidence = {
            cue.id: asr.relisten(
                source,
                cue,
                language,
                settings["asr"],
                settings["media"],
                cache_dir=store.directory / ".cache" / "asr",
            )
            for cue in selected
        }
        second = review_cues(
            selected,
            language,
            "proofread",
            settings,
            cache,
            evidence=evidence,
            requester=requester,
        )
        result["cues"] = [
            row for row in result["cues"] if row["cue_id"] not in uncertain
        ] + second["cues"]
        result["errors"].extend(second["errors"])
        result["second_asr"] = evidence
    result["errors"].extend(
        {"cue_id": row["cue_id"], "error": row["reason"]}
        for row in result["cues"]
        if row["uncertain"]
    )
    replacements = {
        row["cue_id"]: row["text"] for row in result["cues"] if not row["uncertain"]
    }
    report = store.artifact("02-proofread-report.json")
    write_json(report, result)
    corrected = replace_text(cues, replacements)
    paths = [report]
    if not result["errors"]:
        path = store.artifact("source.srt")
        atomic_text(path, render_srt(corrected, settings["asr"]["line_length"]))
        paths.append(path)
    return {
        "cues": encode(corrected),
        "errors": result["errors"],
    }, paths


def prepare_translation(store, cues, language, settings, requester):
    """Render English SRT locally using unchanged source cue order and timestamps."""
    if not settings["run"]["translate"] or not cues:
        store.artifact("english.srt").unlink(missing_ok=True)
        return {"errors": [], "skipped": True}, []
    if not llm_profile(settings, "translate"):
        raise ValueError("Translation requires an LLM profile")
    result = review_cues(
        cues,
        language,
        "translate",
        settings,
        store.directory / ".cache" / "requests",
        requester=requester,
    )
    report = store.artifact("03-translation-report.json")
    write_json(report, result)
    paths = [report]
    if not result["errors"]:
        translated = replace_text(
            cues, {row["cue_id"]: row["text"] for row in result["cues"]}
        )
        path = store.artifact("english.srt")
        atomic_text(path, render_srt(translated, settings["asr"]["line_length"]))
        paths.append(path)
    else:
        store.artifact("english.srt").unlink(missing_ok=True)
    return result, paths


def prepare_vocabulary(store, cues, language, settings):
    """Save canonical plaintext/span data and mapped Blitzer vocabulary."""
    source, spans = plaintext(cues)
    path = store.artifact("transcript.txt")
    atomic_text(path, source)
    span_path = store.artifact("transcript-spans.json")
    write_json(span_path, encode(spans))
    rows = blitzer.extract(language, path, settings["blitzer"]) if source else []
    raw_path = store.artifact("blitzer.json")
    write_json(raw_path, rows)
    return encode(map_candidates(rows, cues)), [path, span_path, raw_path]


def prepare_review(store, candidates, language, settings, review_path, requester):
    """Validate manual/API review and write reports without overwriting user edits."""
    editable = store.artifact("review.json")
    template = store.artifact("review-template.json")
    write_json(template, review_template(candidates))
    if review_path:
        result = read_json(Path(review_path))
    elif candidates and llm_profile(settings, "review"):
        result = review_vocabulary(
            candidates,
            language,
            settings,
            store.directory / ".cache" / "requests",
            requester,
        )
        result = {"decisions": result["decisions"]}
    else:
        result = review_template(candidates)
    valid, errors = validate_decisions(result, candidates)
    report = {
        "decisions": valid,
        "errors": errors,
        "source_frequency_policy": "Unchanged Blitzer candidate count before semantic review",
    }
    report_path = store.artifact("05-review-report.json")
    write_json(report_path, report)
    if not editable.exists():
        write_json(editable, {"decisions": valid})
    return report, [report_path, template]


def prepare_media(store, cards, source, info, settings):
    """Plan all card intervals first, then cut valid clips and report failed cards."""
    plans, valid, errors = {}, [], []
    encoder_identity = tool_identity(settings["media"]["ffmpeg"])
    for card in cards:
        try:
            selected = [
                clip_plan(
                    c,
                    store.manifest["source_hash"],
                    info,
                    settings["media"],
                    encoder_identity,
                )
                for c in card.contexts
            ]
            plans[card.id] = selected
            valid.append(card)
        except ValueError as error:
            errors.append({"card_id": card.id, "error": str(error)})
    finished, paths = [], []
    for card in valid:
        try:
            clips = [
                extract_clip(
                    source, store.directory / "media", plan, info, settings["media"]
                )
                for plan in plans[card.id]
            ]
            paths.extend(clips)
            finished.append(replace(card, media=tuple(p.name for p in clips)))
        except PipelineError as error:
            errors.append({"card_id": card.id, "error": str(error)})
    result = {"cards": encode(finished), "errors": errors}
    report = store.artifact("06-media-report.json")
    write_json(report, result)
    return result, [report, *set(paths)]


def export_stage(store, cards, settings, transport=None):
    """Export checksum-valid cards; always recheck live Anki for direct delivery."""
    options = resolve_export(
        settings, store.manifest.get("request", {}).get("language")
    )
    kind = options["format"]
    destination = store.directory / "exports" / kind
    adapters = {"tsv": export_tsv, "apkg": export_package, "anki": deliver}
    transport_options = {"transport": transport} if kind == "anki" and transport else {}

    def action():
        """Run the selected delivery adapter and return its tracked output paths."""
        paths = adapters[kind](
            cards, store.directory / "media", destination, options, **transport_options
        )
        return {
            "format": kind,
            "cards": len(cards),
            "deck": options["deck"],
            "deck_profile": options["deck_profile"],
            "model": options["model"],
            "paths": [str(p) for p in paths],
        }, paths

    if kind == "anki":
        result, paths = action()
        write_json(store.artifact("07-export-anki.json"), result)
        return result
    return store.stage(
        "07-export-" + kind,
        [encode(cards), options, export_dependencies(options)],
        action,
    )


def resume_pipeline(
    directory,
    *,
    settings=None,
    overrides=None,
    srt=None,
    asr_json=None,
    review_file=None,
    stop_after=None,
    progress=None,
    requester=request_json,
) -> Path:
    """Resume the saved media request with optional settings/transcript/review changes."""
    directory = Path(directory).expanduser().resolve()
    manifest = read_json(directory / "manifest.json")
    selected = settings or upgrade_settings(manifest["settings"])
    # Revalidate merged CLI settings rather than silently accepting saved typos.
    selected = deepcopy(selected)
    for section, values in (overrides or {}).items():
        selected[section].update({k: v for k, v in values.items() if v is not None})
    request = manifest["request"]
    chosen_srt = srt or (None if asr_json else request.get("srt"))
    chosen_json = asr_json or (None if srt else request.get("asr_json"))
    return run_pipeline(
        request["source"],
        request["language"],
        selected,
        directory=directory,
        srt=chosen_srt,
        asr_json=chosen_json,
        review_file=review_file or request.get("review_file"),
        stop_after=stop_after,
        progress=progress,
        requester=requester,
    )


def export_run(directory, *, settings=None, overrides=None, transport=None) -> dict:
    """Export only a current, verified media stage; allow explicit partial delivery."""
    directory = Path(directory).expanduser().resolve()
    with run_lock(directory):
        manifest = read_json(directory / "manifest.json")
        selected = settings or upgrade_settings(manifest["settings"])
        selected = deepcopy(selected)
        for section, values in (overrides or {}).items():
            selected[section].update({k: v for k, v in values.items() if v is not None})
        validate_config(selected)
        assert_export_current(manifest, directory)
        record = manifest["stages"].get("06-media", {})
        if record.get("state") != "complete":
            raise PipelineError("No completed media stage; resume the run first")
        for path, checksum in record["outputs"].items():
            if fingerprint(directory / path) != checksum:
                raise PipelineError("Media artifacts changed; resume before exporting")
        result = read_json(directory / "artifacts" / "06-media.json")
        review = read_json(directory / "artifacts" / "05-review.json")
        unresolved = (
            review["errors"]
            or any(r["decision"] == "needs-review" for r in review["decisions"])
            or result["errors"]
        )
        if unresolved and not selected["run"]["allow_partial"]:
            raise NeedsReview(
                "Unresolved items remain; resume with review or explicitly allow partial export"
            )
        store = RunStore(directory, {}, None)
        return export_stage(store, load_cards(result["cards"]), selected, transport)


def assert_export_current(manifest: dict, directory: Path) -> None:
    """Reject exports when upstream artifacts, external inputs, or Blitzer data changed."""
    snapshot = manifest.get("delivery_inputs")
    if not snapshot or snapshot["request"] != manifest["request"]:
        raise PipelineError(
            "Run inputs changed or are incomplete; resume before exporting"
        )
    for path, checksum in snapshot["files"].items():
        if fingerprint(path) != checksum:
            raise PipelineError("An input file changed; resume before exporting")
    current = blitzer.dependencies(
        manifest["request"]["language"], manifest["settings"]["blitzer"]
    )
    if digest(current) != digest(snapshot["blitzer"]):
        raise PipelineError("Blitzer inputs changed; resume before exporting")
    for stage, record in manifest["stages"].items():
        if stage[:2] > "06":
            continue
        if record.get("state") not in {"complete", "skipped"}:
            raise PipelineError(
                "An upstream stage is incomplete; resume before exporting"
            )
        for path, checksum in record["outputs"].items():
            if fingerprint(directory / path) != checksum:
                raise PipelineError(
                    "An upstream artifact changed; resume before exporting"
                )
