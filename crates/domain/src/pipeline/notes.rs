use super::*;
use regex::Regex;
use serde_json::json;
use std::collections::BTreeSet;
pub const LEGACY_FIELDS: [&str; 10] = [
    "Word",
    "English",
    "Sentence1",
    "Sentence2",
    "Notes",
    "Frequency",
    "Media1",
    "Media2",
    "Source",
    "BlitzlineId",
];
pub fn escaped(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
        .replace("\r\n", "\n")
        .replace('\n', "<br>")
        .replace('\t', "&#9;")
}
pub fn definition(s: &Value) -> Result<Value> {
    let mut model = match s["preset"].as_str().unwrap_or("blitzer") {
        "blitzer" => {
            let mut m: Value =
                serde_json::from_str(include_str!("../../../../assets/presets/blitzer.json"))?;
            m["fields"] = json!(
                m["fields"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| json!({"name":n,"source":n}))
                    .collect::<Vec<_>>()
            );
            m
        }
        "legacy" => {
            json!({"fields":LEGACY_FIELDS.iter().map(|n|json!({"name":n,"source":n})).collect::<Vec<_>>(),"templates":[{"name":"Vocabulary","front":"{{Word}}<br>{{Sentence1}}<br>{{Media1}}","back":"{{FrontSide}}<hr id=answer>{{English}}<br>{{Sentence2}}<br>{{Media2}}<br>{{Notes}}<br><small>{{Frequency}} occurrences · {{Source}}</small>"}],"css":".card { font-family: sans-serif; font-size: 22px; text-align: left; } b { color: #3972bb; } small { font-size: 12px; }"})
        }
        _ => return Err(Error::input("export.preset must be blitzer or legacy")),
    };
    let preset = model["templates"].clone();
    for key in ["fields", "templates"] {
        if s[key].as_array().is_some_and(|a| !a.is_empty()) {
            model[key] = s[key].clone();
        }
    }
    if s["front_template"].is_string() || s["back_template"].is_string() {
        if s["templates"].as_array().is_some_and(|v| !v.is_empty()) {
            return Err(Error::input(
                "Use templates or front_template/back_template, not both",
            ));
        }
        model["templates"] = json!([{"name":"Vocabulary","front":s.get("front_template").filter(|v|v.is_string()).unwrap_or(&model["templates"][0]["front"]),"back":s.get("back_template").filter(|v|v.is_string()).unwrap_or(&model["templates"][0]["back"])}]);
    }
    let mut names = BTreeSet::new();
    for f in model["fields"]
        .as_array()
        .ok_or_else(|| Error::input("Fields must be an array"))?
    {
        let n = f["name"]
            .as_str()
            .ok_or_else(|| Error::input("Field needs a name"))?;
        nonblank(n, "Field name")?;
        if n.chars().any(|c| "{}:\t\r\n".contains(c))
            || n.starts_with(['#', '^', '/'])
            || [
                "FrontSide",
                "Tags",
                "Type",
                "Deck",
                "Subdeck",
                "Card",
                "CardFlag",
            ]
            .contains(&n)
            || !names.insert(vocabulary::fold(n))
        {
            return Err(Error::input("Invalid, reserved, or duplicate field name"));
        }
        if ["source", "value", "format"]
            .iter()
            .filter(|&&k| f.get(k).is_some())
            .count()
            != 1
        {
            return Err(Error::input(
                "Each field requires exactly one source, value, or format",
            ));
        }
        if let Some(source) = f.get("source") {
            let n = source
                .as_str()
                .ok_or_else(|| Error::input("Invalid field source"))?;
            if !LEGACY_FIELDS.contains(&n)
                && ![
                    "Translation",
                    "Comments",
                    "Sentence1 Audio",
                    "Sentence2 Audio",
                    "Word Audio",
                    "Image",
                    "FillForTwoWay",
                    "Language",
                ]
                .contains(&n)
            {
                return Err(Error::input(format!("Unknown field source {n}")));
            }
        }
        if f.get("html")
            .is_some_and(|v| !v.is_boolean() || f.get("value").is_none())
        {
            return Err(Error::input(
                "html is a boolean for literal field values only",
            ));
        }
        if f.as_object()
            .unwrap()
            .keys()
            .any(|k| !["name", "source", "value", "format", "html"].contains(&k.as_str()))
        {
            return Err(Error::input("Unknown field setting"));
        }
    }
    let field_names = model["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let mut templates = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in model["templates"]
        .as_array()
        .ok_or_else(|| Error::input("Templates must be an array"))?
    {
        if entry.get("enabled") == Some(&Value::Bool(false)) {
            continue;
        }
        let n = entry["name"]
            .as_str()
            .ok_or_else(|| Error::input("Template needs a name"))?;
        nonblank(n, "Template name")?;
        if !seen.insert(n) {
            return Err(Error::input("Duplicate template name"));
        }
        let mut t = json!({"name":n});
        for side in ["front", "back"] {
            let v = entry
                .get(side)
                .filter(|v| v.is_string())
                .or_else(|| {
                    preset
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|p| p["name"] == n)
                        .and_then(|p| p.get(side))
                })
                .ok_or_else(|| Error::input(format!("Template {n} needs {side}")))?;
            let raw = v.as_str().unwrap();
            nonblank(raw, "Template")?;
            validate_template(raw, &field_names)?;
            t[side] = v.clone();
        }
        templates.push(t);
    }
    if let Some(selected) = s["card_types"].as_array().filter(|a| !a.is_empty()) {
        let mut ordered = Vec::new();
        let mut selected_seen = BTreeSet::new();
        for n in selected {
            if !selected_seen.insert(
                n.as_str()
                    .ok_or_else(|| Error::input("Invalid card type"))?,
            ) {
                return Err(Error::input("Duplicate card type"));
            }
            ordered.push(
                templates
                    .iter()
                    .find(|t| t["name"] == *n)
                    .ok_or_else(|| Error::input("Unknown or disabled card type"))?
                    .clone(),
            );
        }
        templates = ordered;
    }
    if templates.is_empty() {
        return Err(Error::input("At least one enabled template is required"));
    }
    model["templates"] = json!(templates);
    if s["css"].is_string() {
        model["css"] = s["css"].clone();
    }
    model["sort_field"] = json!(0);
    Ok(model)
}
pub fn validate_template(s: &str, names: &[String]) -> Result<()> {
    let re = Regex::new(r"\{\{\s*([^{}]+?)\s*\}\}").unwrap();
    let mut stack = Vec::new();
    for c in re.captures_iter(s) {
        let token = c[1].trim();
        let name = token
            .trim_start_matches(['#', '^', '/'])
            .split(':')
            .next_back()
            .unwrap()
            .trim();
        if !names.iter().any(|n| n == name)
            && ![
                "FrontSide",
                "Tags",
                "Type",
                "Deck",
                "Subdeck",
                "Card",
                "CardFlag",
            ]
            .contains(&name)
        {
            return Err(Error::input(format!("Unknown template field {name}")));
        }
        if token.starts_with(['#', '^']) {
            stack.push(name.to_owned());
        } else if token.starts_with('/') && stack.pop().as_deref() != Some(name) {
            return Err(Error::input("Unbalanced template conditional"));
        }
    }
    if !stack.is_empty() {
        return Err(Error::input("Unclosed template conditional"));
    }
    Ok(())
}
pub fn numeric_id<T: Serialize + ?Sized>(v: &T) -> i64 {
    (u32::from_str_radix(&digest(v)[..8], 16).unwrap() as i64) % 2_147_483_647 + 1
}
pub fn model_id(name: &str, model: &Value) -> i64 {
    let names = model["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].clone())
        .collect::<Vec<_>>();
    let templates = model["templates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].clone())
        .collect::<Vec<_>>();
    let mut identity = json!([name, names]);
    if names != json!(LEGACY_FIELDS).as_array().unwrap().to_vec()
        || templates != vec![json!("Vocabulary")]
    {
        identity.as_array_mut().unwrap().push(json!(templates));
    }
    numeric_id(&identity)
}
pub fn render_fields(card: &Card, s: &Value, model: &Value) -> Result<Vec<(String, String)>> {
    let max = s["max_sentence_words"].as_u64().unwrap_or(25) as usize;
    let contexts = card
        .contexts
        .iter()
        .map(|c| vocabulary::shorten(c, max))
        .collect::<Vec<_>>();
    let sentence = |i: usize| {
        contexts
            .get(i)
            .map(|c| {
                format!(
                    "{}<b>{}</b>{}",
                    escaped(&text::slice(&c.text, 0, c.highlight_start)),
                    escaped(&text::slice(&c.text, c.highlight_start, c.highlight_end)),
                    escaped(&text::slice(
                        &c.text,
                        c.highlight_end,
                        c.text.chars().count()
                    ))
                )
            })
            .unwrap_or_default()
    };
    let media = |i: usize| {
        card.media
            .get(i)
            .map(|n| format!("[sound:{n}]"))
            .unwrap_or_default()
    };
    let mut data: BTreeMap<String, String> = LEGACY_FIELDS
        .iter()
        .map(|v| v.to_string())
        .zip([
            escaped(&card.word),
            escaped(&card.english),
            sentence(0),
            sentence(1),
            escaped(&card.notes),
            card.frequency.to_string(),
            media(0),
            media(1),
            escaped(&card.source),
            card.id.clone(),
        ])
        .collect();
    for (alias, source) in [
        ("Translation", "English"),
        ("Comments", "Notes"),
        ("Sentence1 Audio", "Media1"),
        ("Sentence2 Audio", "Media2"),
    ] {
        data.insert(alias.into(), data[source].clone());
    }
    for (k, v) in [
        ("Word Audio", String::new()),
        ("Image", String::new()),
        (
            "FillForTwoWay",
            if s["two_way"].as_bool().unwrap_or(false) {
                "1".into()
            } else {
                String::new()
            },
        ),
        ("Language", escaped(s["language"].as_str().unwrap_or(""))),
    ] {
        data.insert(k.into(), v);
    }
    let re = Regex::new(r"\$\$|\$\{([^}]+)\}|\$([_a-zA-Z][_a-zA-Z0-9]*)").unwrap();
    let mut result = Vec::new();
    for f in model["fields"].as_array().unwrap() {
        let name = f["name"].as_str().unwrap().to_owned();
        let value = if let Some(source) = f["source"].as_str() {
            data.get(source)
                .ok_or_else(|| Error::input("Unknown field source"))?
                .clone()
        } else if let Some(fmt) = f["format"].as_str() {
            let mut error = false;
            let value = re
                .replace_all(fmt, |c: &regex::Captures<'_>| {
                    if &c[0] == "$$" {
                        "$".into()
                    } else {
                        let k = c.get(1).or_else(|| c.get(2)).unwrap().as_str();
                        data.get(k).cloned().unwrap_or_else(|| {
                            error = true;
                            String::new()
                        })
                    }
                })
                .to_string();
            if error {
                return Err(Error::input("Unknown placeholder in field format"));
            }
            value
        } else {
            let v = f["value"]
                .as_str()
                .ok_or_else(|| Error::input("Field value must be text"))?;
            if f["html"].as_bool().unwrap_or(false) {
                v.into()
            } else {
                escaped(v)
            }
        };
        result.push((name, value));
    }
    Ok(result)
}
pub fn tags(card: &Card, s: &Value) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for tag in s["tags"].as_array().into_iter().flatten() {
        let mut tag = tag
            .as_str()
            .ok_or_else(|| Error::input("Tag must be text"))?
            .to_string();
        for (k, v) in [
            ("id", card.id.as_str()),
            ("word", card.word.as_str()),
            ("source", card.source.as_str()),
            ("language", s["language"].as_str().unwrap_or("")),
        ] {
            tag = tag.replace(&format!("{{{k}}}"), v);
        }
        if tag.contains(['{', '}']) {
            return Err(Error::input(
                "Tags support only {id}, {word}, {source}, and {language}",
            ));
        }
        let tag = tag.split_whitespace().collect::<Vec<_>>().join("_");
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    Ok(out)
}
pub fn ordered(cards: &[Card], s: &Value) -> Result<Vec<Card>> {
    let mut cards = cards.to_vec();
    let rules = s["order_by"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![json!("chronological")]);
    for r in &rules {
        if ![
            "chronological",
            "reverse_chronological",
            "frequency",
            "word",
        ]
        .contains(&r.as_str().unwrap_or(""))
        {
            return Err(Error::input("Invalid export ordering"));
        }
    }
    let first = |c: &Card| {
        c.contexts
            .iter()
            .map(|x| {
                if c.source_kind == "text" {
                    *x.matches.first().unwrap_or(&0) as u64
                } else {
                    x.start_ms.unwrap_or(0)
                }
            })
            .min()
            .unwrap_or(0)
    };
    cards.sort_by(|a, b| {
        for rule in &rules {
            let order = match rule.as_str().unwrap() {
                "word" => vocabulary::fold(&a.word).cmp(&vocabulary::fold(&b.word)),
                "frequency" => b.frequency.cmp(&a.frequency),
                "reverse_chronological" => first(b).cmp(&first(a)),
                _ => first(a).cmp(&first(b)),
            };
            if !order.is_eq() {
                return order;
            }
        }
        std::cmp::Ordering::Equal
    });
    Ok(cards)
}
/// Evaluate field dependencies, ignoring static HTML/JS when deciding card presence.
pub fn template_active(s: &str, fields: &BTreeMap<String, String>) -> bool {
    let re = Regex::new(r"\{\{\s*([^{}]+?)\s*\}\}").unwrap();
    let mut stack = vec![true];
    let mut active = false;
    for c in re.captures_iter(s) {
        let t = c[1].trim();
        let n = t
            .trim_start_matches(['#', '^', '/'])
            .split(':')
            .next_back()
            .unwrap()
            .trim();
        let filled = fields.get(n).is_some_and(|v| !v.trim().is_empty());
        if t.starts_with('#') {
            stack.push(*stack.last().unwrap() && filled);
        } else if t.starts_with('^') {
            stack.push(*stack.last().unwrap() && !filled);
        } else if t.starts_with('/') {
            if stack.len() > 1 {
                stack.pop();
            }
        } else if *stack.last().unwrap() && filled {
            active = true;
        }
    }
    active
}
