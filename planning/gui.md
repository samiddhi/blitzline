# Blitzline desktop GUI: complete structural plan

Recorded 2026-10-20. This is the full interaction specification. Implementation
progress and acceptance evidence are tracked in [GUI acceptance](gui-acceptance.md).
The specification describes the intended product; a prototype must label its
simulated capabilities rather than imply that the pipeline has been ported.

The [macOS design revision](macos-design.md) supersedes the initial toolbar
and palette choices with AppKit chrome, system typography, and neutral themes.

## 1. Experience and design principles

Blitzline should feel like a calm desktop utility that helps someone turn a
recording or document into study cards. Users should always be able to answer:

- What am I working on?
- What has Blitzline finished?
- What does it need from me?
- What happens when I press the main button?
- Can I leave and return later?

The agreed direction is:

- Guided setup followed by a persistent project workspace.
- Equal prominence for browser-chat assistance, automatic API processing, and
  manual review.
- Separate queued projects for multiple input files.
- Card preview before export by default.
- Restrained visual design, readable typography, familiar controls.
- Framework selection deferred until the experience is approved.

Use **Project** in the interface for the engine's saved job. Prefer “Words,”
“Examples,” “Study cards,” and “Needs your attention” over pipeline terminology.

Avoid a global beginner/expert mode. Give each screen a simple default view
with clearly labeled **Advanced settings**. Search must find advanced settings
and reveal their location.

The first release uses English for meanings and optional translations, matching
the existing pipeline. Choosing a source language must not imply arbitrary
translation-language support.

## 2. Windows, navigation, and toolbars

### Window inventory

| Window or surface | Purpose and behavior |
| --- | --- |
| Main window | Projects, guided setup, active project, languages/decks, tools/downloads. One primary application window. |
| Settings window | Searchable preferences and reusable connections. Preserves the current project and position. |
| Browser-chat assistant | Separate resizable window opened from the current stage, convenient beside a browser. Shares saved project state. |
| Help | Separate searchable utility window; closing it preserves the current workflow. |
| Project settings | Separate utility window pinned to the project opened; Apply commits and Cancel discards its draft. |
| Card design editor | Dedicated advanced window for fields, templates, styling, assets, and preview. |
| Short dialogs/sheets | File selection, naming, locating missing files, applying consequential changes, export destinations, cleanup. |

Long tasks, review sessions, and provider setup must not live in small modal
dialogs. Avoid stacked dialogs. The compact assistant is resizable; “Keep above
other windows” is optional and off by default. Closing it does not cancel or
discard an exchange.

### Main application shell

At the top: **New project**, a search field appropriate to the current area, and
**Help**.

Main navigation:

- Projects
- Languages & decks
- Tools & downloads
- Settings, positioned at the bottom

Opening a project replaces navigation content with its stages and a prominent
**Back to projects** link. Avoid two competing sidebars.

Project header:

- Editable project name.
- Language and destination deck.
- Text status: Processing, Waiting for your reply, Ready to export, etc.
- Project settings.
- Overflow menu: rename, duplicate setup, reveal folder, archive, cleanup.

Each screen has a short local toolbar. Put the primary next action consistently
at the bottom of the task area. Never put destructive actions in that position.
A slim activity area shows background work; detailed logs remain collapsed.

### Project navigation

| Stage | Purpose |
| --- | --- |
| Source | Original file/text, language, source details, transcription choice |
| Transcript or Text | Inspect/correct source text; optional proofreading |
| English translation | Optional translated subtitles/text; marked Off when disabled |
| Words | Vocabulary, meanings, examples, review decisions |
| Study cards | Clips, rendered cards, omissions, final preview |
| Export | Create files or deliver to Anki; previous exports |

Each stage has text and an icon for not started, processing, needs input,
complete, skipped, or needs attention. Completed work remains inspectable.
Opening a stage never reruns it. Unavailable stages explain their prerequisite.
Technical substages appear under **Processing details**.

### Menus and shortcuts

- File: New project, open existing project folder, import settings, export, close.
- Edit: Undo, redo, cut, copy, paste, find.
- View: Projects, text size, processing details, compact assistant.
- Project: Continue, pause, settings, retry failed step.
- Help: Getting started, browser-chat walkthrough, troubleshooting, diagnostics,
  about.

Use familiar platform shortcuts for New, Open, Find, Undo, Settings, and text
scaling. Playback shortcuts apply only outside text inputs. Menus and context
menus supplement visible controls; essential actions cannot require right-click
or a keyboard shortcut.

## 3. Screens and everyday journeys

### First launch

Show “Turn recordings and text into vocabulary cards” with:

- Start a project
- Try an example
- Import existing Blitzline settings

Do not require an account, key, model selection, or configuring every language.
The clearly labeled example uses bundled sample data and simulated replies,
without sending data or spending credits. Request downloads/provider setup only
when needed by the selected project.

### Projects home

Show name/source type, language/deck, last activity, status, and one appropriate
action: Continue, Open, Review, or Export. Filters: All, Needs attention,
Processing, Finished; include search and sorting.

Give input-requiring projects prominence without rearranging rows during
interaction. Empty state: Choose a file and Paste text, with supported formats.

Distinguish Archive (hide from everyday list), Clean up (remove eligible working
files after export), and Delete project (remove managed data). Never delete the
original source file.

### Guided new-project setup

Four steps, Back/Continue, and a saved-draft indicator.

#### Step 1: Choose your material

Offer audio/video, text file, and pasted text. Support drag-and-drop alongside
buttons. Show filename, duration/text length, and type. Pasted text gets an
editable title and becomes project-owned source material.

“Already have subtitles?” imports SRT or transcription JSON alongside a
recording.

Multiple files create separate projects with shared settings and per-file
language corrections. They retain independent review, recovery, and export.
Explain that sources are not merged into one transcript or package.

Unsupported formats receive a precise explanation and an alternative such as
copying text. Never treat PDF as plain text.

#### Step 2: Language and destination

Ask for source language and study deck. Existing profiles are selectable; a new
profile needs a language and friendly deck name. Technical identifiers stay
hidden. Show language-resource availability and explicit downloads with size
and purpose. Inferred language is a visible suggestion.

Text skips transcription. Recordings offer:

- On this computer: no transcription API key; may need a model.
- Online service: requires configured account/key.
- Use supplied subtitles: when imported.

Suggest local transcription initially. Never silently switch to paid online
processing when local resources are missing.

#### Step 3: Choose your help

Three equally prominent choices:

| Choice | Explanation |
| --- | --- |
| Use a chat website | Copy prepared requests to a browser and paste replies back; no API key |
| Connect an AI service | Blitzline sends requests automatically using a connection |
| Review it myself | Enter meanings and choose examples directly |

The first project requires a choice; subsequent projects may reuse a preference.
Explain that browser assistance can use services offering free access, but their
availability and usage limits still apply. Never promise unlimited free use.

Show vocabulary review, optional proofreading (off initially), and optional
English translation (off initially). Advanced controls choose methods and
connections separately per stage, ambiguity checks, and audit.

Estimate browser exchanges when possible; label preliminary counts as estimates
because vocabulary extraction can change them.

#### Step 4: Check and start

Summarize source/language, deck/card style, transcription, AI assistance,
optional stages, downloads, externally transmitted content, and possible service
charges. Use sentence cards by default, optional recall, audio examples for
recordings, and explicit video selection.

Primary action: Start project or Add projects to queue. Inline remedies for
missing prerequisites must preserve completed setup.

### Overview and processing

Show current activity, completed work, next user action, Pause, processing
details, and saved-progress status. Use measured counts when available; otherwise
show activity without invented percentages or time estimates.

Default to one processing project at a time. Waiting projects release the queue
for other explicitly queued work without stealing focus. Advanced concurrency
can be added in Processing settings.

Reference: [GNOME progress guidance](https://developer.gnome.org/hig/patterns/feedback/progress-bars.html).

### Transcript/text

Recordings have play/pause, seek, volume, speed, time, and a searchable transcript.
Clicking a segment seeks to it. Selection exposes its text and timing.
Text has a readable searchable document without empty audio controls.

Toolbar: Find, Edit text, Import replacement, Proofread, advanced timing.
Ordinary edits preserve timing; advanced segment editing adjusts timestamps.
Corrected text discards obsolete word alignment.

Stage edits with Apply changes / Discard changes. Show downstream impact before
applying. Existing exports remain identifiable as earlier versions.
Proofreading compares original/proposal with Accept / Keep original. Group
uncertain passages under Needs your attention.

### English translation

Show aligned original/translation views with search, edits, browser continuation
or API retry, and subtitle/text download. Blitzline owns timing. Disabled state
explains the optional result and offers Enable translation.

### Words

Use list/detail layout. List columns: word, meaning, source frequency, decision.
Filters: Needs attention, Included, Excluded, All.

Details: editable meaning, one/two selected highlighted examples, alternative
supplied contexts, recording playback, notes, Include / Exclude / Decide later.
Label frequency as source count rather than semantically corrected frequency.

Browser/API results populate the same review state. Bulk actions cannot bypass
missing meanings or invalid contexts.

Exclude from this project affects only this project. Add to words I know is a
separate explicit action choosing exact spelling or family and future-use list.
Never silently change shared Blitzer data.

### Study cards

Summarize included words, card count, unresolved items, and media. Distinguish
words from cards because one word can produce several cards.

Card browser: front/back, previous/next, playback, card-type selection, and link
back to the word editor. Standard controls cover directions, media, and order;
advanced customization opens the card design editor.

Default primary action: Continue to export. Preview is a normal pause.
Experienced users may explicitly enable automatic export when valid. Preview
does not guarantee identical behavior in every Anki client.

### Export

Offer Save an Anki deck file (.apkg, default), Send to Anki (connection/setup),
and Export a table and media (advanced TSV).

Show destination, deck, word/card totals, media, and exclusions. Distinguish
note-content updates from template updates. Preserve schedules; incompatible
schemas require resolution.

Afterward: Open deck file, Show in folder, Export another way, export history.
Only confirm Anki delivery after acknowledgment. File export success does not
mean the user imported it.

Cleanup stays secondary, previews retained/removed files, and is never implied
by automatic export.

## 4. Browser-chat assistance

### Guided exchange

Display a compact entry point within its stage and open the guided exchange in
its own native window. Keep the project and stage explicit in that window.
Example header: “Review vocabulary — request 2 of 6. Your progress is saved.
You can come back later.”

**1. Copy the request**

- Copy request button.
- Confirmation: “Copied. Paste this into your chat website.”
- Expandable readable See what you're sending.
- Advanced raw-prompt view.

**2. Send it in your browser**

- Open chat website, if selected.
- Choose or change website.
- Start a new conversation for each independent request.
- Explain copying the complete finished response.

Opening a website never sends content automatically. Any suitable chat can be
used; Blitzline never collects browser-account credentials.

**3. Bring the answer back**

- Large Paste the chat's answer here input.
- Standard paste and Paste from clipboard.
- Check answer.

Never poll/read the clipboard on focus.

### Successful response

Show a plain-language result such as “Answer checked. 18 words are ready; 2 need
another look.” Explain accepted/unresolved work and offer Continue to the next
request or Review the remaining words. Validation checks structure and coverage,
not perfect linguistic correctness. Preserve accepted work.

### Incomplete/malformed response

Explain the issue without a parser exception: “The answer is missing three
words. The other answers have been saved.”

Offer Copy a follow-up request, Paste a replacement answer, or Finish these
items myself. Follow-ups contain the evidence needed and say whether to use the
same conversation; ordinary batches start fresh.

Never require JSON punctuation repair or identifying internal IDs. Preserve an
unparseable response for replacement/inspection; never silently select an
arbitrary object from conflicting answers.

### Limits and interruption

“The chat says this is too long” prepares smaller valid batches. Never split
candidate groups needing joint review. If impossible, offer manual review or
another method.

At service limits: save/stop, change website, return later, review manually, or
switch remaining work to an API. Keep accepted work; paid continuation is
explicit.

Closing either window preserves prompt, pasted draft, response status, and
position. Reject stale replies with an explanation. Resubmission must not
duplicate cards or overwrite newer decisions.

### Separate assistant window

Contains project/stage, request number, Copy, Open website, Paste, Check/Continue,
and Back to project. Shares state with the full workspace. Changing projects
in the main window does not retarget an open assistant.

## 5. Settings, resources, and advanced features

### Languages & decks

Searchable profiles contain friendly name, language, deck, card types/preset,
known-word lists, assistance preferences, and transcription preferences.
Profile changes affect future projects; existing ones explicitly Apply profile
changes.

Explain exact spellings versus dictionary-connected word families with examples.
Managed lists support import, search, add/remove, export. External lists expose
their origin; editing requires an explicit choice, with copying to a managed
list offered first.

### Settings window

| Category | Standard | Advanced |
| --- | --- | --- |
| General & appearance | Theme, text size, folders | Density, startup |
| AI connections | Name, provider, model, masked key, test | Endpoint, environment reference, response mode, timeouts, retries, reasoning/routing |
| Browser assistance | Website, walkthrough | Request limits, prompt inspection |
| Transcription | Local/online, model, resource status | Device/options, chunks, language overrides, re-listening |
| Words & review | Assistance, known-word behavior | Batches/groups, ambiguity, audit, dictionary evidence |
| Cards & export | Preset, directions, media, format | Fields, templates, assets, ordering, Anki updates |
| Processing & storage | Locations, queue, storage use | Executables, caches, timeouts, concurrency |
| Import & diagnostics | Python import, readiness | Redacted logs, capabilities, configuration export |

Explain API key inline. Saving a key never silently runs a paid request.
Billable testing is labeled “Run a small test — may use credits.”

Project controls show Using profile default or Changed for this project, with
Reset to default and Save as a reusable profile. Overrides do not alter others.

### Tools & downloads

Show languages, models, and bundled tools with purpose, version, size,
installation progress, retry/remove, and dependent projects when relevant.
Downloads can be postponed. Failure preserves setup.

### Card design editor

Dedicated advanced window: preset/note identity, ordered fields/mappings,
templates, front/back editors, styling, assets, and sample/project preview.
Basic choices stay outside. Provide Apply, Cancel, Reset and consequences for
note identity or media regeneration.

Custom HTML/JavaScript preview is isolated from application privileges.
Renderer selection follows toolkit selection.

### Help

Brief contextual guidance plus Getting started, browser chat, API connections,
transcription, Anki import, common problems, and a searchable glossary.
Help opens in its own native window and preserves the user's position.

## 6. State, recovery, accessibility, and contracts

### Recovery

| Situation | Response |
| --- | --- |
| Moved source | Locate file and verify |
| Offline | Preserve progress; retry/change method |
| Rejected key | Open relevant connection |
| Service limit | Wait, switch, or manual review |
| Invalid pasted answer | Keep valid items; prepared repair |
| No unknown words | Successful empty result; filtering explanation |
| Unresolved words | Filtered Words view |
| Clip failure | Affected examples and repair |
| Anki unavailable | Reconnect or save file |
| Changed input/cache | Explain required rebuild |
| Another process owns job | Read-only view with refresh/retry |
| Disk full | Preserve committed work; explain space needed |

Navigation does not pause. Explicit Pause stops scheduling; explain any
operation still finishing. Quit with active work offers Pause and quit / Keep
working. Platform window-close behavior must clearly communicate continuation.
No hidden background daemon in v1.

After crash/relaunch, restore projects and drafts without automatically
restarting paid work.

### Accessibility and visual defaults

- Comfortable spacing, approximately 16-point-equivalent body text, important
  controls around 44 logical pixels high.
- System/light/dark appearance, text scaling, reduced motion.
- Visible focus, logical keyboard traversal, accessible names, non-color state.
- Reflow at large text sizes rather than clipping.
- Essential actions reachable at 1024×768 logical size; collapse split views.
- Accessible list/detail alternative to tables.
- No automatic media playback.
- Announce important status changes without moving focus.
- Errors persist until resolved; transient notifications are supplementary.

References:
[W3C target sizes](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html)
and [status messages](https://www.w3.org/WAI/WCAG22/Understanding/status-messages.html).
Toolkit prototypes must demonstrate these capabilities.

### Application contract

Typed operations cover projects/queue/state, saved setup/navigation drafts,
profile/default overrides, transcript edits with impact previews, review and
exchange submission, user-initiated clipboard operations, resource installation,
export readiness/receipts, and cleanup previews.

UI preferences such as window position/tab/expanded sections do not invalidate
processing. Domain validation belongs in the engine, not only the UI.

## 7. Prototype and acceptance

Build against a simulated engine before connecting real services. Navigation and
persistent mock state must work; static screenshots alone are insufficient.

Required journeys:

1. Pasted text → browser help → two exchanges → cards → export.
2. Recording requires model download → local transcription.
3. Incomplete answer → retained accepted items → successful repair.
4. Close mid-exchange → reopen at the same point.
5. Chat limit → other website or manual review.
6. Different API connections by stage plus advanced settings.
7. Multiple files → separate queued projects; waiting releases execution.
8. Transcript change → impact explanation → rebuild.
9. Anki unavailable → APKG alternative.
10. Python import reports unsupported settings without losing valid settings.
11. Keyboard-only and screen-reader completion.
12. Large text and narrow window preserve essential actions.

Novices must complete browser assistance without a terminal, writing prompts,
editing JSON, or understanding APIs. Experts must find advanced settings through
local sections or Settings search.

Toolkit and visual prototype decisions follow this structure. Live pipeline
integration follows approval of the skeleton.
