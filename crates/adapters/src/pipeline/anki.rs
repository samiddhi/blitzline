//! Known words taken live from an Anki collection: every word in the chosen fields of any note
//! that has at least one card that is not new.
use super::*;
use std::collections::BTreeSet;

/// Copies the collection first: Anki keeps it open (WAL mode), and a read-only connection to the
/// live file would need to write the shared-memory file next to it.
pub fn known_words(collection: &Path, deck: &str, fields: &[String]) -> Result<BTreeSet<String>> {
    if !collection.is_file() {
        return Err(Error::input(
            "The Anki collection file was not found; check the path in the profile",
        ));
    }
    let copy = tempfile::tempdir()?;
    let local = copy.path().join("collection.anki2");
    fs::copy(collection, &local)?;
    let wal = PathBuf::from(format!("{}-wal", collection.display()));
    if wal.is_file() {
        fs::copy(&wal, copy.path().join("collection.anki2-wal"))?;
    }
    let fail = |_| Error::operation("Could not read the Anki collection");
    let db = rusqlite::Connection::open(&local).map_err(fail)?;
    // Anki sorts note-type and field names with its own collation.
    db.create_collation("unicase", |a, b| a.to_lowercase().cmp(&b.to_lowercase()))
        .map_err(fail)?;
    let mut statement = db
        .prepare(
            "select n.mid, n.flds from notes n where exists (
                select 1 from cards c join decks d on d.id = case when c.odid != 0 then c.odid else c.did end
                where c.nid = n.id and c.type != 0 and (d.name = ?1 or d.name like ?1 || char(31) || '%'))",
        )
        .map_err(fail)?;
    let mut positions: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    let mut words = BTreeSet::new();
    let mut rows = statement.query([deck]).map_err(fail)?;
    while let Some(row) = rows.next().map_err(fail)? {
        let mid: i64 = row.get(0).map_err(fail)?;
        if !positions.contains_key(&mid) {
            let mut named = db
                .prepare("select name from fields where ntid = ?1 order by ord")
                .map_err(fail)?;
            let names = named
                .query_map([mid], |r| r.get::<_, String>(0))
                .map_err(fail)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(fail)?;
            let wanted = names
                .iter()
                .enumerate()
                .filter(|(_, n)| fields.iter().any(|f| f.eq_ignore_ascii_case(n)))
                .map(|(i, _)| i)
                .collect();
            positions.insert(mid, wanted);
        }
        let flds: String = row.get(1).map_err(fail)?;
        let parts: Vec<&str> = flds.split('\x1f').collect();
        for &i in &positions[&mid] {
            if let Some(text) = parts.get(i) {
                words.extend(tokens(text));
            }
        }
    }
    if words.is_empty() {
        return Err(Error::input(
            "No known words were found in Anki; check the deck name and field names in the profile",
        ));
    }
    Ok(words)
}

/// Words of one field with markup, `[sound:...]` references and punctuation removed.
pub fn tokens(field: &str) -> Vec<String> {
    let mut text = String::new();
    let (mut tag, mut sound) = (false, false);
    for c in field.chars() {
        match c {
            '<' if !sound => tag = true,
            '>' if tag => {
                tag = false;
                text.push(' ');
            }
            '[' if !tag => sound = true,
            ']' if sound => {
                sound = false;
                text.push(' ');
            }
            _ if tag || sound => {}
            _ => text.push(c),
        }
    }
    text.replace("&nbsp;", " ")
        .split(|c: char| !(c.is_alphabetic() || c == '-'))
        .map(|w| w.trim_matches('-').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_audio_and_punctuation_are_removed() {
        assert_eq!(
            tokens("<b>Dober</b> dan,&nbsp;Žan! [sound:a b.mp3] sally-anne"),
            ["dober", "dan", "žan", "sally-anne"]
        );
    }

    #[test]
    fn only_seen_cards_of_the_deck_and_its_subdecks_count() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.anki2");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(
            "create table notes(id integer primary key, mid integer, flds text);
             create table cards(id integer primary key, nid integer, did integer, odid integer, type integer);
             create table decks(id integer primary key, name text);
             create table fields(ntid integer, ord integer, name text);
             insert into fields values (1,0,'Slovenian'),(1,1,'English'),(1,2,'SentenceSL');
             insert into decks values (1,'starter'),(2,'starter'||char(31)||'Grammar'),(3,'other'),(4,'Filtered');
             insert into notes values
               (1,1,'voda'||char(31)||'water'||char(31)||'Pijem <b>vodo</b>.'),
               (2,1,'nova'||char(31)||'new'||char(31)||'Nova hiša.'),
               (3,1,'drugo'||char(31)||'other'||char(31)||'Drugi dan.'),
               (4,1,'sub'||char(31)||'sub'||char(31)||'Podmapa.'),
               (5,1,'filtrirano'||char(31)||'filtered'||char(31)||'');
             insert into cards values
               (1,1,1,0,2),(2,2,1,0,0),(3,3,3,0,2),(4,4,2,0,1),(5,5,4,1,2);",
        )
        .unwrap();
        drop(db);
        let words =
            known_words(&path, "starter", &["slovenian".into(), "SentenceSL".into()]).unwrap();
        let words: Vec<_> = words.iter().map(String::as_str).collect();
        assert_eq!(
            words,
            ["filtrirano", "pijem", "podmapa", "sub", "vodo", "voda"]
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        );
        assert!(known_words(&path, "missing", &["Slovenian".into()]).is_err());
    }
}
