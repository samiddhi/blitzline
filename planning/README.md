# Blitzline planning

Blitzline is being rebuilt as a shared Rust engine with a CLI and a desktop
application that beginners can use without opening a terminal or installing
Python.

This repository is the new project. The Python reference implementation lives
at [`../../blitzline-py`](../../blitzline-py). Do not port from an older checkout
or modify the reference as part of this work.

## Reading order

1. [Architecture](architecture.md): approved organization, responsibilities, and
   user-facing principles.
2. [Rust port](rust-port.md): implementation contracts, technology choices,
   milestones, and release gates.
3. [Compatibility](compatibility.md): preserved behavior, deliberate changes,
   and reference-baseline procedure.
4. [Full GUI specification](gui.md): windows, navigation, screens, browser
   assistance, settings, recovery, and accessibility.
5. [GUI acceptance](gui-acceptance.md): implemented prototype surfaces, tests,
   and remaining integrations.
6. [macOS design revision](macos-design.md): native toolbar/toolkit decision,
   menus, appearance, visual system, and platform acceptance boundaries.

## Decisions recorded on 2026-10-20

The user approved the overall organization and selected:

- macOS, Windows, and Linux for the first desktop release.
- Importing Python settings, while leaving existing Python jobs with
  `blitzline-py`.
- Preserving Anki note identity and supported output behavior.
- Both managed local and remote transcription.
- A native replacement for WhisperX, accepting different recognition and timing
  results; equivalent forced alignment is not a release requirement.
- A ready-to-use hosted transcription API and custom HTTP workers. Direct Modal
  function invocation is not required in v1.
- GUI design and a simulated GUI skeleton before connecting real processing.

The port plan records implementation defaults, including `whisper.cpp` and an
initial Groq transcription adapter. These are recommendations from the planning
discussion, distinguished from the user-selected requirements above.

Blitzer's compiled replacement is a separate project. Its integration contract
and availability are dependencies of Blitzline's release.

## Current status

The native CLI and background desktop pipeline are implemented in Rust, with
explicit demonstration workflows retained separately. See
[implementation status](implementation-status.md) for capabilities, evidence,
limitations, and remaining release gates.

| Milestone | Status |
| --- | --- |
| A. Documentation and baseline | Captured working-tree provenance; reference suite 184 passed; synthetic fixtures recorded |
| B. Shared contracts and simulated engine | Implemented; examples remain explicitly isolated |
| C. GUI design and skeleton | Retained as application; native toolbar and approved appearance; accessibility acceptance remains |
| D. Domain and storage | Implemented with fixture, transaction, lock, corruption, and restart tests |
| E. First working pipeline | Native CLI and desktop processing connected; compiled Blitzer release remains external |
| F. LLM automation | Implemented; provider/repair behavior verified with local services |
| G. Media and transcription | Native adapters and second ASR implemented; real-media and HTTP tests; model/service release acceptance pending |
| H. Complete exports | Native TSV/APKG/AnkiConnect implemented; fixture/fake-service checks; isolated live-Anki acceptance pending |
| I. Distribution and release | Bundled CLI, GUI CLI installation, archive/build workflows; signing/installers/catalogs/clean-machine checks pending |

The approved egui/eframe application and Rust AppKit bridge remain in use. The
GUI prototype is being completed as the product, not replaced by another GUI.
No installed component requires Python. Distribution readiness remains distinct
from implemented pipeline capabilities.
