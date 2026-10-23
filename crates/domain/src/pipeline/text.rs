use super::*;
use regex::Regex;
use std::collections::BTreeSet;
pub fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
pub fn slice(s: &str, start: usize, end: usize) -> String {
    s.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}
pub fn milliseconds(s: f64) -> Result<u64> {
    if !s.is_finite() || s < 0.0 || s * 1000.0 >= u64::MAX as f64 {
        return Err(Error::input("Time must be finite nonnegative seconds"));
    }
    Ok((s * 1000.0).round_ties_even() as u64)
}
pub fn validate(cues: &[Cue], duration: Option<u64>) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut previous = 0;
    for c in cues {
        nonblank(&c.id, "Cue ID")?;
        nonblank(&c.text, "Cue text")?;
        if !seen.insert(&c.id) {
            return Err(Error::input("Duplicate cue ID"));
        }
        match (duration, c.start_ms, c.end_ms) {
            (None, None, None) if c.words.is_empty() => {}
            (Some(d), Some(s), Some(e)) if s >= previous && e > s && e <= d => {
                previous = s;
                let mut p = s;
                for w in &c.words {
                    nonblank(&w.text, "Aligned word")?;
                    if w.start_ms < p || w.end_ms <= w.start_ms || w.end_ms > e {
                        return Err(Error::input("Invalid aligned word timing"));
                    }
                    p = w.start_ms;
                }
            }
            _ => return Err(Error::input(format!("Invalid timing for cue {}", c.id))),
        }
    }
    Ok(())
}
pub fn import(raw: &str, max: usize) -> Result<Vec<Cue>> {
    if raw.contains('\0') || max == 0 {
        return Err(Error::input("Invalid text input or unit size"));
    }
    let raw = raw.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut values = Vec::new();
    for paragraph in Regex::new(r"\n\s*\n").unwrap().split(&raw) {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if word.chars().count() > max {
                return Err(Error::input(
                    "Text contains a token too long for bounded model requests",
                ));
            }
            if !current.is_empty() && current.chars().count() + word.chars().count() + 1 > max {
                values.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            values.push(current);
        }
    }
    let cues = values
        .into_iter()
        .enumerate()
        .map(|(i, text)| Cue {
            id: format!("cue-{:06}", i + 1),
            start_ms: None,
            end_ms: None,
            text,
            words: vec![],
        })
        .collect::<Vec<_>>();
    validate(&cues, None)?;
    Ok(cues)
}
pub fn plaintext(cues: &[Cue]) -> (String, Vec<Span>) {
    let mut cursor = 0;
    let mut parts = Vec::new();
    let mut spans = Vec::new();
    for c in cues {
        let s = normalize(&c.text);
        let end = cursor + s.chars().count();
        spans.push(Span {
            cue_id: c.id.clone(),
            start: cursor,
            end,
        });
        parts.push(s);
        cursor = end + 1;
    }
    (parts.join(" "), spans)
}
pub fn render_text(cues: &[Cue]) -> String {
    let mut s = cues
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    if !cues.is_empty() {
        s.push('\n');
    }
    s
}
pub fn parse_srt(raw: &str, duration: u64) -> Result<Vec<Cue>> {
    let re =
        Regex::new(r"^(\d{2,}):(\d{2}):(\d{2}),(\d{3})\s+-->\s+(\d{2,}):(\d{2}):(\d{2}),(\d{3})$")
            .unwrap();
    let raw = raw.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut cues = Vec::new();
    if raw.trim().is_empty() {
        return Ok(cues);
    }
    for block in Regex::new(r"\n\s*\n").unwrap().split(raw.trim()) {
        let lines = block.lines().collect::<Vec<_>>();
        if lines.len() < 3 || !lines[0].chars().all(|c| c.is_ascii_digit()) {
            return Err(Error::input("Malformed SRT block"));
        }
        let c = re
            .captures(lines[1])
            .ok_or_else(|| Error::input("Invalid SRT timestamp"))?;
        let at = |offset: usize| -> Result<u64> {
            let n = (offset..offset + 4)
                .map(|i| {
                    c[i].parse::<u64>()
                        .map_err(|_| Error::input("Invalid SRT number"))
                })
                .collect::<Result<Vec<_>>>()?;
            if n[1] > 59 || n[2] > 59 || n[0] > u64::MAX / 3_600_000 {
                return Err(Error::input("Invalid SRT time"));
            }
            n[0].checked_mul(3_600_000)
                .and_then(|v| v.checked_add(n[1] * 60_000 + n[2] * 1000 + n[3]))
                .ok_or_else(|| Error::input("SRT time overflow"))
        };
        cues.push(Cue {
            id: format!("cue-{:06}", cues.len() + 1),
            start_ms: Some(at(1)?),
            end_ms: Some(at(5)?),
            text: lines[2..].join("\n"),
            words: vec![],
        });
    }
    validate(&cues, Some(duration))?;
    Ok(cues)
}
pub fn render_srt(cues: &[Cue], width: usize) -> Result<String> {
    fn time(n: u64) -> String {
        format!(
            "{:02}:{:02}:{:02},{:03}",
            n / 3_600_000,
            n / 60_000 % 60,
            n / 1000 % 60,
            n % 1000
        )
    }
    let mut blocks = Vec::new();
    for (i, c) in cues.iter().enumerate() {
        let (s, e) = c
            .start_ms
            .zip(c.end_ms)
            .ok_or_else(|| Error::input("Untimed text cannot be rendered as SRT"))?;
        let mut lines = Vec::new();
        for line in c.text.lines() {
            let mut row = String::new();
            for w in line.split_whitespace() {
                if !row.is_empty() && row.chars().count() + w.chars().count() + 1 > width {
                    lines.push(std::mem::take(&mut row));
                }
                if !row.is_empty() {
                    row.push(' ');
                }
                row.push_str(w);
            }
            lines.push(row);
        }
        blocks.push(format!(
            "{}\n{} --> {}\n{}",
            i + 1,
            time(s),
            time(e),
            lines.join("\n")
        ));
    }
    Ok(if blocks.is_empty() {
        String::new()
    } else {
        blocks.join("\n\n") + "\n"
    })
}
pub fn word_spans(source: &str, words: &[AlignedWord]) -> Vec<(usize, usize)> {
    let mut cursor = 0;
    let mut out = Vec::new();
    for w in words {
        let tail = slice(source, cursor, source.chars().count());
        let Some(byte) = tail.find(w.text.trim()) else {
            return vec![];
        };
        let start = cursor + tail[..byte].chars().count();
        cursor = start + w.text.trim().chars().count();
        out.push((start, cursor));
    }
    out
}
pub fn normalize_asr(data: &Value, duration: u64, settings: &Value) -> Result<Vec<Cue>> {
    let segments = data["segments"]
        .as_array()
        .ok_or_else(|| Error::input("ASR JSON must contain a segments array"))?;
    let offset = data
        .get("blitzline_audio_offset_ms")
        .map(|x| {
            x.as_u64()
                .ok_or_else(|| Error::input("Invalid audio offset"))
        })
        .transpose()?
        .unwrap_or(0);
    let mut cues = Vec::new();
    for seg in segments {
        let source = seg["text"]
            .as_str()
            .ok_or_else(|| Error::input("ASR segment text must be a string"))?
            .trim();
        if source.is_empty() {
            continue;
        }
        let s = milliseconds(
            seg["start"]
                .as_f64()
                .ok_or_else(|| Error::input("Missing segment start"))?,
        )?;
        let e = milliseconds(
            seg["end"]
                .as_f64()
                .ok_or_else(|| Error::input("Missing segment end"))?,
        )?;
        if e <= s {
            return Err(Error::input("ASR segment end must follow its start"));
        }
        let mut words = Vec::new();
        if let Some(rows) = seg.get("words") {
            let rows = rows
                .as_array()
                .ok_or_else(|| Error::input("ASR words must be an array"))?;
            if rows
                .iter()
                .all(|w| w.get("start").is_some() && w.get("end").is_some())
            {
                for w in rows {
                    let t = w["word"]
                        .as_str()
                        .ok_or_else(|| Error::input("Missing aligned word text"))?;
                    nonblank(t, "ASR word")?;
                    words.push(AlignedWord {
                        text: t.into(),
                        start_ms: milliseconds(
                            w["start"]
                                .as_f64()
                                .ok_or_else(|| Error::input("Invalid word start"))?,
                        )?,
                        end_ms: milliseconds(
                            w["end"]
                                .as_f64()
                                .ok_or_else(|| Error::input("Invalid word end"))?,
                        )?,
                    });
                }
            }
        }
        let positions = word_spans(source, &words);
        if words.is_empty() || positions.len() != words.len() {
            cues.push(Cue {
                id: String::new(),
                text: source.into(),
                start_ms: Some(s),
                end_ms: Some(e),
                words: vec![],
            });
            continue;
        }
        let mut first = 0;
        for (i, w) in words.iter().enumerate() {
            let tail = i + 1 == words.len();
            let boundary = w
                .text
                .trim_end_matches(['\"', '\'', '’', '”', ')'])
                .ends_with(['.', '!', '?']);
            let gap = !tail
                && words[i + 1].start_ms.saturating_sub(w.end_ms)
                    >= settings["pause_ms"].as_u64().unwrap_or(800);
            if tail
                || gap
                || boundary
                || w.end_ms.saturating_sub(words[first].start_ms)
                    >= settings["max_cue_ms"].as_u64().unwrap_or(8000)
                || (positions[i].1 - positions[first].0) as u64
                    >= settings["line_length"].as_u64().unwrap_or(42) * 2
            {
                let left = if first == 0 { 0 } else { positions[first].0 };
                let right = if tail {
                    source.chars().count()
                } else {
                    positions[i + 1].0
                };
                cues.push(Cue {
                    id: String::new(),
                    text: slice(source, left, right).trim().into(),
                    start_ms: Some(words[first].start_ms),
                    end_ms: Some(w.end_ms),
                    words: words[first..=i].to_vec(),
                });
                first = i + 1;
            }
        }
    }
    for (i, c) in cues.iter_mut().enumerate() {
        c.id = format!("cue-{:06}", i + 1);
        c.start_ms = c.start_ms.and_then(|v| v.checked_add(offset));
        c.end_ms = c.end_ms.and_then(|v| v.checked_add(offset));
        for w in &mut c.words {
            w.start_ms = w
                .start_ms
                .checked_add(offset)
                .ok_or_else(|| Error::input("Word time overflow"))?;
            w.end_ms = w
                .end_ms
                .checked_add(offset)
                .ok_or_else(|| Error::input("Word time overflow"))?;
        }
    }
    validate(&cues, Some(duration))?;
    Ok(cues)
}
