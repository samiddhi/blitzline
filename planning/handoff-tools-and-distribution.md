# Handoff: Tool Discovery and Linux/Windows Distribution

Continue the active Blitzline Rust port work in this repository. The user wants the native GUI and CLI to work for nontechnical users without Python, Rust, a terminal, or building from source on their machines. Preserve the existing GUI and architecture; this is a focused implementation task, not a redesign.

## Current user requests

1. Fix native tool discovery. The GUI reported that compiled `ffmpeg` was missing, even though it is installed. The same discovery problem affects `bltzr`.
2. Let users select tool binaries in Settings, while discovering common installation locations automatically.
3. Show required-tool failures as blocking warnings in one consistent location. The user explicitly clarified: they must appear in the same place, and FFmpeg must not be required unless the user chooses work that needs audio splitting/preparation.
4. Continue the work and produce usable Linux and Windows distributions, not just CI build definitions.

## Findings already established

- `/opt/homebrew/bin/ffmpeg` exists on the current Mac. GUI apps launched from Finder often receive a reduced `PATH`, so the current resolver misses Homebrew binaries.
- Current resolver: `crates/adapters/src/pipeline/process.rs::resolve`. It searches the bundled and managed tool directories and `PATH`, but not common OS locations.
- `/Library/Frameworks/Python.framework/Versions/3.12/bin/bltzr` exists, but it is a Python console-script launcher. The current native executable validator correctly rejects script files. Do not report this as a compiled Blitzer installation. A compiled Blitzer binary is still an external dependency and must be built/obtained separately.
- `ffmpeg` and `ffprobe` are checked together as media dependencies; media probing happens early in `crates/engine/src/pipeline/mod.rs`. This can make FFmpeg appear mandatory even when users do not want generated audio clips. Rework dependency checks and the media/card path so FFmpeg is only resolved when a selected operation needs it. Keep FFprobe requirements limited to operations that truly need media metadata. Preserve audio transcription where its chosen path requires conversion/chunking and explain that requirement at the point the user chooses it.
- A real GUI error is copied into both `Project.error` and the top-level `Desktop.error`, while `apps/desktop/src/screens.rs` also draws `Project.error` inside the project body. This causes inconsistent and duplicate placement. Use one persistent, blocking alert at the top of the active project view for project failures, with the same visual style/actions for all required tool failures. Do not leave a second casual inline copy. Settings/tool health can use the same alert component and placement convention.

## Relevant code

- Tool resolver and native binary validation: `crates/adapters/src/pipeline/process.rs`.
- Native pipeline defaults/config loading: `crates/adapters/src/pipeline/config.rs`.
- Tool status page and settings categories: `apps/desktop/src/settings.rs`; the existing Tools & downloads page is `Desktop::resources`.
- Error application/persistence: `apps/desktop/src/live.rs::poll_native`, `apps/desktop/src/lib.rs` top-level frame, and `apps/desktop/src/screens.rs::project`.
- Media probe/transcription/clips: `crates/adapters/src/pipeline/media.rs` and `crates/engine/src/pipeline/mod.rs`.
- User project media options: `crates/domain/src/lib.rs::Settings`, setup controls in `apps/desktop/src/screens.rs`, and preference/config application in `apps/desktop/src/live.rs::settings`.
- Existing project retries reopen saved native jobs in `apps/desktop/src/live.rs::execute_with_credentials`. Ensure selected global tool overrides are applied on retry as well as new-project creation, without replacing unrelated per-project settings.

## Tool configuration behavior to implement

- Search OS-standard executable directories in addition to `PATH` and bundled/managed resources. Include `/opt/homebrew/bin` and `/usr/local/bin` on macOS, standard system/local paths on Linux, and appropriate Windows executable locations. Keep explicit absolute paths authoritative: if the user explicitly selects a bad path, show that failure instead of silently substituting a different binary.
- Add native file pickers for at least Blitzer, FFmpeg, FFprobe, and local transcription in Settings/Tools & downloads. Validate selection as a compiled executable using the existing native-header check. Show the resolved path and a clear status. Offer “Choose…” and “Use automatic discovery” actions.
- Persist only non-secret paths in user preferences. Apply them to new and existing GUI jobs. Resetting to automatic discovery must remove an earlier GUI override cleanly.
- Errors for Blitzer should make clear that the current Python `bltzr` launcher is not suitable; do not weaken the compiled-only requirement.
- Avoid treating FFmpeg as a global startup prerequisite. For text projects, do not check or report media tools. For recordings with supplied subtitles/ASR and no clip generation or re-listening, avoid requiring FFmpeg if no operation calls it. If automatic transcription needs chunk conversion or generated card clips are enabled, validate just before that work and offer the relevant Settings action.
- Provide a clear user-facing option to skip generated clips if that is the simplest way to use an imported transcript without FFmpeg. Default and wording should fit the current beginner-friendly flow.

## Linux and Windows distributions

The repository has `.github/workflows/release-build.yml`, which can manually build CLI and GUI binaries on macOS, Windows, and Ubuntu and upload unsigned artifacts. It does **not** yet produce complete Linux or Windows end-user distributions. `distribution/package-macos.sh` is macOS-specific; `distribution/package-cli.sh` builds a Unix CLI archive only. `README.md` and `planning/implementation-status.md` already disclose installers/signing as release gates.

Implement practical distributable artifacts for Linux and Windows:

- Linux: package the GUI and CLI together in a versioned archive with a desktop entry, icon, launch script if needed, and concise install/uninstall/readme instructions. Provide a suitable portable format such as AppImage only if its dependencies/tooling are available in CI; otherwise create an honest tarball first. Ensure GUI shared-library requirements are documented or bundled appropriately. Include x86-64 first; add ARM64 only if the target and CI can be supported.
- Windows: package GUI and CLI together in a versioned ZIP and provide an installer (for example, WiX/MSI or Inno Setup) that installs the app, Start Menu shortcut, optional PATH entry for CLI, and uninstall support. Keep unsigned status explicit until signing credentials are configured. Avoid requiring Rust, Python, or manual terminal steps for end users.
- Update the release workflow to invoke each platform packager and upload the actual package artifacts, not only raw `.exe`/ELF binaries. Pin third-party packaging actions/tools where practical. Build from CI; the current Mac host cannot locally execute Windows/Linux packaging and acceptance checks.
- Include the native CLI alongside the GUI so Settings can install it without asking users to build it.
- Add a clean-machine smoke checklist and document supported architectures, system dependencies, install/update/uninstall behavior, data locations, and unsigned-package warnings. Do not claim signed or production-ready status before those gates pass.

## Repository and implementation constraints

- The Python reference repository is `/Users/smdi/dev/blitzline-py`; it is read-only and only useful for compatibility planning. The Rust repository is the current working directory.
- There are already many unrelated uncommitted changes from the port. Do not reset, discard, or reformat unrelated work.
- Keep user data in OS app-data directories, never inside the installed application or repository.
- Preserve example/sample behavior as explicitly marked; never silently substitute sample data for a real failed pipeline.
- No subagents unless the user explicitly requests delegation.
- Work autonomously. Run the most relevant checks after code changes when available. Local TCP-based integration tests may need approved escalated execution. Linux and Windows packaging needs CI runners; report that limitation rather than pretending a Mac build validates those targets.

## Suggested completion sequence

1. Implement and document OS-aware tool resolution, Settings pickers, persistent path preferences, and override application for retries.
2. Refactor media dependency checks so FFmpeg is required only at an operation that actually uses it; make “skip generated clips” discoverable and preserve transcript-only flows.
3. Consolidate project prerequisite failures into one persistent alert location and remove duplicate casual error messages.
4. Add Linux and Windows packaging scripts and wire them into the manual release workflow, producing GUI+CLI packages.
5. Run formatting, lint, Rust tests, and local macOS packaging where appropriate; let CI perform actual Linux/Windows package builds. Update README/release status with exactly what is and is not verified.

## Expected handoff result

Deliver a working implementation with reviewable Linux and Windows artifacts generated by CI, consistent tool-error UX, automatic discovery for common install paths, editable tool locations in Settings, and no unnecessary FFmpeg requirement for text or transcript-only projects. Summarize validation and any remaining release blockers, especially compiled Blitzer availability and code signing.
