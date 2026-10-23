use super::text::{plaintext, slice, word_spans};
use super::*;
use serde_json::json;
use std::collections::BTreeSet;
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;
pub fn fold(s: &str) -> String {
    s.nfc().case_fold().collect()
}
pub fn map_candidates(rows: &Value, cues: &[Cue]) -> Result<Vec<Candidate>> {
    let rows = rows
        .as_array()
        .ok_or_else(|| Error::input("Blitzer JSON must be an array"))?;
    let (source, spans) = plaintext(cues);
    let chars = source.chars().collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for row in rows {
        let term = row["term"]
            .as_str()
            .ok_or_else(|| Error::input("Blitzer entry requires a term"))?;
        nonblank(term, "Blitzer term")?;
        if !seen.insert(term.nfc().collect::<String>()) {
            return Err(Error::input(format!("Duplicate Blitzer term: {term}")));
        }
        let count = row["count"]
            .as_u64()
            .filter(|v| *v > 0)
            .ok_or_else(|| Error::input("Blitzer count must be a positive integer"))?;
        let id = format!("candidate-{}", &digest(term)[..16]);
        let mut contexts = Vec::new();
        for (i, r) in row["contexts"]
            .as_array()
            .ok_or_else(|| Error::input("Blitzer contexts must be an array"))?
            .iter()
            .enumerate()
        {
            let t = r["text"]
                .as_str()
                .ok_or_else(|| Error::input("Missing context text"))?;
            nonblank(t, "Context")?;
            let left = r["highlight_start"]
                .as_u64()
                .ok_or_else(|| Error::input("Invalid highlight start"))?
                as usize;
            let right = r["highlight_end"]
                .as_u64()
                .ok_or_else(|| Error::input("Invalid highlight end"))?
                as usize;
            let n = t.chars().count();
            if left >= right || right > n {
                return Err(Error::input("Blitzer highlight is outside its context"));
            }
            let needle = t.chars().collect::<Vec<_>>();
            let matches = if chars.len() < n {
                vec![]
            } else {
                (0..=chars.len() - n)
                    .filter(|&p| {
                        chars[p..p + n] == needle[..]
                            && (p == 0
                                || !(chars[p - 1].is_alphanumeric() && needle[0].is_alphanumeric()))
                            && (p + n == chars.len()
                                || !(chars[p + n].is_alphanumeric()
                                    && needle[n - 1].is_alphanumeric()))
                    })
                    .collect::<Vec<_>>()
            };
            let mut c = Context {
                id: format!("{id}-context-{}", i + 1),
                text: t.into(),
                highlight_start: left,
                highlight_end: right,
                cue_ids: vec![],
                start_ms: Some(0),
                end_ms: Some(0),
                matches: matches.clone(),
                error: String::new(),
            };
            if let Some(&p) = matches.first() {
                let selected = spans
                    .iter()
                    .filter(|s| s.start < p + n && s.end > p)
                    .collect::<Vec<_>>();
                let mut intervals = Vec::new();
                for span in selected {
                    let cue = cues.iter().find(|c| c.id == span.cue_id).unwrap();
                    c.cue_ids.push(cue.id.clone());
                    let positions = word_spans(&text::normalize(&cue.text), &cue.words);
                    let words = cue
                        .words
                        .iter()
                        .zip(positions)
                        .filter(|(_, (s, e))| span.start + s < p + n && span.start + e > p)
                        .map(|(w, _)| w)
                        .collect::<Vec<_>>();
                    intervals.push(if let Some(w) = words.first() {
                        (Some(w.start_ms), Some(words.last().unwrap().end_ms))
                    } else {
                        (cue.start_ms, cue.end_ms)
                    });
                }
                c.start_ms = intervals.iter().filter_map(|x| x.0).min();
                c.end_ms = intervals.iter().filter_map(|x| x.1).max();
            } else {
                c.error = "No exact transcript match".into();
            }
            contexts.push(c);
        }
        out.push(Candidate {
            id,
            term: term.into(),
            source_frequency: count,
            contexts,
        });
    }
    Ok(out)
}
pub fn validate_decision(d: &Decision, c: &Candidate) -> Result<()> {
    if d.candidate_id != c.id {
        return Err(Error::input("Decision belongs to another candidate"));
    }
    for s in [&d.reason, &d.english, &d.notes] {
        if s.contains('\0') {
            return Err(Error::input("Decision contains a NUL character"));
        }
    }
    if !["keep", "reject", "needs-review"].contains(&d.decision.as_str()) {
        return Err(Error::input(
            "Decision must be keep, reject, or needs-review",
        ));
    }
    let unique = d.context_ids.iter().collect::<BTreeSet<_>>();
    if d.context_ids.len() > 2
        || unique.len() != d.context_ids.len()
        || d.context_ids
            .iter()
            .any(|id| !c.contexts.iter().any(|v| &v.id == id))
    {
        return Err(Error::input(
            "Select up to two distinct contexts owned by this candidate",
        ));
    }
    if d.decision == "keep" {
        nonblank(&d.english, "English meaning")?;
        if d.context_ids.is_empty() {
            return Err(Error::input("Kept candidates need at least one context"));
        }
        if c.contexts
            .iter()
            .any(|v| unique.contains(&v.id) && !v.error.is_empty())
        {
            return Err(Error::input(
                "Selected context has an unresolved source match",
            ));
        }
    } else {
        nonblank(&d.reason, "Rejected/unresolved reason")?;
    }
    Ok(())
}
pub fn validate_decisions(value: &Value, candidates: &[Candidate]) -> (Vec<Decision>, Vec<Value>) {
    let mut valid = Vec::new();
    let mut errors = Vec::new();
    let Some(rows) = value
        .as_object()
        .filter(|v| v.len() == 1)
        .and_then(|v| v.get("decisions"))
        .and_then(Value::as_array)
    else {
        return (valid,candidates.iter().map(|c|json!({"candidate_id":c.id,"error":"Response must contain only a decisions array"})).collect());
    };
    if rows.iter().any(|r| {
        !candidates
            .iter()
            .any(|c| r["candidate_id"].as_str() == Some(&c.id))
    }) {
        return (
            valid,
            vec![
                json!({"candidate_id":"","error":"Unknown/malformed candidate ID; no changes were applied"}),
            ],
        );
    }
    for c in candidates {
        let found = rows
            .iter()
            .filter(|r| r["candidate_id"].as_str() == Some(&c.id))
            .collect::<Vec<_>>();
        if found.len() != 1 {
            errors.push(json!({"candidate_id":c.id,"error":"Expected exactly one decision"}));
            continue;
        }
        match serde_json::from_value::<Decision>(found[0].clone())
            .map_err(Error::from)
            .and_then(|d| validate_decision(&d, c).map(|()| d))
        {
            Ok(d) => valid.push(d),
            Err(e) => errors.push(json!({"candidate_id":c.id,"error":e.message})),
        }
    }
    (valid, errors)
}
pub fn note_id(source_hash: &str, language: &str, term: &str, kind: &str) -> String {
    let term = term.nfc().collect::<String>();
    let values = if kind == "text" {
        vec!["text", source_hash, language, &term]
    } else {
        vec![source_hash, language, &term]
    };
    format!("blitzline-{}", &digest(&values)[..32])
}
pub fn build_cards(
    decisions: &BTreeMap<String, Decision>,
    candidates: &[Candidate],
    request: &Request,
) -> Vec<Card> {
    candidates
        .iter()
        .filter_map(|c| {
            decisions
                .get(&c.id)
                .filter(|d| d.decision == "keep")
                .map(|d| Card {
                    id: note_id(
                        &request.source_hash,
                        &request.language,
                        &c.term,
                        &request.source_kind,
                    ),
                    word: c.term.clone(),
                    english: d.english.clone(),
                    notes: d.notes.clone(),
                    frequency: c.source_frequency,
                    contexts: d
                        .context_ids
                        .iter()
                        .filter_map(|id| c.contexts.iter().find(|ctx| &ctx.id == id).cloned())
                        .collect(),
                    source: request
                        .source
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or(&request.source)
                        .into(),
                    media: vec![],
                    source_kind: request.source_kind.clone(),
                })
        })
        .collect()
}
pub fn review_template(candidates: &[Candidate]) -> Value {
    json!({"decisions":candidates.iter().map(|c|Decision{candidate_id:c.id.clone(),decision:"needs-review".into(),reason:"Awaiting review".into(),english:String::new(),context_ids:vec![],notes:String::new()}).collect::<Vec<_>>()})
}
pub fn object_schema(properties: Value) -> Value {
    let required = properties.as_object().unwrap().keys().collect::<Vec<_>>();
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
pub fn review_schema() -> Value {
    object_schema(
        json!({"decisions":{"type":"array","items":object_schema(json!({"candidate_id":{"type":"string"},"decision":{"type":"string","enum":["keep","reject","needs-review"]},"reason":{"type":"string"},"english":{"type":"string"},"context_ids":{"type":"array","items":{"type":"string"},"maxItems":2},"notes":{"type":"string"}}))}}),
    )
}
pub fn groups(
    candidates: &[Candidate],
    forms: &BTreeMap<String, Vec<String>>,
) -> Vec<Vec<Candidate>> {
    let mut parents = (0..candidates.len()).collect::<Vec<_>>();
    let mut owners = BTreeMap::new();
    fn root(p: &[usize], mut i: usize) -> usize {
        while p[i] != i {
            i = p[i];
        }
        i
    }
    for (i, c) in candidates.iter().enumerate() {
        let mut own = c
            .contexts
            .iter()
            .map(|x| slice(&x.text, x.highlight_start, x.highlight_end))
            .collect::<Vec<_>>();
        own.extend(forms.get(&c.term).cloned().unwrap_or_default());
        for f in own {
            let f = fold(&f);
            if let Some(&j) = owners.get(&f) {
                let a = root(&parents, i);
                let b = root(&parents, j);
                parents[a] = b;
            } else {
                owners.insert(f, i);
            }
        }
    }
    let mut indices = Vec::new();
    let mut result: Vec<Vec<Candidate>> = Vec::new();
    for (i, c) in candidates.iter().enumerate() {
        let r = root(&parents, i);
        let group = if let Some(n) = indices.iter().position(|&x| x == r) {
            n
        } else {
            indices.push(r);
            result.push(vec![]);
            result.len() - 1
        };
        result[group].push(c.clone());
    }
    result
}
pub fn batches(
    groups: Vec<Vec<Candidate>>,
    size: usize,
    max: usize,
) -> Result<Vec<Vec<Candidate>>> {
    let mut all = Vec::new();
    let mut current = Vec::new();
    for group in groups {
        if group.len() > size || canonical(&group).chars().count() > max {
            return Err(Error::input(
                "A related candidate group exceeds batch_size or max_chars; increase the limit to keep shared forms together",
            ));
        }
        let mut combined = current.clone();
        combined.extend(group.clone());
        if !current.is_empty()
            && (combined.len() > size || canonical(&combined).chars().count() > max)
        {
            all.push(std::mem::take(&mut current));
        }
        current.extend(group);
    }
    if !current.is_empty() {
        all.push(current);
    }
    Ok(all)
}
pub fn shorten(c: &Context, max: usize) -> Context {
    let chars = c.text.chars().collect::<Vec<_>>();
    let mut boundaries = vec![0];
    let mut i = 0;
    while i < chars.len() {
        if ['.', '!', '?', '。', '！', '？'].contains(&chars[i])
            && !(chars[i] == '.'
                && i > 0
                && chars[i - 1].is_ascii_digit()
                && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let mut end = i + 1;
            if chars[i] == '.' {
                while chars.get(end).is_some_and(char::is_ascii_digit) {
                    end += 1;
                }
            }
            while chars
                .get(end)
                .is_some_and(|c| ['”', '’', '"', '\'', ')', ']'].contains(c))
            {
                end += 1;
            }
            if end == chars.len() || chars[end].is_whitespace() {
                boundaries.push(end);
                i = end.saturating_sub(1);
            }
        }
        i += 1;
    }
    boundaries.push(chars.len());
    let mut left = *boundaries
        .iter()
        .filter(|&&v| v <= c.highlight_start)
        .max()
        .unwrap_or(&0);
    let mut right = *boundaries
        .iter()
        .filter(|&&v| v >= c.highlight_end)
        .min()
        .unwrap_or(&chars.len());
    if slice(&c.text, left, right).split_whitespace().count() > max {
        let mut cuts = vec![left];
        for (i, ch) in chars.iter().enumerate().take(right).skip(left) {
            if [',', ';', ':', '，', '；', '：', '、', '—', '–'].contains(ch)
                && !(c.highlight_start < i + 1 && i + 1 < c.highlight_end)
            {
                cuts.push(i + 1);
            }
        }
        cuts.push(right);
        let mut first = left;
        for win in cuts.windows(2) {
            if win[0] > first && slice(&c.text, first, win[1]).split_whitespace().count() > max {
                if first <= c.highlight_start && win[0] >= c.highlight_end {
                    right = win[0];
                    left = first;
                    break;
                }
                first = win[0];
            }
            if win[1] == right && first <= c.highlight_start {
                left = first;
            }
        }
    }
    while left < right && chars[left].is_whitespace() {
        left += 1;
    }
    while right > left && chars[right - 1].is_whitespace() {
        right -= 1;
    }
    let mut out = c.clone();
    out.text = slice(&c.text, left, right);
    out.highlight_start -= left;
    out.highlight_end -= left;
    out.matches = out.matches.iter().map(|v| v + left).collect();
    out
}

/// Recompute bounds after shortening while retaining the selected occurrence.
/// Never rematch a repeated sentence to a different place in the recording.
pub fn retime(context: &mut Context, cues: &[Cue]) -> Result<()> {
    let (source, spans) = plaintext(cues);
    let start = *context
        .matches
        .first()
        .ok_or_else(|| Error::input("Example has no source occurrence"))?;
    let end = start + context.text.chars().count();
    if end > source.chars().count() || slice(&source, start, end) != context.text {
        return Err(Error::input(
            "Example no longer matches its source occurrence",
        ));
    }
    let mut intervals = Vec::new();
    let mut ids = Vec::new();
    for span in spans
        .iter()
        .filter(|span| span.start < end && span.end > start)
    {
        let cue = cues
            .iter()
            .find(|cue| cue.id == span.cue_id)
            .ok_or_else(|| Error::input("Missing source cue"))?;
        ids.push(cue.id.clone());
        let positions = word_spans(&text::normalize(&cue.text), &cue.words);
        let words = cue
            .words
            .iter()
            .zip(positions)
            .filter(|(_, (left, right))| span.start + left < end && span.start + right > start)
            .map(|(word, _)| word)
            .collect::<Vec<_>>();
        intervals.push(if let Some(word) = words.first() {
            (Some(word.start_ms), Some(words.last().unwrap().end_ms))
        } else {
            (cue.start_ms, cue.end_ms)
        });
    }
    context.cue_ids = ids;
    context.start_ms = intervals.iter().filter_map(|interval| interval.0).min();
    context.end_ms = intervals.iter().filter_map(|interval| interval.1).max();
    Ok(())
}
