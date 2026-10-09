# Choose which words to skip

Blitzline asks Blitzer for vocabulary, then turns the result into cards. Use
two optional files to say which words you do not want counted or made into cards.
Each file has one job. You can use either file or both.

## Skip these exact words (word form)

For the verb **to be**, you might know **be** and **am** but still want to
study **is**, **are**, **was**, **were**, **being**, and **been**.

Put this in your exact-word file:

```text
be
am
cat
```

Only **be**, **am**, and **cat** are skipped. **Are** and **cats** can still
be counted. Even though **be** is the basic word, listing it here never skips
its other forms. In Slovenian, listing **sem** and **biti** here leaves **sva**
counted.

## Skip these words and their other forms: word families (lexeme/lemma)

Use the other file when you want to skip an entire word family:

```text
be
cat
```

For **to be**, write **be**, without “to”. The dictionary connects **be**,
**am**, **is**, **are**, **was**, **were**, **being**, and **been**. Listing
**cat** skips both **cat** and **cats**. Related words such as **catlike**
are not part of this grammatical family.

| Put this in a file | Which file | Skipped | Still counted |
|---|---|---|---|
| `be`, `am` | Exact words (word form) | be, am | is, are, was, were, being, been |
| `be` | Word families (lexeme/lemma) | All dictionary-recognized forms of be | Other words |
| `cat` | Exact words (word form) | cat | cats |
| `cat` | Word families (lexeme/lemma) | cat, cats | Other words |

Families depend on the dictionary. Bundled English currently lacks some
**to be** links; list all eight spellings in the exact-word file if you want
to skip them all with that dictionary. A spelling may also have several
meanings: **saw** can be a tool or a form of **see**. Skipping the **see**
family can leave the tool candidate for review.

## Where to select the files

If you already use Blitzer, put the settings in its config,
`~/.config/bltzr/bltzr.toml`. Blitzline reads them automatically through Blitzer:

```toml
[languages.eng]
skip_exact_words_file = "eng-exact-words.txt"
skip_word_families_file = "eng-word-families.txt"

[languages.slv]
skip_exact_words_file = "slv-exact-words.txt"
skip_word_families_file = "slv-word-families.txt"
```

This is the recommended arrangement when you study several languages. Each
language uses its own lists. Paths are relative to the Blitzer config.
An existing config under `$XDG_CONFIG_HOME/bltzr/bltzr.toml` takes priority;
`BLITZER_CONFIG` or Blitzline's `blitzer.config` can select a different file.

For a separate Blitzline setup, override the files in your **Blitzline** config:

```toml
[blitzer]
skip_exact_words_file = "eng-exact-words.txt"
skip_word_families_file = "eng-word-families.txt"
```

These paths are relative to the Blitzline config. Each override replaces only
the corresponding Blitzer file; the other list stays active. These overrides
apply to every language run using that Blitzline config, so use the per-language
Blitzer setup above if you switch languages. Leaving an override out, or setting
it to `""`, inherits Blitzer's selection. `no_config = true` bypasses Blitzer's
config, while explicit file overrides still apply.

Use one word per line, with blank lines and whole-line `#` comments if useful.
Hyphenated entries such as `sally-anne` are allowed. Missing files act as empty
lists. Words are compared using the language's normalization rules, usually
ignoring capital letters. Write basic words in the family file: **be**, rather
than **am**, and **cat**, rather than **cats**.

Use a Blitzer build whose `bltzr blitz --help` lists both skip-file options.
To use your updated source checkout, set `blitzer.executable` to its
`.venv/bin/bltzr` path. Blitzline invokes the external command and does not
install or update Blitzer itself.

## What appears on cards

Blitzline always requests basic words (lexeme/lemma) for card headings. This
does not change either skip list. If **be** and **am** are in the exact-word
list and the story says **are**, that occurrence is still counted and may
produce a card headed **be**. Only the surviving occurrences contribute to
the count and example sentences.

Blitzline reads both lists but never adds words to them. It also disables
Blitzer history saving, even if your Blitzer config enables automatic updates
or saving. Reading a story does not mean you have learned its words.

After editing either list, resume the run to refresh vocabulary, review and
cards. Blitzline notices changes to both directly selected files and files
selected through Blitzer's config, including a previously missing file that
you have now created. It refuses to export old cards until the run is refreshed.

## Moving from `known_file`

Existing Blitzline configs and saved runs using `blitzer.known_file` still
work with Blitzer's legacy filtering rules. To migrate, decide what the words
in that file mean:

- If they mean “skip only these spellings,” use `skip_exact_words_file`.
- If they mean “skip these words and their other forms,” use
  `skip_word_families_file`, writing basic words in that file.

Remove the old `known_file` override when using either new override. In
Blitzer's own config, replace its old file settings too; do not mix `known_file`,
`exclusions`, `forms_only`, or language-level `filter_by` with the new file
settings in one language section. An explicit new file override selects the
new list system for that run instead of reading configured legacy files.
Blitzline never rewrites or migrates your personal files automatically.
