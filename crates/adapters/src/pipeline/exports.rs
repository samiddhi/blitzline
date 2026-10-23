use super::*;
use base64::Engine as _;
use rusqlite::{Connection, params};
use std::collections::BTreeSet;
use zip::{ZipWriter, write::SimpleFileOptions};
fn media_files(rt: &Runtime, cards: &[Card], s: &Value) -> Result<BTreeMap<String, PathBuf>> {
    let mut names = BTreeSet::new();
    let mut files = BTreeMap::new();
    for c in cards {
        if !names.insert(&c.id) || c.contexts.is_empty() || c.contexts.len() > 2 {
            return Err(Error::input(
                "Duplicate note identity or invalid example count",
            ));
        }
        if c.source_kind == "text" {
            if !c.media.is_empty()
                || c.contexts.iter().any(|x| {
                    x.start_ms.is_some()
                        || x.end_ms.is_some()
                        || x.matches.is_empty()
                        || !x.error.is_empty()
                })
            {
                return Err(Error::input(
                    "Invalid text card: expected verified untimed examples without generated media",
                ));
            }
        } else if c.source_kind == "media" {
            if c.media.len() != c.contexts.len()
                || c.contexts
                    .iter()
                    .any(|x| x.start_ms.is_none() || x.end_ms.is_none() || !x.error.is_empty())
            {
                return Err(Error::input("Media card has missing clips or timing"));
            }
            for name in &c.media {
                if !name.starts_with("blitzline-") || name.contains(['/', '\\', '\0', '\r', '\n']) {
                    return Err(Error::input("Invalid generated-media filename"));
                }
                let p = rt.path(&format!("media/{name}"))?;
                if !p.is_file() || p.metadata()?.len() == 0 {
                    return Err(Error::operation(format!("Missing or empty media {name}")));
                }
                files.insert(name.clone(), p);
            }
        } else {
            return Err(Error::input("Invalid card source kind"));
        }
    }
    for path in s["assets"].as_array().into_iter().flatten() {
        let path = PathBuf::from(
            path.as_str()
                .ok_or_else(|| Error::input("Asset path must be text"))?,
        );
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error::input("Invalid asset filename"))?
            .to_owned();
        if name.chars().any(|c| "[]<>\"\r\n\t".contains(c))
            || !path.is_file()
            || path.metadata()?.len() == 0
        {
            return Err(Error::input("Invalid, missing, or empty asset"));
        }
        if let Some(old) = files.get(&name)
            && old.canonicalize()? != path.canonicalize()?
        {
            return Err(Error::input(
                "An asset filename collides with media or another asset",
            ));
        }
        files.insert(name, path);
    }
    Ok(files)
}
pub fn export(rt: &Runtime, cards: &[Card], settings: &Value) -> Result<Vec<String>> {
    let cards = notes::ordered(cards, settings)?;
    let model = notes::definition(settings)?;
    let files = media_files(rt, &cards, settings)?;
    match settings["format"].as_str().unwrap_or("tsv") {
        "tsv" => tsv(rt, &cards, settings, &model, &files),
        "apkg" => package(rt, &cards, settings, &model, &files),
        "anki" => anki(rt, &cards, settings, &model, &files),
        _ => Err(Error::input("Unknown export format")),
    }
}
fn tsv(
    rt: &Runtime,
    cards: &[Card],
    s: &Value,
    model: &Value,
    files: &BTreeMap<String, PathBuf>,
) -> Result<Vec<String>> {
    let names = model["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut writer = csv::WriterBuilder::new()
        .delimiter(b'\t')
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(Vec::new());
    for card in cards {
        let mut fields = notes::render_fields(card, s, model)?
            .into_iter()
            .map(|(_, v)| v)
            .collect::<Vec<_>>();
        fields.push(notes::tags(card, s)?.join(" "));
        writer
            .write_record(fields)
            .map_err(|e| Error::operation(e.to_string()))?;
    }
    let body = writer
        .into_inner()
        .map_err(|e| Error::operation(e.to_string()))?;
    let mut raw = format!(
        "#separator:tab\n#html:true\n#columns:{}\tTags\n#tags column:{}\n",
        names.join("\t"),
        names.len() + 1
    )
    .into_bytes();
    raw.extend(body);
    rt.write("exports/tsv/cards.tsv", &raw)?;
    output.push("exports/tsv/cards.tsv".into());
    let definition = json!({"name":s["model"],"deck":s["deck"],"language":s["language"],"deck_profile":s["deck_profile"],"fields":names,"templates":model["templates"],"css":model["css"],"sort_field":names[0]});
    rt.write(
        "exports/tsv/note-type.json",
        &serde_json::to_vec_pretty(&definition)?,
    )?;
    output.push("exports/tsv/note-type.json".into());
    for (i, t) in model["templates"].as_array().unwrap().iter().enumerate() {
        for side in ["front", "back"] {
            let path = format!("exports/tsv/templates/{}-{side}.html", i + 1);
            rt.write(&path, t[side].as_str().unwrap().as_bytes())?;
            output.push(path);
        }
    }
    rt.write(
        "exports/tsv/templates/style.css",
        model["css"].as_str().unwrap_or("").as_bytes(),
    )?;
    output.push("exports/tsv/templates/style.css".into());
    for (name, path) in files {
        let destination = format!("exports/tsv/media/{name}");
        atomic(&rt.path(&destination)?, &fs::read(path)?)?;
        output.push(destination);
    }
    rt.write("exports/tsv/IMPORT.md",format!("# Import into Anki\n\n{}\nSelect destination deck {} and note type {}.\nCreate its fields in this order: {}.\nThe complete definition is in note-type.json. Create every card template in order using templates/N-front.html and N-back.html, and paste templates/style.css into Styling. Import cards.tsv with HTML enabled. The final column contains tags.\n\nTSV duplicate detection uses the first field, {}. Identical words from different sources may collide. APKG and AnkiConnect use stable source-linked identities instead.\n",if files.is_empty(){"This export has no media files to copy."}else{"Copy the files inside media/ to the Anki profile's collection.media directory."},s["deck"].as_str().unwrap(),s["model"].as_str().unwrap(),names.join(", "),names[0]).as_bytes())?;
    output.push("exports/tsv/IMPORT.md".into());
    Ok(output)
}
const SCHEMA:&str="CREATE TABLE col (id integer primary key,crt integer not null,mod integer not null,scm integer not null,ver integer not null,dty integer not null,usn integer not null,ls integer not null,conf text not null,models text not null,decks text not null,dconf text not null,tags text not null);
CREATE TABLE notes (id integer primary key,guid text not null,mid integer not null,mod integer not null,usn integer not null,tags text not null,flds text not null,sfld integer not null,csum integer not null,flags integer not null,data text not null);
CREATE TABLE cards (id integer primary key,nid integer not null,did integer not null,ord integer not null,mod integer not null,usn integer not null,type integer not null,queue integer not null,due integer not null,ivl integer not null,factor integer not null,reps integer not null,lapses integer not null,left integer not null,odue integer not null,odid integer not null,flags integer not null,data text not null);
CREATE TABLE revlog (id integer primary key,cid integer not null,usn integer not null,ease integer not null,ivl integer not null,lastIvl integer not null,factor integer not null,time integer not null,type integer not null);
CREATE TABLE graves (usn integer not null,oid integer not null,type integer not null);
CREATE INDEX ix_notes_usn ON notes (usn); CREATE INDEX ix_cards_usn ON cards (usn); CREATE INDEX ix_revlog_usn ON revlog (usn); CREATE INDEX ix_cards_nid ON cards (nid); CREATE INDEX ix_cards_sched ON cards (did, queue, due); CREATE INDEX ix_revlog_cid ON revlog (cid); CREATE INDEX ix_notes_csum ON notes (csum);";
fn sql_error(e: rusqlite::Error) -> Error {
    Error::operation(format!("Anki package database: {e}"))
}
fn package(
    rt: &Runtime,
    cards: &[Card],
    s: &Value,
    model: &Value,
    files: &BTreeMap<String, PathBuf>,
) -> Result<Vec<String>> {
    let name = s["model"].as_str().unwrap();
    let mid = notes::model_id(name, model);
    let did = notes::numeric_id(s["deck"].as_str().unwrap());
    let stamp = now() as i64;
    let seconds = stamp / 1000;
    let fields=model["fields"].as_array().unwrap().iter().enumerate().map(|(i,f)|json!({"name":f["name"],"ord":i,"font":"Liberation Sans","media":[],"rtl":false,"size":20,"sticky":false})).collect::<Vec<_>>();
    let field_names = fields
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    let mut templates = Vec::new();
    let mut requirements = Vec::new();
    for (i, t) in model["templates"].as_array().unwrap().iter().enumerate() {
        let front = t["front"].as_str().unwrap();
        let all = field_names
            .iter()
            .map(|n| (n.clone(), "present".into()))
            .collect::<BTreeMap<String, String>>();
        if !notes::template_active(front, &all) {
            return Err(Error::input(
                "Cannot determine APKG card requirements; each front needs a positive note-field reference",
            ));
        }
        let required = field_names
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                let mut f = all.clone();
                f.insert(n.clone(), String::new());
                (!notes::template_active(front, &f)).then_some(i)
            })
            .collect::<Vec<_>>();
        let (kind, indices) = if required.is_empty() {
            (
                "any",
                field_names
                    .iter()
                    .enumerate()
                    .filter_map(|(i, n)| {
                        let f = BTreeMap::from([(n.clone(), "present".into())]);
                        notes::template_active(front, &f).then_some(i)
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            ("all", required)
        };
        if indices.is_empty() {
            return Err(Error::input(
                "This APKG template has unsupported field dependencies",
            ));
        }
        requirements.push(json!([i, kind, indices]));
        templates.push(json!({"name":t["name"],"ord":i,"qfmt":t["front"],"afmt":t["back"],"bqfmt":"","bafmt":"","did":null}));
    }
    let definition = json!({"id":mid.to_string(),"name":name,"type":0,"mod":seconds,"usn":-1,"sortf":0,"did":did,"tmpls":templates,"flds":fields,"css":model["css"],"latexPre":"\\documentclass[12pt]{article}\n\\begin{document}\n","latexPost":"\\end{document}","latexsvg":false,"req":requirements,"tags":[],"vers":[]});
    let deck = |id: i64, name: &str| json!({"id":id,"name":name,"mod":seconds,"usn":-1,"desc":"","dyn":0,"collapsed":false,"conf":1,"extendNew":0,"extendRev":50,"lrnToday":[0,0],"newToday":[0,0],"revToday":[0,0],"timeToday":[0,0]});
    let models = json!({mid.to_string():definition});
    let mut decks = json!({"1":deck(1,"Default")});
    decks[did.to_string()] = deck(did, s["deck"].as_str().unwrap());
    let conf = json!({"activeDecks":[did],"addToCur":true,"collapseTime":1200,"curDeck":did,"curModel":mid.to_string(),"dueCounts":true,"estTimes":true,"newBury":true,"newSpread":0,"nextPos":cards.len()+1,"sortBackwards":false,"sortType":"noteFld","timeLim":0});
    let dconf = json!({"1":{"id":1,"name":"Default","mod":0,"usn":0,"maxTaken":60,"autoplay":true,"timer":0,"replayq":true,"new":{"bury":true,"delays":[1,10],"initialFactor":2500,"ints":[1,4,7],"order":1,"perDay":20,"separate":true},"rev":{"bury":true,"ease4":1.3,"fuzz":0.05,"ivlFct":1,"maxIvl":36500,"minSpace":1,"perDay":100},"lapse":{"delays":[10],"leechAction":0,"leechFails":8,"minInt":1,"mult":0}}});
    let temp = tempfile::NamedTempFile::new_in(&rt.root)?;
    let mut db = Connection::open(temp.path()).map_err(sql_error)?;
    db.execute_batch(SCHEMA).map_err(sql_error)?;
    let tx = db.transaction().map_err(sql_error)?;
    tx.execute(
        "INSERT INTO col VALUES (1,?1,?2,?2,11,0,0,0,?3,?4,?5,?6,'{}')",
        params![
            seconds,
            stamp,
            conf.to_string(),
            models.to_string(),
            decks.to_string(),
            dconf.to_string()
        ],
    )
    .map_err(sql_error)?;
    let mut identity = stamp;
    for (position, c) in cards.iter().enumerate() {
        identity += 1;
        let nid = identity;
        let values = notes::render_fields(c, s, model)?;
        let fields = values
            .iter()
            .map(|(_, v)| v.as_str())
            .collect::<Vec<_>>()
            .join("\u{1f}");
        let sort = values[0].1.clone();
        let checksum = {
            use sha1::Digest as _;
            let hash = format!("{:x}", sha1::Sha1::digest(sort.as_bytes()));
            u32::from_str_radix(&hash[..8], 16).unwrap()
        };
        let tags = notes::tags(c, s)?.join(" ");
        tx.execute(
            "INSERT INTO notes VALUES (?1,?2,?3,?4,-1,?5,?6,?7,?8,0,'')",
            params![
                nid,
                c.id,
                mid,
                seconds,
                format!(" {tags} "),
                fields,
                sort,
                checksum
            ],
        )
        .map_err(sql_error)?;
        let field_map = values.into_iter().collect::<BTreeMap<_, _>>();
        for (ordinal, t) in model["templates"].as_array().unwrap().iter().enumerate() {
            if notes::template_active(t["front"].as_str().unwrap(), &field_map) {
                identity += 1;
                tx.execute(
                    "INSERT INTO cards VALUES (?1,?2,?3,?4,?5,-1,0,0,?6,0,0,0,0,0,0,0,0,'')",
                    params![
                        identity,
                        nid,
                        did,
                        ordinal as i64,
                        seconds,
                        position as i64 + 1
                    ],
                )
                .map_err(sql_error)?;
            }
        }
    }
    tx.commit().map_err(sql_error)?;
    let check: String = db
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(sql_error)?;
    if check != "ok" {
        return Err(Error::operation(
            "Generated Anki database failed verification",
        ));
    }
    db.close().map_err(|(_, e)| sql_error(e))?;
    let path = rt.path("exports/apkg/cards.apkg")?;
    fs::create_dir_all(path.parent().unwrap())?;
    let output = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    {
        let mut zip = ZipWriter::new(output.as_file());
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("collection.anki2", options)
            .map_err(|e| Error::operation(e.to_string()))?;
        std::io::copy(&mut File::open(temp.path())?, &mut zip)?;
        let mut mapping = BTreeMap::new();
        for (i, (name, file)) in files.iter().enumerate() {
            let index = i.to_string();
            zip.start_file(&index, options)
                .map_err(|e| Error::operation(e.to_string()))?;
            std::io::copy(&mut File::open(file)?, &mut zip)?;
            mapping.insert(index, name);
        }
        zip.start_file("media", options)
            .map_err(|e| Error::operation(e.to_string()))?;
        zip.write_all(&serde_json::to_vec(&mapping)?)?;
        zip.finish().map_err(|e| Error::operation(e.to_string()))?;
    }
    {
        let mut archive = zip::ZipArchive::new(File::open(output.path())?)
            .map_err(|e| Error::operation(e.to_string()))?;
        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| Error::operation(e.to_string()))?;
            std::io::copy(&mut file, &mut std::io::sink())?;
        }
    }
    output
        .persist(&path)
        .map_err(|e| Error::operation(e.error.to_string()))?;
    Ok(vec!["exports/apkg/cards.apkg".into()])
}
fn invoke(rt: &Runtime, url: &str, action: &str, params: Value) -> Result<Value> {
    let response = super::providers::post(
        rt,
        url,
        &json!({"action":action,"version":6,"params":params}),
        &BTreeMap::new(),
        30,
        0,
    )?;
    let object = response
        .as_object()
        .ok_or_else(|| Error::operation("Malformed AnkiConnect response"))?;
    if object.len() != 2 || !object.contains_key("result") || !object.contains_key("error") {
        return Err(Error::operation("Malformed AnkiConnect response"));
    }
    if !response["error"].is_null() {
        return Err(Error::operation(format!(
            "AnkiConnect rejected {action}; check Anki and its note-type settings"
        )));
    }
    Ok(response["result"].clone())
}
fn anki(
    rt: &Runtime,
    cards: &[Card],
    s: &Value,
    model: &Value,
    files: &BTreeMap<String, PathBuf>,
) -> Result<Vec<String>> {
    let url = s["anki_url"]
        .as_str()
        .ok_or_else(|| Error::input("Missing AnkiConnect URL"))?;
    let name = s["model"].as_str().unwrap();
    let version = invoke(rt, url, "version", json!({}))?;
    if version.as_u64().is_none_or(|v| v < 6) {
        return Err(Error::operation(
            "AnkiConnect API version 6 or newer is required",
        ));
    }
    let names = model["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].clone())
        .collect::<Vec<_>>();
    let exists = invoke(rt, url, "modelNames", json!({}))?
        .as_array()
        .is_some_and(|a| a.contains(&json!(name)));
    if exists {
        if invoke(rt, url, "modelFieldNames", json!({"modelName":name}))? != json!(names) {
            return Err(Error::operation(
                "The Anki note type has incompatible fields; choose a new model name",
            ));
        }
        if s["update_model"] == true {
            let existing = invoke(rt, url, "modelTemplates", json!({"modelName":name}))?;
            let names = existing
                .as_object()
                .ok_or_else(|| Error::operation("Invalid model templates from Anki"))?
                .keys()
                .collect::<Vec<_>>();
            let expected = model["templates"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap())
                .collect::<Vec<_>>();
            if names.iter().map(|s| s.as_str()).collect::<Vec<_>>() != expected {
                return Err(Error::operation(
                    "Existing Anki card templates differ; choose a new model name",
                ));
            }
        }
    }
    invoke(rt, url, "createDeck", json!({"deck":s["deck"]}))?;
    if !exists {
        invoke(
            rt,
            url,
            "createModel",
            json!({"modelName":name,"inOrderFields":names,"css":model["css"],"isCloze":false,"cardTemplates":model["templates"].as_array().unwrap().iter().map(|t|json!({"Name":t["name"],"Front":t["front"],"Back":t["back"]})).collect::<Vec<_>>()}),
        )?;
    } else if s["update_model"] == true {
        let templates = model["templates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                (
                    t["name"].as_str().unwrap().to_string(),
                    json!({"Front":t["front"],"Back":t["back"]}),
                )
            })
            .collect::<BTreeMap<_, _>>();
        invoke(
            rt,
            url,
            "updateModelTemplates",
            json!({"model":{"name":name,"templates":templates}}),
        )?;
        invoke(
            rt,
            url,
            "updateModelStyling",
            json!({"model":{"name":name,"css":model["css"]}}),
        )?;
    }
    let receipt = "exports/anki/anki-delivery.json";
    let previous: Value = rt
        .read(receipt)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| json!({}));
    let mut state = json!({"endpoint":url,"deck":s["deck"],"model":name,"notes":previous.get("notes").cloned().unwrap_or_else(||json!({})),"state":"running"});
    rt.write(receipt, &serde_json::to_vec_pretty(&state)?)?;
    let result = (|| -> Result<()> {
        for (name, path) in files {
            let v = invoke(
                rt,
                url,
                "storeMediaFile",
                json!({"filename":name,"data":base64::engine::general_purpose::STANDARD.encode(fs::read(path)?)}),
            )?;
            if v != *name {
                return Err(Error::operation(
                    "Anki did not acknowledge an uploaded media file",
                ));
            }
        }
        for c in cards {
            if rt.cancelled() {
                return Err(Error::paused());
            }
            let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
            let tag = format!("blitzline_id_{}", c.id);
            let mut found = invoke(
                rt,
                url,
                "findNotes",
                json!({"query":format!("\"note:{escaped}\" \"tag:{tag}\"")}),
            )?;
            if found.as_array().is_some_and(Vec::is_empty) && names.contains(&json!("BlitzlineId"))
            {
                found = invoke(
                    rt,
                    url,
                    "findNotes",
                    json!({"query":format!("\"note:{escaped}\" \"BlitzlineId:{}\"",c.id)}),
                )?;
            }
            let ids = found
                .as_array()
                .ok_or_else(|| Error::operation("Anki returned invalid note identities"))?;
            if ids.len() > 1 {
                return Err(Error::operation(format!(
                    "Multiple Anki notes share identity {}",
                    c.id
                )));
            }
            let fields = notes::render_fields(c, s, model)?
                .into_iter()
                .map(|(k, v)| (k, Value::String(v)))
                .collect::<serde_json::Map<_, _>>();
            let outcome = if let Some(id) = ids.first() {
                if s["existing"] == "update" {
                    invoke(
                        rt,
                        url,
                        "updateNoteFields",
                        json!({"note":{"id":id,"fields":fields}}),
                    )?;
                    json!({"note_id":id,"action":"updated"})
                } else {
                    json!({"note_id":id,"action":"skipped"})
                }
            } else {
                let mut tags = notes::tags(c, s)?;
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
                let id = invoke(
                    rt,
                    url,
                    "addNote",
                    json!({"note":{"deckName":s["deck"],"modelName":name,"fields":fields,"tags":tags,"options":{"allowDuplicate":true}}}),
                )?;
                if id.as_i64().is_none() {
                    return Err(Error::operation(
                        "Anki returned no new note ID; resume will look up the stable identity before trying again",
                    ));
                }
                json!({"note_id":id,"action":"added"})
            };
            state["notes"][&c.id] = outcome;
            rt.write(receipt, &serde_json::to_vec_pretty(&state)?)?;
        }
        Ok(())
    })();
    state["state"] = json!(if result.is_ok() { "complete" } else { "failed" });
    rt.write(receipt, &serde_json::to_vec_pretty(&state)?)?;
    result?;
    Ok(vec![receipt.into()])
}
