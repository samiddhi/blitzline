"""Present the recording-to-Anki pipeline as a small command-line interface.

Scope statement: translate explicit terminal choices into application calls.
Included: run/resume/export/check commands, option validation, concise progress,
artifact locations, dependency diagnostics, and documented exit statuses.
Excluded: stage algorithms (core.py/transcript.py/vocabulary.py), provider calls
(integrations/), persistence (storage.py), and export construction (exports/).
Start here: cli is the console entry; run starts the main pipeline.
Exit statuses: 0 success/intentional stop, 1 failed stage, 2 invalid input,
3 awaiting review. Commands never prompt during processing.
"""

import os
import shutil
from contextlib import contextmanager
from importlib.util import find_spec
from pathlib import Path

import click

from blitzline import __version__
from blitzline.config import get_config, llm_profile, resolve_language
from blitzline.core import STAGES, export_run, resume_pipeline, run_pipeline
from blitzline.integrations.blitzer import preflight
from blitzline.records import NeedsReview, PipelineError
from blitzline.storage import read_json

CONTEXT = {"help_option_names": ["-h", "--help"]}


@contextmanager
def errors():
    """Convert expected failures into concise CLI messages and stable exit codes."""
    try:
        yield
    except NeedsReview as error:
        click.echo(str(error), err=True)
        raise click.exceptions.Exit(3) from None
    except (ValueError, KeyError, TypeError) as error:
        raise click.UsageError(str(error)) from None
    except (PipelineError, OSError) as error:
        raise click.ClickException(str(error)) from None


def common_options(function):
    """Attach shared explicit configuration and delivery overrides to commands."""
    options = [
        click.option(
            "--deck-profile", help="Configured language/deck/card-type profile."
        ),
        click.option(
            "--config", type=click.Path(exists=True, dir_okay=False, path_type=Path)
        ),
        click.option(
            "--format", "output_format", type=click.Choice(["tsv", "apkg", "anki"])
        ),
        click.option("--clips", type=click.Choice(["audio", "video"])),
        click.option("--proofread/--no-proofread", default=None),
        click.option("--translate/--no-translate", default=None),
        click.option("--allow-partial/--no-allow-partial", default=None),
    ]
    for option in options:
        function = option(function)
    return function


def transcript_options(function):
    """Attach import, review, and stage-stop choices to run/resume commands."""
    options = [
        click.option(
            "--asr-backend",
            type=click.Choice(["local", "modal", "http"]),
            help="Transcription backend; remote endpoints are configured in TOML.",
        ),
        click.option(
            "--srt", type=click.Path(exists=True, dir_okay=False, path_type=Path)
        ),
        click.option(
            "--asr-json", type=click.Path(exists=True, dir_okay=False, path_type=Path)
        ),
        click.option(
            "--review",
            "review_file",
            type=click.Path(exists=True, dir_okay=False, path_type=Path),
        ),
        click.option("--stop-after", type=click.Choice(STAGES)),
    ]
    for option in options:
        function = option(function)
    return function


def overrides(
    output_format=None,
    clips=None,
    proofread=None,
    translate=None,
    allow_partial=None,
    asr_backend=None,
    deck_profile=None,
):
    """Build nested non-None CLI choices for the configuration/application APIs."""
    return {
        "export": {"format": output_format, "deck_profile": deck_profile},
        "asr": {"backend": asr_backend},
        "media": {"kind": clips},
        "run": {
            "proofread": proofread,
            "translate": translate,
            "allow_partial": allow_partial,
        },
    }


def progress(message: str) -> None:
    """Show a brief stage message on stderr, leaving stdout for results."""
    click.echo(message, err=True)


def show_result(directory: Path) -> None:
    """Print the run directory, state, card count, and completed artifact locations."""
    manifest = read_json(directory / "manifest.json")
    click.echo(f"Run: {directory}")
    click.echo(f"State: {manifest['state']}; cards: {manifest.get('cards', 0)}")
    if (directory / "artifacts" / "source.srt").is_file():
        click.echo(f"Subtitles: {directory / 'artifacts' / 'source.srt'}")
    if manifest["state"] == "complete":
        click.echo(
            f"Exports: {directory / 'exports' / manifest['settings']['export']['format']}"
        )


@click.group(context_settings=CONTEXT)
@click.version_option(__version__)
def cli():
    """Turn a foreign-language recording into subtitles and Anki vocabulary."""


@cli.command()
@click.argument("media", type=click.Path(exists=True, dir_okay=False, path_type=Path))
@click.option(
    "--language",
    "-l",
    help="Blitzer three-letter language code; inferred from --deck-profile when omitted.",
)
@click.option("--output", "directory", type=click.Path(file_okay=False, path_type=Path))
@common_options
@transcript_options
def run(
    media,
    language,
    directory,
    config,
    deck_profile,
    output_format,
    clips,
    proofread,
    translate,
    allow_partial,
    asr_backend,
    srt,
    asr_json,
    review_file,
    stop_after,
):
    """Process MEDIA, optionally starting from an existing subtitle/transcript."""
    with errors():
        settings = get_config(
            config,
            overrides(
                output_format,
                clips,
                proofread,
                translate,
                allow_partial,
                asr_backend,
                deck_profile,
            ),
        )
        result = run_pipeline(
            media,
            resolve_language(settings, language),
            settings,
            directory=directory,
            srt=srt,
            asr_json=asr_json,
            review_file=review_file,
            stop_after=stop_after,
            progress=progress,
        )
        show_result(result)


@cli.command()
@click.argument(
    "run_dir", type=click.Path(exists=True, file_okay=False, path_type=Path)
)
@common_options
@transcript_options
def resume(
    run_dir,
    config,
    deck_profile,
    output_format,
    clips,
    proofread,
    translate,
    allow_partial,
    asr_backend,
    srt,
    asr_json,
    review_file,
    stop_after,
):
    """Continue a saved run with optional edited review or transcript inputs."""
    with errors():
        selected = get_config(config) if config else None
        result = resume_pipeline(
            run_dir,
            settings=selected,
            overrides=overrides(
                output_format,
                clips,
                proofread,
                translate,
                allow_partial,
                asr_backend,
                deck_profile,
            ),
            srt=srt,
            asr_json=asr_json,
            review_file=review_file,
            stop_after=stop_after,
            progress=progress,
        )
        show_result(result)


@cli.command("export")
@click.argument(
    "run_dir", type=click.Path(exists=True, file_okay=False, path_type=Path)
)
@click.option("--config", type=click.Path(exists=True, dir_okay=False, path_type=Path))
@click.option(
    "--format",
    "output_format",
    type=click.Choice(["tsv", "apkg", "anki"]),
    required=True,
)
@click.option("--allow-partial/--no-allow-partial", default=None)
@click.option(
    "--deck-profile",
    help="Configured destination deck profile for this run's language.",
)
def export_command(run_dir, config, output_format, allow_partial, deck_profile):
    """Deliver existing validated cards as TSV, APKG, or directly to Anki."""
    with errors():
        result = export_run(
            run_dir,
            settings=get_config(config) if config else None,
            overrides=overrides(
                output_format=output_format,
                allow_partial=allow_partial,
                deck_profile=deck_profile,
            ),
        )
        for path in result["paths"]:
            click.echo(path)


@cli.command("check")
@click.option(
    "--language", "-l", help="Also verify this Blitzer language pack and config."
)
@click.option("--config", type=click.Path(exists=True, dir_okay=False, path_type=Path))
def check_command(config, language):
    """Show configured tools/providers and missing prerequisites without requests."""
    with errors():
        settings = get_config(config)
        missing = []
        for label, executable in (
            ("Blitzer", settings["blitzer"]["executable"]),
            ("FFmpeg", settings["media"]["ffmpeg"]),
            ("FFprobe", settings["media"]["ffprobe"]),
        ):
            path = shutil.which(executable)
            click.echo(f"{label}: {path or 'missing'}")
            if not path and not label.startswith("ASR"):
                missing.append(label)
        for name, profile in settings["decks"].items():
            click.echo(
                f"Deck profile {name}: {profile['language']} -> {profile['deck']}; card types: {', '.join(profile['card_types'])}"
            )
        asr = settings["asr"]
        click.echo(f"ASR backend: {asr['backend']} (not needed for imported SRT/JSON)")
        if asr["backend"] == "local":
            click.echo(f"WhisperX: {shutil.which(asr['executable']) or 'missing'}")
        elif asr["backend"] == "modal":
            click.echo(
                f"Modal: {asr['modal_app']}/{asr['modal_function']}; SDK: {'available' if find_spec('modal') else 'install blitzline[modal]'}"
            )
        else:
            click.echo(f"Remote ASR endpoint: {asr['endpoint']}")
            if asr["api_key_env"]:
                click.echo(
                    f"{asr['api_key_env']}: {'set' if os.environ.get(asr['api_key_env']) else 'missing'}"
                )
        click.echo(
            f"APKG writer: {'available' if find_spec('genanki') else 'install blitzline[apkg]'}"
        )
        for stage in ("proofread", "translate", "review"):
            profile = llm_profile(settings, stage)
            if not profile:
                click.echo(f"{stage}: no LLM profile")
                continue
            present = bool(os.environ.get(profile["api_key_env"]))
            click.echo(
                f"{stage}: {profile['provider']} / {profile['model']}; {profile['api_key_env']}: {'set' if present else 'missing'}"
            )
        if missing:
            raise PipelineError("Missing required tools: " + ", ".join(missing))
        if language:
            preflight(language, settings["blitzer"])
            click.echo(f"Blitzer {language}: ready")
