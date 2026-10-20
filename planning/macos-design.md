# macOS design revision

Requested 2026-10-20: real macOS chrome, a clean and spacious visual design,
restrained color, consistent controls, and a readily available light/dark switch.
This revision supersedes the initial prototype's painted toolbar and green palette.

## Toolkit decision

Keep the current shared Rust application and eframe content renderer for this
prototype, with a small **AppKit shell written in Rust using objc2**. The main
window uses a genuine `NSToolbar`, SF Symbols, and native menu items. AppKit
controls their rendering, overflow, customization, tooltips, and target/action
dispatch. There is no Swift, Objective-C source, browser frontend, or Python
runtime dependency.

“Written in Rust” does not mean replacing Apple's operating-system frameworks:
AppKit is the native macOS UI framework. The Rust bindings call it directly.
The workspace remains cross-platform; the AppKit module is macOS-only.
The prototype bundle declares macOS 11 or later for unified-toolbar and
system-symbol APIs. Release platform support still needs testing.

Options considered:

| Option | Fit for this requirement |
| --- | --- |
| eframe plus AppKit through objc2 | Supports a genuine macOS toolbar on the existing window. Preserves working prototype workflows. Selected for this revision. |
| Direct AppKit through objc2 for all desktop controls | Strongest route to system widgets throughout the macOS app, with substantially more platform-specific view code. Keep available if custom content fails macOS interaction/accessibility acceptance. |
| Xilem/Masonry | Rust-native compilation and custom rendering using winit/Vello; switching alone would not give us AppKit widgets. |
| Slint | Cross-platform renderer with styled widgets. Its native-looking styles are being deprecated; this does not establish a better route to an actual AppKit toolbar. |

Sources consulted: [objc2 NSToolbar bindings](https://docs.rs/objc2-app-kit/0.3.2/objc2_app_kit/struct.NSToolbar.html),
[Apple NSToolbar](https://developer.apple.com/documentation/appkit/nstoolbar),
[Xilem architecture](https://github.com/linebender/xilem),
[Slint style direction](https://slint.dev/blog/default-native-style-change).
The integration was also checked against the locally installed eframe 0.33.3
window-handle implementation and objc2-app-kit 0.3.2 bindings.

## Window and menu structure

- Use the system titlebar, traffic lights, title, and unified toolbar.
- Toolbar: Projects, New project, flexible spacing, Appearance, Settings, Help.
  Icons have descriptive tooltips and accessibility descriptions. AppKit supports
  toolbar customization; essential actions remain reachable through menus.
- Application menu: Settings (Command-comma), plus the existing system
  About/Services/Hide/Quit entries.
- File: New Project (Command-N), Choose Files (Command-O), Import Python Settings.
- View: Projects, Switch Light / Dark, Larger Text, Smaller Text.
- Help: Blitzline Help.
- Non-macOS builds and headless interaction tests retain an in-content toolbar.
- Secondary windows retain system titlebars; they do not duplicate the main
  window's toolbar.

The native delegate queues actions; the normal application update applies them.
It never calls a processing service from an AppKit callback. Native objects are
retained while installed; menu items and toolbar targets are detached before the
delegate is released.

## Visual system

Use the installed macOS system font when available, with the bundled renderer
font as fallback. Apple fonts are read from the user's system, never packaged.

Light mode uses near-white neutral backgrounds, white content surfaces, fine
cool-gray borders, pale blue selection, and restrained blue primary actions.
Dark mode uses neutral slate surfaces, subtle borders, and lighter blue accents.
Avoid the previous green tint, heavy outlines, and oversized buttons.

Body and button text default to 15 points, headings to 26. Standard controls
are approximately 32–34 points high with consistent padding and rounded corners.
Text scaling remains in Settings and the View menu. Content has 28-point outer
insets; cards have 22-point padding and consistent available widths. Navigation
uses equal-height rows. Browser assistance switches between columns and a
vertical sequence according to available space.

The toolbar appearance button switches between light and dark immediately,
including AppKit appearance. Settings also offers System, Light, and Dark.
The preference is saved with the existing application preferences.

### Contrast and utility-window refinements

Help, Project Settings, and browser assistance open as separate native utility
windows. The project contains a compact “Open chat assistant” entry point.
Closing the assistant retains its reply; Project Settings remains bound to the
original project when the main window switches projects. Cancel or window-close
discards uncommitted project-setting changes.

Inputs use a thin visible outline, 12-point horizontal padding, and 9–10-point
vertical padding. Radio controls have visible rings; independent checkboxes
retain square corners. Slider tracks have an accent-filled portion and circular,
outlined handles, with a readable numeric value.

Settings uses explicit navigation buttons with selected states and chevrons,
switching to a category picker in narrow layouts. Headings and fine dividers
separate groups. Setup help choices are outlined selectable rows with labels
and descriptions aligned together. Run summaries and secondary context use
muted italic text; interactive labels remain upright.

## Acceptance and limits

Validate both themes on Projects, Setup, browser assistance, Cards, and Export.
Exercise native toolbar target/action routing as well as existing engine and
accessible-control tests. Verify copy/paste, partial replies, saved progress,
and discoverable advanced settings remain available.

`distribution/native-visual-check.sh` captures only the app window, including
native chrome, after startup has settled. It requires macOS Screen Recording
permission. The earlier eframe screenshot hook captures too early after toolbar
geometry changes and can produce a black image; it is not used for macOS
visual acceptance. The generic visual script delegates to the native script
on macOS.

This is a hybrid interface, not an all-AppKit widget implementation. Content
text fields, lists, cards, dialogs rendered within egui, and the stage sidebar
remain custom-rendered. Full VoiceOver, macOS text editing conventions, menu
coverage, focus behavior, reduced motion, and other-platform testing remain
production acceptance work. If those checks fail, move affected views to
AppKit through Rust bindings rather than accepting a cosmetic approximation.
