# Anki note formats

New runs default to the `blitzer` preset, based on `ref/example_note.apkg`.
The installed package bundles the preset; the reference archive is not needed
at runtime. Delivery still defaults to TSV; select `--format apkg` for a package
or `--format anki` for AnkiConnect.

The default ordered fields are **Word, Translation, Sentence1, Sentence2,
Comments, Frequency, Sentence1 Audio, Sentence2 Audio, Word Audio, Image,
FillForTwoWay**. Meaning and review notes populate Translation and Comments.
Sentence clips populate the corresponding audio fields, including video clips
when selected. Word Audio and Image are initially empty.

The templates are ordered Recall, Sentence1, Sentence2. Each available sentence
creates a card: its front shows the bolded word and plays the example clip;
clicking the word toggles the full sentence. Its answer shows the full sentence,
word, translation, and optional comments/word audio. Recall asks for the word
from its translation with a typed answer and is disabled by default. The styling
matches the example: centered 35px Avenir Next, faded bold text on desktop, and
light-blue italic bold text on small screens. Missing elements and words without
bold spans are handled, and escaped text stays escaped during the click toggle.

```toml
[export]
format = "apkg"
preset = "blitzer"
model = "Blitzer Basic"
deck = "Blitzline"
two_way = true # Populate FillForTwoWay to generate Recall cards too.
existing = "skip" # Or update existing note fields via AnkiConnect.
tags = ["blitzline", "recording_{source}", "word_{word}"]
# css_file = "anki/style.css" # Relative to this config file.
# update_model = true # Explicitly update matching live templates and CSS.
```

Stable identity and URL-encoded source filename tags are always included in
APKG, AnkiConnect, and TSV. Custom tag placeholders are `{id}`, `{word}`,
`{source}`, and `{language}`; whitespace in these custom tags becomes underscores. Source metadata
can also be mapped into a custom field. The reserved identity/source tags do not
appear on the card unless a template explicitly displays tags.

## Multiple languages and explicit deck profiles

Configure each deck's source language and applicable card types. A single TOML
can contain as many named profiles as needed:

```toml
[export]
preset = "blitzer" # The example note type is the shared default.
format = "apkg"
# deck_profile = "slovenian" # Optional default selection; also a CLI flag.

[decks.slovenian]
language = "slv"
deck = "Languages::Slovenian"
card_types = ["Sentence1", "Sentence2"]

[decks.polish]
language = "pol"
deck = "Languages::Polish"
card_types = ["Recall", "Sentence1", "Sentence2"]
```

Every configured profile must explicitly provide a three-letter Blitzer
`language`, destination `deck`, and nonempty `card_types` list. Card types are
Anki card-template names, such as Recall/Sentence1/Sentence2 from the reference
note type, or names from your custom templates. Their listed order determines
template order. Selecting Recall in a profile enables it automatically, unless
you explicitly set that profile's `two_way = false`. Sentence cards still require
the corresponding example; selecting a template does not fabricate missing data.

```sh
# The selected profile supplies the source language:
blitzline run slovene.mp4 --config config.toml --deck-profile slovenian
blitzline run polish.mp4 --config config.toml --deck-profile polish
# A language can select its single matching configured profile:
blitzline run slovene.mp4 --config config.toml --language slv
# Choose a profile when exporting/resuming an existing run too:
blitzline export RUN_DIR --config config.toml --deck-profile slovenian --format apkg
```

Selecting a profile for the wrong source language fails before processing or
export. If several profiles use the same language, explicitly select one; if
profiles are configured but no profile matches the run language, configure a
matching profile instead of silently falling back to an unrelated deck. One
destination deck cannot be assigned to different source languages. Without deck
profiles, the base export configuration remains available for simple/legacy runs.

Profiles inherit all note-format settings from `[export]`. They can override
`preset`, `model`, fields, templates, CSS, assets, tags, two-way generation, and
AnkiConnect update policy. Use nested tables such as `[[decks.slovenian.fields]]`
and `[[decks.slovenian.templates]]` for customization. File paths still resolve
relative to the TOML file. Transport format is selected globally or by `--format`.
By default, the profile key is appended to the inherited model name, keeping
card selections and styling independent across languages. An explicitly shared
`model` name is allowed only when the profiles define identical note schemas,
templates, and CSS. For existing Anki note types, specify the exact `model` name.

The source language is stored in export metadata and a `blitzline_language_CODE`
tag. Custom fields can map `source = "Language"`, and custom tags can use
`{language}`. Existing runs retain their source language on resume; switching a
destination profile does not reinterpret or translate the recording's language.

## Selecting and modifying card templates

A nonempty `templates` array replaces the preset's template list, in the order
you specify. Matching preset names inherit their front/back when omitted.
`enabled = false` omits an entry entirely. For example, retain just Sentence1:

```toml
[export]
model = "My single-example notes"

[[export.templates]]
name = "Sentence1"
```

Or override the front of a preset template while inheriting its answer:

```toml
[export]
model = "My sentence cards"

[[export.templates]]
name = "Sentence1"
front = "{{Sentence1}}<br>{{Sentence1 Audio}}"

[[export.templates]]
name = "Sentence2"
```

Every template can use inline `front`/`back` or `front_file`/`back_file`, with
file paths relative to the TOML file. Inline HTML can contain JavaScript, Anki
filters such as `type:` and `hint:`, and conditional field sections. Every APKG
front needs a note-field reference for card generation. Unknown
fields and unbalanced sections fail validation before delivery. Both front and
back are required for a new template name. Templates are standard Anki note
cards, rather than cloze or image-occlusion note types.

`css` overrides the complete preset stylesheet, including an empty string;
`css_file` loads it from a file. Choose inline text or a file for each value.
The older `front_template`/`back_template` settings remain available as shorthand
for a single Vocabulary template; choose those or the `templates` array.

## Custom fields, mappings, and assets

A nonempty `fields` array replaces the entire preset schema in the given order.
Each entry needs a unique `name` and exactly one of:

- `source`: a built-in value, already escaped and rendered appropriately.
- `value`: a literal string, escaped by default; `html = true` treats it as
  explicitly authored HTML, such as an image or optional audio reference.
- `format`: authored HTML with `$Word`, `$Sentence1`, `$Media1`, etc. substituted
  from escaped/rendered built-in values. Use `$$` for a literal dollar sign.

The built-in sources are Word, English/Translation, Sentence1, Sentence2,
Notes/Comments, Frequency, Media1/Sentence1 Audio, Media2/Sentence2 Audio, Source,
BlitzlineId, Language, Word Audio, Image, and FillForTwoWay. Word Audio and Image are blank
unless you provide your own mappings/literals. For `format`, use identifier names
like `$Media1`; source names containing spaces are available through `source`.
Generated sentences contain controlled `<b>` highlights and media sources contain
`[sound:filename]` references. The first configured field is the browser sort field.

This complete custom schema demonstrates renamed fields, composed HTML,
metadata, two card directions, and an optional image asset:

```toml
[export]
model = "My vocabulary v1"
assets = ["anki/picture.png"]
css = ".card { font-family: sans-serif; font-size: 24px; }"

[[export.fields]]
name = "Prompt"
format = "<h2>$Word</h2>$Sentence1<br>$Media1"

[[export.fields]]
name = "Answer"
source = "Translation"

[[export.fields]]
name = "Picture"
value = '<img src="picture.png">'
html = true

[[export.fields]]
name = "Recording"
source = "Source"

[[export.fields]]
name = "Identity"
source = "BlitzlineId"

[[export.templates]]
name = "Recognition"
front = "{{Prompt}}"
back = "{{FrontSide}}<hr id=answer>{{Answer}}{{Picture}}"

[[export.templates]]
name = "Production"
front = "{{Answer}}"
back = "{{Prompt}}{{Picture}}"
```

Assets are bundled/copied/uploaded by all exporters using their base filenames.
They must exist, be nonempty, have unique filenames, and avoid collisions with
generated clips. You can include fonts, images, audio, JavaScript, and stylesheets;
use Anki's usual static-media naming conventions for assets referenced only by
templates. No images or separate word pronunciations are synthesized automatically.
Edits to template/CSS files and asset contents invalidate the export cache.

## Delivery and existing collections

All formats use the same resolved fields, templates, and styling. TSV also writes
`note-type.json` and `templates/` with numbered front/back HTML and `style.css`.
Follow `IMPORT.md` to create the matching note type before importing. TSV has one
column per note field plus a tags column identified by its file header. TSV's
normal duplicate matching still uses the first note field. APKG uses stable note
GUIDs; AnkiConnect searches the reserved identity tag, independent of custom
field names, with a fallback for older BlitzlineId fields.

AnkiConnect validates field order before mutation. Existing live templates and
styling are preserved unless `update_model = true`; that option requires the
same template names/order and updates their HTML/CSS. Choose a new model name
when changing a field schema or the template set. `existing = "update"` changes
mapped note fields without rebuilding notes or resetting scheduling. The model
update setting is separate from the note-field update setting.

Use `preset = "legacy"` and `model = "Blitzline v1"` for the previous ten-field,
single-Vocabulary-card format. Saved runs predating presets are upgraded to
legacy settings automatically, including custom old model names. New runs use
the Blitzer preset. To export an old run in the new format, provide a new config
with `preset = "blitzer"` and a distinct model name to `blitzline export`.
