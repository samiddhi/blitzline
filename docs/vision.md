# Blitzline

Blitzline is a versatile pipeline using the `bltzr` CLI to turn audio or video in a foreign language into an Anki deck. Configuring an LLM API (DeepSeek is strongly recommended for this for reasons stated below) will enable this process to be fully automated, with the user simply selecting audio/video and receiving on the other end, a subtitle file, an English translation of that subtitle, and either an Anki-ready tsv file or even a full deck delivered directly to Anki. The process is as follows:

1. Selected video/audio file is passed to whisperx or similar and a JSON output is recieved.
2. JSON is converted to an SRT file manually (not using whisper's built-in)
3. The SRT is optionally sent to an LLM for proofreading, able to "re-listen" to questionable lines and correct evident mistakes.
4. From the final SRT, a temporary plaintext file without timestamps is created and an English SRT is optionally translated by an LLM.
  - DeepSeek is strongly recommended for this translation work — not because it is a good translator, but because it does not merely summarize and preserves timestamps loyally.
5. The temporary plaintext file is passed to the `bltzr` command-line tool and a wordlist is generated.
  - Mostly according to either the user's default config (or else according to a `blitzline` specific user config which can be passed to the CLI) but with flags `--freq`, `--lemmatize`, `--context`, and `--bold html` mandatory.
6. An LLM goes through the word list, removes false lemma matches, and returns a TSV file. See below for a sample prompt. This prompt still fails sometimes in the TSV tabbing, so this could be improved (or alternatively we could migrate away from TSV to something more stable. I actually would advocate for this instead of TSV)
7. This TSV file is verified to have the correct TAB situation or else the problem lines are repaired by the API LLM. — actually if we choose a more stable format this step may not be necessary. 
8. The Verified TSV (or whatever we choose) file is used to take audio (or video — make this a configurable option) clips from the original audio/video file. See `ref/tsvaudio` for a very loose look at how this has been implemented by past tooling to get a sense of what I mean.
9. Use these audio/video clips as media when generating the Anki cards. Options for how to actually get this into anki can very from simply making a tsv to generating a deck file to actually putting it into the user's anki file manually. Make multiple options and make them configurable as to which the user prefers.

## Notes:
- The user may configure different LLMs for different steps of processing if he so desires.
- A very high level user-friendly document should be put together tha guides the configuration of this program. This should be extremely concise, easy to read, and not too technical.

## Step 6: `bltzr` output word list processing prompt
This is the prompt that was used for manual tsv extraction by copy and pasting to an online AI chatbot. It can be modified as is seen fit for direct API usage. 

```You are a renowned language educator with expertise in word form lemmatization semantics, cross-language syntactical relations including cognates and false cognates, and identifying lemmatization errors both by students and by machines in a wide array of languages with particular expertise in Slovenian, Polish, and Pali. 
You are a renowned language educator with expertise in word form lemmatization semantics, cross-language syntactical relations including cognates and false cognates, and identifying lemmatization errors both by students and by machines in a wide array of languages with particular expertise in the language of the provided text. 

You are given a machine generated list of lemmas generated from a transcript. Words known to the student have been automatically filtered out in the process, with a list of lemmas unknown the student remaining. Each lemma is displayed inline with its frequency in the original transcript. Obviously, due to the unavoidable nature of certain words being valid wordforms of multiple lemmas, many of the provided lemmas here flag the same sentence[s] in the text. Your job is to determine which lemma is the correct one when proceeding to collate the output document, letting all other listed lemmas which DO NOT ACTUALLY APPEAR in the text be EXCLUDED from the output. DO NOT INCLUDE PROPER NAMES IN THE OUTPUT.

Your output will be ONLY a tab-separated TSV document inside a fenced code block (```tsv … ```).  Do not write anything else – no introductory text, no closing remarks.

The TSV must contain exactly these six columns, in this exact order:

1. Word
2. English translation (ONLY the meaning(s) that apply in the given sentences)
3. Sentence1 (FULL, verbatim transcript sentence, with the target word wrapped in <b>…</b>)
4. Sentence2 (another transcript sentence containing the word, formatted the same way; LEAVE CELL EMPTY if the word appears only once)
5. Notes (Cognates and false friends only; leave empty if none)
6. Frequency (the integer from the original list)

Critical rules (follow them precisely, or the machine pipeline will break):

- No extra columns.  If a field is empty, leave an empty cell (two consecutive tabs).
- Sentences must be taken exactly from the transcript – do not modify punctuation, capitalisation, or words, except to preserve the <b> tags around the target word.
- NEVER truncate sentences.  Do not use ellipsis (…).
- When multiple grammatical variants appear, keep only the "dictionary form" and drop the others.  Never include duplicates.
- If a lemma is a false lemmatisation (the wordform belongs to another lemma), exclude it silently – do not mention it in the notes.
- The notes field is for the student — never include processing details or mistakes.

Before writing the final TSV, perform these checks silently:

- Ensure that any cells with no data (e.g. a lemma with no sentence 2) exist in the output as blanks. E.g. under no circumstances should notes or frequency integers appear before their respective column.
- For every word you output, confirm that the bolded form really appears in the transcript sentence(s) you provide.
- Review the full list of input lemmas; if any valid lemma is missing from your output, verify that it was either a duplicate, a proper name, or a false lemmatisation – never omit a real unknown word.
- Ensure that the sentences have remained UNCHANGED from the original input. No shortening, changing, or abbreviation of any sort. This is essential for the next step of the processing pipeline.

DOUBLE CHECK THAT YOU HAVE THE CORRECT TABS IN PLACE FOR BLANK CELLS!!! 

Now process the following input and return only the TSV inside a code block:
```
