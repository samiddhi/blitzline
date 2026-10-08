"""Export Anki-importable TSV together with its media files.

Scope statement: own portable text delivery and import instructions.
Included: tab-delimited writing, literal empty fields, round-trip verification,
media copying, and a short import guide.
Excluded: building cards (vocabulary.py), cutting clips (media.py), other export
formats (package.py/anki.py), and direct collection database changes.
Start here: export_tsv creates cards.tsv and a media subdirectory.
"""

import csv
import io
import shutil
from pathlib import Path

from blitzline.exports.common import FIELDS, fields, media_paths
from blitzline.storage import atomic_text


def export_tsv(cards, media_dir: Path, destination: Path, settings: dict) -> list[Path]:
    """Write UTF-8 TSV and copy only referenced media, preserving empty columns."""
    paths = media_paths(cards, media_dir)
    destination.mkdir(parents=True, exist_ok=True)
    media = destination / "media"
    media.mkdir(exist_ok=True)
    copied = []
    for source in paths:
        target = media / source.name
        shutil.copy2(source, target)
        copied.append(target)
    rows = [list(fields(card).values()) for card in cards]
    stream = io.StringIO(newline="")
    writer = csv.writer(stream, delimiter="\t", lineterminator="\n")
    writer.writerows(rows)
    raw = stream.getvalue()
    if list(csv.reader(io.StringIO(raw), delimiter="\t")) != rows:
        raise ValueError("TSV round-trip verification failed")
    path = destination / "cards.tsv"
    atomic_text(
        path, "#separator:tab\n#html:true\n#columns:" + "\t".join(FIELDS) + "\n" + raw
    )
    guide = destination / "IMPORT.md"
    atomic_text(
        guide,
        "# Import into Anki\n\nCopy the files inside media/ to your Anki profile's collection.media directory.\n"
        "Import cards.tsv as tab-separated text with HTML enabled. Create a note type\n"
        "with these fields in order: " + ", ".join(FIELDS) + ". Map all ten columns.\n"
        "Use Word/Sentence1/Media1 on the front and English/Sentence2/Media2/Notes on the back.\n\n"
        "TSV duplicate detection normally uses the note type's first field (Word).\n"
        "Choose import duplicate settings deliberately; identical words from different\n"
        "recordings can collide. APKG and AnkiConnect instead use stable source-linked IDs.\n",
    )
    return [path, guide, *copied]
