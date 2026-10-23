# Production UI audit — 2026-10-21

The GUI was searched for prototype/simulation/sample code, fault injection,
placeholder settings and exports, disconnected controls, import limits, data
paths, and persistence/recovery behavior. Simulation is now an explicit property
of an example project; a missing native run is never permission to simulate.

## Corrected

| Finding | Correction |
| --- | --- |
| Unknown audio extensions entered the text importer and hit its 2 MB cap | Expanded recording extensions and added common container/header recognition. Audio/video files have **no Blitzline file-size cap**. Import reads only a small recording header; hashing is streamed and transcription is chunked. A sparse 6 GiB recording regression covers large inputs. |
| Multi-file imports silently hid earlier failures | File selection and drag/drop now use one batch operation. Every successful file is retained; failures are reported by filename and remain visible beside the selected files even if another UI action clears the general banner. |
| Old prototype text limit and cached copies of source documents | Text uses the backend's 128 MiB safety limit, with a production error message. File-backed text is validated but not duplicated into workspace JSON; processing reads the source file. This limit does not apply to recordings. |
| Save for later appeared to lose the project | The setup was persisted but had no visible return path. Projects now shows a Saved project setup card with Continue saved setup. Text, file references, settings, and current step survive restarting. A failed save keeps the editor open and reports the failure. Saving a setup does not start processing or contact a service. |
| Saved profiles were unclear and could not be deselected | Added No saved profile. Deselecting retains the visible choices as manual settings, but removes the association and profile word lists. Stage help explains that a profile is a reusable set of language/deck/review preferences and word lists. |
| Setup stages lacked contextual help | Each stage heading has a small help button with both hover text and a click popup, including browser-chat versus API processing, transcription, destination decks, profiles, starting, and saving for later. |
| Any project without a run path could enter the demo engine | Only a project created explicitly with Try an example receives the example marker. Ordinary projects cannot receive canned results. Old saved previews are preserved and offer Start real project from this setup; sample answers are not promoted into real results. |
| Simulated connection-error control in the UI | Removed the fault-injection button. The development helper rejects non-example projects. |
| Fake delivery action in the example export screen | Removed the Anki delivery action from examples. Practice exports and sample replies remain explicitly labeled inside Try an example only. Real projects retain real APKG/TSV/AnkiConnect delivery. |
| Outdated Help claimed the app made no requests | Replaced with actual behavior: automatic services may send data/incur charges; browser assistance does not need an API key. |
| Fake local-model status in Settings | Reports the selected native model file's real presence. The historical model-installed boolean is no longer displayed as production status. |
| Global batch-size default disconnected from setup | Updates the setup default and is retained when the next setup is initialized. Per-project choices remain available. |
| A missing subtitle attachment could leave a manifest without the attachment | Attachment paths are resolved before creating the native run. Failure cannot turn a subsequent retry into an unintended transcription request. |
| Local selection could retain an HTTP/Groq backend from configuration | On this computer explicitly selects the local backend. Inconsistent older saved runs stop before upload and require applying Project settings. |
| No GUI entry for an online transcription credential | Added a separate session-only transcription key in Settings. It is scoped to the configured Groq/HTTP connection, never persisted, and cleared when selecting another configuration. Environment credentials remain supported. |
| Session key could be reused after changing providers/endpoints | Provider changes clear service-bound key/endpoint/model fields; endpoint changes clear the key. Existing runs reject a session key when their saved connection differs from Settings. Model and transcription credentials are separate. |
| Card count was an estimate based only on recall | Native projects count active cards using the actual note templates and fields. |
| Known-word buttons wrote to the first profile regardless of selection | They now target the project's selected profile. Projects without a profile explain how to use one rather than modifying an unrelated language's list. |
| Card-design labels still described a prototype | Labels explain actual Anki export styling and the intentional plain-text preview. |

## Persistence and distribution

Normal desktop launches use the operating system's per-user application-data
folder, outside the source repository and installed app bundle: Application
Support on macOS, Local AppData on Windows, and XDG data storage on Linux.
The desktop retains its original application-directory name for continuity;
renaming that directory must not silently hide existing work. Native runs live
under the library's runs directory. The CLI has its own OS application-data run
root and supports explicitly selected output folders. Development-only
`--data-dir` overrides may deliberately point inside a checkout.

Source recordings/documents remain in their original locations; the app stores
references. Pasted text and processing artifacts are saved in the native run.
Model/transcription keys entered in the GUI stay in memory for the session.

The intended consumer product is a **prebuilt application**, with no source
checkout, terminal, Rust compiler, Cargo, or Python required. Building is a
developer/CI activity. The local macOS app includes a standalone CLI that can be
installed from Settings. Signed installers, Windows/Linux desktop packaging,
complete licensed native-tool/model bundles, and clean-machine acceptance still
need release work; source-build instructions are not the end-user installation
experience.

## Deliberate examples and remaining limitations

Try an example remains available for learning the interface without accounts.
It uses bundled words, canned replies, a practice player, and practice exports;
it is visibly separate from real projects. Legacy preview projects must be
restarted from their source before doing real work. The separate blitzline-demo
binary is a development tool and is not included by the app packaging helper.

Features that remain incomplete are documented rather than presented as working:
OS credential-manager persistence, fully guided managed downloads/updates,
embedded media playback and full HTML rendering in preview, and editing timed
media cues in place. The current app opens recordings in the system player,
previews card content as text, and accepts corrected subtitles for a new run.
The compiled Blitzer distribution remains an external dependency.

The updated full suite passes **76 Rust tests**, including the real-FFmpeg and
local-HTTP cases. Formatting and Clippy with warnings denied are checked as part
of verification.

Regression coverage includes mixed import success/failure, large/unknown-extension
recordings, text over 2 MB, save/restart and save failure, optional profiles,
clickable help, legacy/example isolation, real native exports/card counts,
attachment failure, local transcription routing, and credential isolation.
