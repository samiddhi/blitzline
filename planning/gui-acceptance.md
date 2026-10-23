> Native pipeline update (2026-10-21): normal projects now use the real Rust
> pipeline through a background worker. The historical prototype checklist below
> describes the design baseline. Current capabilities and unresolved production
> work are recorded in [implementation-status.md](implementation-status.md).

# GUI prototype acceptance and implementation status

Specification: [full GUI plan](gui.md).

This is a Rust/egui desktop skeleton with a shared simulated engine and CLI.
egui/eframe is provisional; production acceptance still requires real
assistive-technology, playback, and rendering validation.

## macOS redesign, 2026-10-20

See the [design revision and toolkit comparison](macos-design.md).

- Real AppKit unified toolbar, SF Symbols, native application/File/View/Help
  menus, and toolbar customization, all integrated from Rust.
- Neutral light/dark themes, a toolbar appearance switch, system typography,
  equal-height controls, consistent navigation rows, and wider content insets.
- 26 automated tests passed, including appearance changes during an unfinished
  browser reply; formatting and Clippy with warnings denied passed.
- Native visual checks exercise all five toolbar actions through AppKit
  target/action dispatch and capture five screens in each theme.
- Content controls remain egui-rendered. These checks do not constitute full
  VoiceOver or cross-platform production acceptance.

## Implemented surfaces

The contrast/utility-window refinement adds native Help and Project Settings
windows and moves browser exchanges into their own window. Settings navigation,
sliders, input padding/outlines, radio/checkbox distinction, and setup choice
rows were refined. Run metadata is muted and italic.

Automated coverage now totals 28 tests. New interaction checks verify that a
project-settings window cannot accidentally update a newly selected project
and that closing/reopening the assistant preserves its unfinished reply.
Native captures cover Help, Project Settings, Settings, browser assistance,
help choices, setup inputs, and the final run summary in both themes.

| Surface | Implementation |
| --- | --- |
| Projects | Empty state, example, filters/search, archive/restore, delete |
| Setup | Four steps, text/files, separate projects, language, method choice, optional stages |
| Workspace | Stages, status, pause/resume/retry, settings, activity |
| Source/text | Reference, edits/replacement, downstream-impact confirmation |
| Translation | Optional stage; API simulation or browser/manual exchange |
| Chat assistant | Copy/open/paste/check, partial repair, samples, smaller batches, method switching |
| Compact assistant | Separate window, pinned project, optional keep-on-top |
| Words | Filters, meaning/notes, include/exclude/pending, managed-list actions |
| Cards | Front/back, previous/next, edit link, preview gate |
| Export | Simulated APKG receipt, unavailable-Anki error, real sample TSV, history |
| Settings | Search, advanced sections, session-only key, appearance/scale |
| Profiles | Language/deck/card defaults, managed spelling/family lists |
| Resources | Explicit simulated installation/removal and progress |
| Card design | Separate window, front/back/CSS drafts, safe text preview |
| Help | Searchable explanations/glossary |
| Persistence | Atomic saves, exclusive writer, corruption protection, paused restart |

## Scenario evidence

| Journey | Evidence / remaining check |
| --- | --- |
| Text → browser → two batches → preview → export | Engine and accessible-control tests |
| Recording needs model | Failure/install/retry test; simulated download UI |
| Incomplete reply | Engine and accessible-control tests preserve accepted items |
| Restart mid-exchange | Serialization/restart and disk roundtrip tests |
| Switch after chat limit | Accepted answers retained by method-switch test |
| Per-stage methods | Optional text-stage tests and controls; real connections pending |
| Queue | Waiting-project progression test |
| Text change | Revision/invalidation/export-history test |
| Anki unavailable | Explicit error with simulated APKG/sample TSV alternatives |
| Config import | Profile retention, unsupported report, unchanged input, secret-exclusion test |
| Keyboard/accessibility | AccessKit control tests; full VoiceOver/NVDA/Orca evaluation pending |
| Narrow/large text | 1024×768 control test; scale control; full platform inspection pending |

Tests live in `crates/engine/tests`, `crates/adapters/tests`, and the desktop
library. They never contact external services.

### Local verification, 2026-10-20

- 25 automated tests passed: 15 engine workflows, 5 storage/import tests, and
  5 desktop/accessibility-control tests.
- Workspace build, formatting, and Clippy with warnings denied passed.
- The native macOS app was launched and screenshots of Projects, Setup,
  Browser assistance, Study cards, and Export were visually inspected.
- Visual inspection led to a two-column assistant on wider windows and
  stronger dark-mode text contrast. Narrow layouts retain vertical scrolling.
- The local `.app` bundle was built; its property list is valid and the
  executable links only macOS system libraries/frameworks.
- Screenshots are generated artifacts in `target/previews/`, reproducible with
  `sh distribution/visual-check.sh` in an authorized desktop session.

## Deliberate prototype limits

- Source processing, extraction, downloads, APIs, and delivery are simulated.
- Media controls animate a position but play no audio/video.
- APKG creates no package; only sample TSV writes actual study data.
- Keys are session-only; secure credential persistence is not connected.
- Import applies deck/language profiles only, with an explicit report.
- Full timing editing, proofreading diffs, context alternatives, and shared-form
  batching belong to real integration.
- Custom fields/assets and HTML/JavaScript rendering remain pending. The current
  design editor never executes authored code or applies templates to exports.
- Some advanced preferences are stored for design exploration but not executed;
  those controls explain this in the UI.
- Persistence uses one locked prototype workspace. Production per-job folders,
  concurrent read-only views, verified source relinking, and resource catalogs
  remain port milestones.
- Native menu parity, complete undo, notifications, runtime estimates, and
  screen-reader status announcements need production validation.
- Only macOS is locally available. Windows/Linux builds and installers require
  verification on those platforms.

## Review procedure

1. Choose **Try an example**, then continue to Words.
2. Expand sample replies and submit an incomplete answer. Finish with a
   follow-up and verify accepted items were retained.
3. Finish the second request, prepare cards, show answers, and export.
4. Try manual review and automatic processing in separate projects.
5. Explore the compact assistant, settings search, larger text, and card editor.
6. Quit with a pasted draft and reopen.
7. Correct source text and inspect the rebuild explanation.

Use `--data-dir` for isolated experiments. Record issues before committing to
the production toolkit or connecting services. Product gates in `gui.md` remain.
