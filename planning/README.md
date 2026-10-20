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

A Cargo workspace, shared simulated engine, CLI, and native desktop skeleton
now implement the GUI plan for review. The full GUI specification is recorded.
Real pipeline porting and reference-baseline capture remain pending.

| Milestone | Status |
| --- | --- |
| A. Documentation and baseline | Planning documents written; baseline capture and detailed inventory pending |
| B. Shared contracts and simulated engine | Implemented for prototype workflows |
| C. GUI design and skeleton | Native prototype implemented; user review and complete accessibility acceptance pending |
| D. Domain and storage | Planned |
| E. First working pipeline | Planned |
| F. LLM automation | Planned |
| G. Media and transcription | Planned |
| H. Complete exports | Planned |
| I. Distribution and release | Planned |

The prototype uses egui/eframe with AccessKit. This is an implementation default
for evaluating the approved interaction structure, not a final production
toolkit commitment. Minimum OS versions and release installers remain pending.
A local macOS application-bundle helper is available.

Keep milestone status here, architectural responsibilities in
`architecture.md`, implementation sequencing in `rust-port.md`, and migration
exceptions in `compatibility.md`. Update the relevant document when a decision
changes rather than leaving contradictory plans.
