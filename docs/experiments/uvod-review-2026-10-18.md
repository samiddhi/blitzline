# Uvod vocabulary-review experiments (17–18 October 2026)

Local archive: `maintenance/archive/uvod-review-2026-10-18/`. The archive is
ignored by Git because its results and request caches contain excerpts from the
user's subtitles. It is not included in the PyPI package. The actual pipeline
run, its audio clips, and its exported deck remain in `.runs/uvod-slv/`; the
active configuration remains in `.runs/config.toml` until the user moves it.

The archived Python probes and JSON results compare reasoning on/off, batch
sizes of 5 and 20, the revised context-sensitive prompt, a ten-candidate
ambiguity pass, and hybrid first-pass/focused-review variants. Three small
`*-cache/` directories hold responses from those experiments. `config.toml~`
is an old backup, not the active configuration. These are historical probes;
their hard-coded paths should be updated before rerunning them.

The chosen production route for this Uvod run was 20-candidate non-thinking
review batches with shared-form lemmas kept together, followed by one thinking
audit of all 407 candidates. The first pass used 21 cached responses costing
$0.025617352; the audit cost $0.01652176. The combined review cost recorded
in this run's cache was $0.042139112. The audit supplied 14 corrections,
including the `boleče`, `mučno`, `neznosno`, and `obetavno` adjective/adverb
cases. The final review had 339 keeps, 68 rejects, and no unresolved errors.
Media cutting produced 398 clips for the 339 accepted vocabulary entries with
no clip failures.

The archive is separate from `.runs/uvod-slv/.cache/requests`: that cache is
part of the resumable run and can prevent repeat paid requests. Do not remove
the run's intermediates until the exported deck has been checked in Anki.
