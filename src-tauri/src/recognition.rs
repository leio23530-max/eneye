use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// Frequency and dictionary evidence used to judge an OCR/UIA candidate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WordEvidence {
    pub dictionary_hit: bool,
    pub frq: Option<i64>,
    pub bnc: Option<i64>,
    pub collins: i64,
    pub oxford: bool,
}

impl WordEvidence {
    pub fn rank(&self) -> Option<i64> {
        [self.frq, self.bnc]
            .into_iter()
            .flatten()
            .filter(|rank| *rank > 0)
            .min()
    }

    pub fn is_common_short_word(&self) -> bool {
        self.oxford || self.collins >= 3 || self.rank().is_some_and(|rank| rank <= 3_000)
    }
}

/// A candidate that survived validation and can be shown in diagnostics and,
/// when it wins, on the card.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecognitionCandidate {
    pub text: String,
    pub provider: String,
    pub raw_token: Option<String>,
    pub pointer_inside: bool,
    pub pointer_on_letter: bool,
    pub merge_count: usize,
    pub crossed_space: bool,
    pub crossed_punctuation: bool,
    pub edit_distance: usize,
    pub dictionary_hit: bool,
    pub frq: Option<i64>,
    pub bnc: Option<i64>,
    pub collins: i64,
    pub oxford: bool,
    pub confidence: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OcrBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl OcrBox {
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(self) -> f32 {
        self.y + self.height
    }

    pub fn contains_with_margin(&self, x: i32, y: i32, margin: f32) -> bool {
        let x = x as f32;
        let y = y as f32;
        x >= self.x - margin
            && x <= self.right() + margin
            && y >= self.y - margin
            && y <= self.bottom() + margin
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrToken {
    pub text: String,
    pub bounds: OcrBox,
}

impl OcrToken {
    pub fn char_index_at(&self, x: i32) -> Option<usize> {
        let characters = self.text.chars().count();
        if characters == 0 || self.bounds.width <= 0.0 {
            return None;
        }
        let fraction = ((x as f32 - self.bounds.x) / self.bounds.width).clamp(0.0, 0.999_999);
        Some((fraction * characters as f32).floor() as usize)
    }
}

#[derive(Debug, Clone)]
pub struct GeneratedCandidate {
    pub text: String,
    pub raw_token: String,
    pub merge_count: usize,
    pub crossed_space: bool,
    pub crossed_punctuation: bool,
    pub edit_distance: usize,
    pub raw_exact: bool,
    pub origin: &'static str,
    pub pointer_char: usize,
}

impl GeneratedCandidate {
    pub fn from_hyphen(text: String, raw_token: String, pointer_char: usize) -> Self {
        Self {
            text,
            raw_token,
            merge_count: 0,
            crossed_space: false,
            crossed_punctuation: false,
            edit_distance: 0,
            raw_exact: false,
            origin: "hyphen",
            pointer_char,
        }
    }

    fn new(
        text: String,
        raw_token: &str,
        pointer_char: usize,
        origin: &'static str,
        raw_exact: bool,
        edit_distance: usize,
    ) -> Self {
        Self {
            text,
            raw_token: raw_token.to_owned(),
            merge_count: 0,
            crossed_space: false,
            crossed_punctuation: false,
            edit_distance,
            raw_exact,
            origin,
            pointer_char,
        }
    }
}

fn allowed_word_character(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '\'' || character == '-'
}

/// Returns the cleaned word plus its character span inside the raw OCR token.
fn clean_span(text: &str) -> Option<(String, usize, usize)> {
    let characters = text.chars().collect::<Vec<_>>();
    let mut start = characters
        .iter()
        .position(|character| allowed_word_character(*character))?;
    let mut end = characters
        .iter()
        .rposition(|character| allowed_word_character(*character))?
        + 1;
    if start + 2 < end && characters[start] == '\'' && characters[end - 1] == '\'' {
        start += 1;
        end -= 1;
    }
    if start >= end {
        return None;
    }
    let word = characters[start..end].iter().collect::<String>();
    if word.is_empty()
        || !word.chars().all(allowed_word_character)
        || !word
            .chars()
            .any(|character| character.is_ascii_alphabetic())
    {
        return None;
    }
    Some((word, start, end))
}

/// A single letter followed by ")" is an ordered-list marker ("A)"), not a
/// word. "As" in the same token is still a valid candidate.
fn single_letter_list_marker(raw: &str, start: usize, end: usize) -> bool {
    if end != start + 1 {
        return false;
    }
    let chars = raw.chars().collect::<Vec<_>>();
    (start > 0 && chars[start - 1] == ')') || (end < chars.len() && chars[end] == ')')
}

#[allow(clippy::too_many_arguments)]
fn push_span_candidate(
    output: &mut Vec<GeneratedCandidate>,
    raw: &str,
    pointer_char: usize,
    span_start: usize,
    span_end: usize,
    origin: &'static str,
    raw_exact: bool,
    edit_distance: usize,
    transform: impl Fn(&[char]) -> String,
) {
    if pointer_char < span_start || pointer_char >= span_end {
        return;
    }
    let chars = raw.chars().collect::<Vec<_>>();
    let text = transform(&chars[span_start..span_end]);
    if text.is_empty() {
        return;
    }
    let candidate = GeneratedCandidate::new(
        text,
        raw,
        pointer_char - span_start,
        origin,
        raw_exact,
        edit_distance,
    );
    if !output.iter().any(|existing| {
        existing.text == candidate.text
            && existing.origin == candidate.origin
            && existing.pointer_char == candidate.pointer_char
    }) {
        output.push(candidate);
    }
}

/// Generates candidates for the character under the pointer.
pub fn generate_ocr_candidates_at(
    tokens: &[OcrToken],
    selected: usize,
    pointer_x: i32,
    _pointer_y: i32,
    next_line: Option<&[OcrToken]>,
) -> Vec<GeneratedCandidate> {
    let Some(token) = tokens.get(selected) else {
        return Vec::new();
    };
    let Some(pointer_char) = token.char_index_at(pointer_x) else {
        return Vec::new();
    };
    let raw = token.text.as_str();
    let mut output: Vec<GeneratedCandidate> = Vec::new();

    if raw.chars().any(|character| character.is_whitespace()) {
        let joined = raw
            .chars()
            .filter(|character| character.is_ascii_alphabetic())
            .collect::<String>();
        let joined_pointer = raw
            .chars()
            .take(pointer_char)
            .filter(|character| character.is_ascii_alphabetic())
            .count();
        if joined.chars().count() >= 4
            && joined_pointer < joined.chars().count()
            && !output
                .iter()
                .any(|existing| existing.text == joined && existing.origin == "joined")
        {
            output.push(GeneratedCandidate::new(
                joined,
                raw,
                joined_pointer,
                "joined",
                false,
                0,
            ));
        }
    }

    if let Some((word, start, end)) = clean_span(raw) {
        let base_chars = word.chars().collect::<Vec<_>>();
        if !single_letter_list_marker(raw, start, end) {
            push_span_candidate(
                &mut output,
                raw,
                pointer_char,
                start,
                end,
                "raw",
                true,
                0,
                |chars| chars.iter().collect(),
            );
            for variant in spelling_variants(&base_chars) {
                let distance = hamming(&base_chars, &variant.chars().collect::<Vec<_>>());
                push_span_candidate(
                    &mut output,
                    raw,
                    pointer_char,
                    start,
                    end,
                    "spelling",
                    false,
                    distance,
                    |_| variant.clone(),
                );
            }
        }
        // OCR often turns a leading "l" into "1" and drops it from the
        // clean span. Include up to two edge characters and correct them too.
        for count in 1..=2 {
            if start < count {
                continue;
            }
            let extended = raw
                .chars()
                .skip(start - count)
                .take(end - (start - count))
                .collect::<Vec<_>>();
            for variant in spelling_variants(&extended) {
                if variant.chars().count() < 2 {
                    continue;
                }
                let distance = hamming(&extended, &variant.chars().collect::<Vec<_>>());
                push_span_candidate(
                    &mut output,
                    raw,
                    pointer_char,
                    start - count,
                    end,
                    "edge-spelling",
                    false,
                    distance,
                    |_| variant.clone(),
                );
            }
        }
        for (count, from_left) in [(1_usize, true), (2, true), (1, false), (2, false)] {
            if base_chars.len() < count + 3 {
                continue;
            }
            let (span_start, span_end) = if from_left {
                (start + count, end)
            } else {
                (start, end - count)
            };
            let trimmed = if from_left {
                &base_chars[count..]
            } else {
                &base_chars[..base_chars.len() - count]
            };
            let trimmed_chars = trimmed.to_vec();
            push_span_candidate(
                &mut output,
                raw,
                pointer_char,
                span_start,
                span_end,
                "boundary-trim",
                false,
                0,
                |_| trimmed_chars.iter().collect::<String>(),
            );
            for variant in spelling_variants(&trimmed_chars) {
                let distance = hamming(&trimmed_chars, &variant.chars().collect::<Vec<_>>());
                push_span_candidate(
                    &mut output,
                    raw,
                    pointer_char,
                    span_start,
                    span_end,
                    "boundary-spelling",
                    false,
                    distance,
                    |_| variant.clone(),
                );
            }
        }
    }

    // A digit or x can sit inside an otherwise valid word ("resu1t",
    // "ana1ysis"). clean_span rejects the raw token, so repair the whole
    // token while keeping the pointer on the same character.
    let raw_chars = raw.chars().collect::<Vec<_>>();
    let pointer_on_letter = raw_chars
        .get(pointer_char)
        .is_some_and(|character| character.is_ascii_alphabetic());
    let has_internal_digit = raw_chars
        .iter()
        .any(|character| matches!(character, '0' | '1' | '5'));
    if (clean_span(raw).is_none() || has_internal_digit) && pointer_on_letter {
        for variant in spelling_variants(&raw_chars) {
            if clean_span(&variant).is_none() {
                continue;
            }
            let distance = hamming(&raw_chars, &variant.chars().collect::<Vec<_>>());
            push_span_candidate(
                &mut output,
                raw,
                pointer_char,
                0,
                raw_chars.len(),
                "raw-spelling",
                false,
                distance,
                |_| variant.clone(),
            );
        }
    }

    for (start, end) in letter_segments(raw) {
        if pointer_char < start || pointer_char >= end {
            continue;
        }
        if single_letter_list_marker(raw, start, end) {
            continue;
        }
        let text = raw
            .chars()
            .skip(start)
            .take(end - start)
            .collect::<String>();
        if clean_span(&text).is_none() {
            continue;
        }
        push_span_candidate(
            &mut output,
            raw,
            pointer_char,
            start,
            end,
            "segment",
            false,
            0,
            |chars| chars.iter().collect(),
        );
    }

    push_fragment_candidates(tokens, selected, pointer_char, &mut output);
    push_hyphen_candidates(tokens, selected, pointer_char, next_line, &mut output);
    output
}

/// Rebuilds a word broken by a line-end hyphen, e.g. "iden-" / "tification".
/// Only known dictionary words survive scoring; this does not synthesize a
/// word for an unknown OCR fragment.
fn push_hyphen_candidates(
    tokens: &[OcrToken],
    selected: usize,
    selected_pointer_char: usize,
    next_line: Option<&[OcrToken]>,
    output: &mut Vec<GeneratedCandidate>,
) {
    let Some(next_line) = next_line else {
        return;
    };
    let hyphen_index = if tokens
        .get(selected)
        .is_some_and(|token| token.text.trim_end().ends_with('-'))
    {
        selected
    } else if tokens
        .get(selected + 1)
        .is_some_and(|token| token.text.trim_end().ends_with('-'))
    {
        selected + 1
    } else {
        return;
    };
    let Some(selected_token) = tokens.get(selected) else {
        return;
    };
    let selected_letter_index = letters_before(selected_token, selected_pointer_char);

    for start in [
        selected,
        selected.saturating_sub(1),
        selected.saturating_sub(2),
    ] {
        if start > hyphen_index {
            continue;
        }
        let mut base = String::new();
        let mut prefix_chars = 0;
        for (index, token) in tokens.iter().enumerate().take(hyphen_index + 1).skip(start) {
            let letters = letters_fragment(&token.text);
            if index == selected {
                prefix_chars = base.chars().count() + selected_letter_index;
            }
            base.push_str(&letters);
        }
        if base.chars().count() < 2 {
            continue;
        }
        let mut suffix = String::new();
        for token in next_line.iter().take(8) {
            suffix.push_str(&letters_fragment(&token.text));
            if suffix.chars().count() < 2 {
                continue;
            }
            let text = format!("{base}{suffix}");
            if output.iter().any(|candidate| candidate.text == text) {
                continue;
            }
            let raw = format!(
                "{} {}",
                tokens[start..=hyphen_index]
                    .iter()
                    .map(|token| token.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                token.text
            );
            if prefix_chars < text.chars().count() {
                output.push(GeneratedCandidate::from_hyphen(text, raw, prefix_chars));
            }
        }
    }
}

fn letters_before(token: &OcrToken, pointer_char: usize) -> usize {
    token
        .text
        .chars()
        .take(pointer_char)
        .filter(|character| character.is_ascii_alphabetic())
        .count()
}

fn letters_fragment(text: &str) -> String {
    text.chars().filter(char::is_ascii_alphabetic).collect()
}

fn push_fragment_candidates(
    tokens: &[OcrToken],
    selected: usize,
    selected_pointer_char: usize,
    output: &mut Vec<GeneratedCandidate>,
) {
    if tokens.len() < 2 {
        return;
    }
    let median_gap = median_word_gap(tokens);
    let selected_token = &tokens[selected];
    let selected_height = selected_token.bounds.height.max(1.0);
    let limit = (median_gap * 0.60).min(selected_height * 0.35).max(2.5);

    let Some(mut right_text) = mergeable_text(selected_token) else {
        return;
    };
    let mut right_end = selected;
    let prefix_chars = selected_pointer_char;
    while right_end + 1 < tokens.len() && right_end < selected + 2 {
        let next = &tokens[right_end + 1];
        if !should_merge(&tokens[right_end], next, limit) {
            break;
        }
        let Some(next_text) = mergeable_text(next) else {
            break;
        };
        right_text.push_str(&next_text);
        right_end += 1;
        if let Some(candidate) = merged_candidate(
            &right_text,
            tokens,
            selected,
            right_end,
            prefix_chars,
            "merge-right",
        ) {
            output.push(candidate);
        }
    }

    let Some(mut left_text) = mergeable_text(selected_token) else {
        return;
    };
    let mut left_start = selected;
    let mut left_prefix_chars = selected_pointer_char;
    while left_start > 0 && selected - left_start < 2 {
        let previous = &tokens[left_start - 1];
        if !should_merge(previous, &tokens[left_start], limit) {
            break;
        }
        let Some(previous_text) = mergeable_text(previous) else {
            break;
        };
        left_text = format!("{previous_text}{left_text}");
        left_start -= 1;
        left_prefix_chars += previous_text.chars().count();
        if let Some(candidate) = merged_candidate(
            &left_text,
            tokens,
            left_start,
            selected,
            left_prefix_chars,
            "merge-left",
        ) {
            output.push(candidate);
        }
    }
    let _ = selected_token;
}

fn merged_candidate(
    text: &str,
    tokens: &[OcrToken],
    start: usize,
    end: usize,
    pointer_char: usize,
    origin: &'static str,
) -> Option<GeneratedCandidate> {
    if text.is_empty() || pointer_char >= text.chars().count() {
        return None;
    }
    let raw = tokens[start..=end]
        .iter()
        .map(|token| token.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let mut candidate =
        GeneratedCandidate::new(text.to_owned(), &raw, pointer_char, origin, false, 0);
    candidate.merge_count = end - start;
    Some(candidate)
}

fn should_merge(left: &OcrToken, right: &OcrToken, limit: f32) -> bool {
    if mergeable_text(left).is_none() || mergeable_text(right).is_none() {
        return false;
    }
    let vertical_overlap = (left.bounds.bottom().min(right.bounds.bottom())
        - left.bounds.y.max(right.bounds.y))
    .max(0.0);
    if vertical_overlap < left.bounds.height.min(right.bounds.height) * 0.5 {
        return false;
    }
    let gap = right.bounds.x - left.bounds.right();
    gap >= -2.0 && gap <= limit
}

fn mergeable_text(token: &OcrToken) -> Option<String> {
    let raw = token.text.trim();
    let raw =
        raw.trim_matches(|character: char| matches!(character, '"' | '\'' | '“' | '”' | '‘' | '’'));
    if raw.is_empty() {
        return None;
    }
    let mut text = String::new();
    for character in raw.chars() {
        if character.is_ascii_alphabetic() {
            text.push(character);
        } else if raw.chars().count() <= 2 && matches!(character, '0' | '1' | '5') {
            text.push(match character {
                '0' => 'o',
                '1' => 'l',
                '5' => 's',
                _ => unreachable!(),
            });
        } else {
            return None;
        }
    }
    (!text.is_empty()).then_some(text)
}

fn median_word_gap(tokens: &[OcrToken]) -> f32 {
    let mut gaps = Vec::new();
    for pair in tokens.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        let vertical_overlap = (left.bounds.bottom().min(right.bounds.bottom())
            - left.bounds.y.max(right.bounds.y))
        .max(0.0);
        if vertical_overlap >= left.bounds.height.min(right.bounds.height) * 0.5 {
            let gap = right.bounds.x - left.bounds.right();
            if gap >= 0.0 {
                gaps.push(gap);
            }
        }
    }
    if gaps.is_empty() {
        return tokens
            .first()
            .map(|token| token.bounds.height * 0.30)
            .unwrap_or(8.0);
    }
    gaps.sort_by(|left, right| left.total_cmp(right));
    gaps[gaps.len() / 2]
}

fn letter_segments(text: &str) -> Vec<(usize, usize)> {
    let mut segments = Vec::new();
    let mut start = None;
    for (index, character) in text.chars().enumerate() {
        if character.is_ascii_alphabetic() || character == '\'' || character == '-' {
            start.get_or_insert(index);
        } else if let Some(start) = start.take() {
            segments.push((start, index));
        }
    }
    if let Some(start) = start {
        segments.push((start, text.chars().count()));
    }
    segments
}

fn spelling_variants(base: &[char]) -> Vec<String> {
    let options = base
        .iter()
        .enumerate()
        .filter_map(|(index, character)| {
            let replacements: &[char] = match character {
                '1' => &['l', 'i'],
                'l' | 'L' => &['i'],
                'i' | 'I' => &['l'],
                '0' => &['o'],
                '5' | 'S' => &['s'],
                'x' | 'X' => &['i'],
                _ => &[],
            };
            (!replacements.is_empty()).then(|| (index, replacements.to_vec()))
        })
        .collect::<Vec<_>>();
    let mut variants = Vec::new();
    for (index, replacements) in &options {
        for replacement in replacements {
            let mut candidate = base.to_vec();
            candidate[*index] = *replacement;
            push_variant(&mut variants, &candidate);
        }
    }
    for left in 0..options.len() {
        for right in (left + 1)..options.len() {
            for left_replacement in &options[left].1 {
                for right_replacement in &options[right].1 {
                    let mut candidate = base.to_vec();
                    candidate[options[left].0] = *left_replacement;
                    candidate[options[right].0] = *right_replacement;
                    push_variant(&mut variants, &candidate);
                }
            }
        }
    }
    variants
}

fn push_variant(variants: &mut Vec<String>, characters: &[char]) {
    if variants.len() >= 128 {
        return;
    }
    let candidate = characters.iter().collect::<String>();
    if !variants.contains(&candidate) {
        variants.push(candidate);
    }
}

fn hamming(left: &[char], right: &[char]) -> usize {
    left.iter()
        .zip(right.iter())
        .filter(|(left, right)| left != right)
        .count()
}

pub fn is_plausible_unknown(text: &str) -> bool {
    let chars = text.chars().collect::<Vec<_>>();
    if !(4..=45).contains(&chars.len())
        || !chars
            .iter()
            .all(|character| allowed_word_character(*character))
    {
        return false;
    }
    if !chars[0].is_ascii_alphabetic() || !chars[chars.len() - 1].is_ascii_alphabetic() {
        return false;
    }
    let all_upper = chars
        .iter()
        .all(|character| !character.is_ascii_alphabetic() || character.is_ascii_uppercase());
    if !all_upper
        && chars
            .iter()
            .skip(1)
            .any(|character| character.is_ascii_uppercase())
    {
        return false;
    }
    true
}

pub fn score_candidate(
    generated: &GeneratedCandidate,
    evidence: &WordEvidence,
) -> Option<RecognitionCandidate> {
    let chars = generated.text.chars().collect::<Vec<_>>();
    if chars.is_empty() || chars.len() > 45 {
        return None;
    }
    if !chars
        .iter()
        .all(|character| allowed_word_character(*character))
    {
        return None;
    }
    if chars.len() == 1
        && !generated.text.eq_ignore_ascii_case("a")
        && !generated.text.eq_ignore_ascii_case("i")
    {
        return None;
    }
    if !generated
        .text
        .chars()
        .any(|character| character.is_ascii_alphabetic())
    {
        return None;
    }
    let pointer_char = chars.get(generated.pointer_char)?;
    if !pointer_char.is_ascii_alphabetic() && *pointer_char != '\'' && *pointer_char != '-' {
        return None;
    }
    let mut score = 0.48_f64;
    let mut reasons = Vec::new();
    if generated.raw_exact {
        score += 0.20;
        reasons.push("OCR 原文".to_string());
    } else {
        score -= 0.03;
    }
    if generated.origin == "boundary-trim" {
        score -= 0.12;
        reasons.push("截断边缘噪声".into());
    }
    score -= 0.05 * generated.edit_distance.min(3) as f64;
    if generated.edit_distance > 0 {
        reasons.push(format!("字符校正 {}", generated.edit_distance));
    }
    score -= 0.06 * generated.merge_count as f64;
    if generated.merge_count > 0 {
        reasons.push(format!("合并 {} 段", generated.merge_count + 1));
    }
    if generated.crossed_space {
        score -= 0.50;
        reasons.push("跨过空格".into());
    }
    if generated.crossed_punctuation {
        score -= 0.35;
        reasons.push("跨过标点".into());
    }

    if evidence.dictionary_hit {
        score += 0.22;
        reasons.push("词典命中".into());
    } else {
        if !generated.raw_exact
            || generated.edit_distance > 0
            || generated.merge_count > 0
            || generated.crossed_space
            || generated.crossed_punctuation
            || !is_plausible_unknown(&generated.text)
        {
            return None;
        }
        score += 0.02;
        reasons.push("未收录但字形完整".into());
    }

    if generated.text.chars().count() == 2 && !evidence.is_common_short_word() {
        return None;
    }
    if evidence.collins > 0 {
        score += 0.04 * evidence.collins.min(5) as f64;
        reasons.push(format!("Collins {}", evidence.collins));
    }
    if evidence.oxford {
        score += 0.08;
        reasons.push("Oxford 3000".into());
    }
    if let Some(rank) = evidence.rank() {
        score += if rank <= 1_000 {
            0.10
        } else if rank <= 3_000 {
            0.08
        } else if rank <= 8_000 {
            0.05
        } else if rank <= 20_000 {
            0.02
        } else {
            0.0
        };
        if rank <= 20_000 {
            reasons.push(format!("词频 {rank}"));
        }
    }
    Some(RecognitionCandidate {
        text: generated.text.clone(),
        provider: "ocr".into(),
        raw_token: Some(generated.raw_token.clone()),
        pointer_inside: true,
        pointer_on_letter: true,
        merge_count: generated.merge_count,
        crossed_space: generated.crossed_space,
        crossed_punctuation: generated.crossed_punctuation,
        edit_distance: generated.edit_distance,
        dictionary_hit: evidence.dictionary_hit,
        frq: evidence.frq,
        bnc: evidence.bnc,
        collins: evidence.collins,
        oxford: evidence.oxford,
        confidence: score.clamp(0.0, 0.99),
        reason: reasons.join("；"),
    })
}

/// Scores every generated spelling once. The caller supplies dictionary
/// evidence, which is the only part that needs a database lookup.
pub fn rank_generated<F>(
    generated: &[GeneratedCandidate],
    mut evidence_for: F,
) -> Vec<RecognitionCandidate>
where
    F: FnMut(&str) -> WordEvidence,
{
    let mut seen = HashSet::new();
    let mut ranked = Vec::new();
    for candidate in generated {
        if !seen.insert(candidate.text.clone()) {
            continue;
        }
        if let Some(scored) = score_candidate(candidate, &evidence_for(&candidate.text)) {
            ranked.push(scored);
        }
    }
    ranked
}

pub fn choose_best(candidates: Vec<RecognitionCandidate>) -> Option<RecognitionCandidate> {
    let mut unique: Vec<RecognitionCandidate> = Vec::new();
    for candidate in candidates {
        if let Some(existing) = unique
            .iter_mut()
            .find(|existing| existing.text == candidate.text)
        {
            if candidate.confidence > existing.confidence {
                *existing = candidate;
            }
        } else {
            unique.push(candidate);
        }
    }
    unique.sort_by(|left, right| {
        right
            .confidence
            .total_cmp(&left.confidence)
            .then_with(|| right.dictionary_hit.cmp(&left.dictionary_hit))
            .then_with(|| right.text.len().cmp(&left.text.len()))
    });
    unique
        .into_iter()
        .find(|candidate| candidate.confidence >= 0.68)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(
        dictionary_hit: bool,
        frq: Option<i64>,
        bnc: Option<i64>,
        collins: i64,
        oxford: bool,
    ) -> WordEvidence {
        WordEvidence {
            dictionary_hit,
            frq,
            bnc,
            collins,
            oxford,
        }
    }

    fn fake_evidence(text: &str) -> WordEvidence {
        match text.to_ascii_lowercase().as_str() {
            "war" => evidence(true, Some(349), Some(304), 5, true),
            "on" => evidence(true, Some(17), Some(17), 5, true),
            "as" => evidence(true, Some(33), Some(33), 5, true),
            "being" => evidence(true, Some(1_676), Some(1_956), 3, false),
            "less" => evidence(true, Some(477), Some(379), 5, true),
            "linear" => evidence(true, Some(5_802), Some(4_880), 1, false),
            "circular" => evidence(true, Some(6_749), Some(4_711), 2, false),
            "identification" => evidence(true, Some(3_289), Some(3_230), 1, false),
            "mining" => evidence(true, Some(4_271), Some(3_819), 2, false),
            "mxning'" | "iden-" | "ccurban" => WordEvidence::default(),
            "urban" => evidence(true, Some(3_721), Some(2_889), 3, false),
            "insufficient" => evidence(true, Some(4_265), Some(4_825), 1, false),
            "wa" => evidence(true, None, Some(11_044), 0, false),
            "nanogenerator" | "anogenerator" => WordEvidence::default(),
            "pollution" => evidence(true, Some(1_200), Some(1_500), 4, true),
            _ if KNOWN_WORDS.contains(&text.to_ascii_lowercase().as_str()) => {
                evidence(true, Some(4_000), Some(3_500), 3, false)
            }
            _ => WordEvidence::default(),
        }
    }

    const KNOWN_WORDS: &[&str] = &[
        "war",
        "on",
        "as",
        "being",
        "less",
        "linear",
        "circular",
        "identification",
        "mining",
        "urban",
        "insufficient",
        "information",
        "other",
        "system",
        "small",
        "result",
        "analysis",
        "selection",
        "implementation",
        "really",
        "will",
        "follow",
        "language",
        "large",
        "example",
        "word",
        "because",
        "methods",
        "results",
        "introduction",
    ];

    fn token(text: &str, x: f32, width: f32) -> OcrToken {
        OcrToken {
            text: text.to_owned(),
            bounds: OcrBox {
                x,
                y: 100.0,
                width,
                height: 20.0,
            },
        }
    }

    fn best_at(tokens: &[OcrToken], pointer_x: i32) -> Option<RecognitionCandidate> {
        let selected = tokens
            .iter()
            .position(|token| token.bounds.contains_with_margin(pointer_x, 110, 1.0))?;
        let generated = generate_ocr_candidates_at(tokens, selected, pointer_x, 110, None);
        choose_best(rank_generated(&generated, fake_evidence))
    }

    #[test]
    fn cleans_quotes_and_reports_a_bounded_span() {
        assert_eq!(clean_span("\"circular\"").unwrap().0, "circular");
        assert!(clean_span("A)As").is_none());
        assert_eq!(clean_span("mxning'").unwrap().0, "mxning'");
    }

    #[test]
    fn splits_punctuation_and_uses_the_pointer_segment() {
        let segments = letter_segments("A)As");
        assert_eq!(segments, vec![(0, 1), (2, 4)]);
    }

    #[test]
    fn pointer_on_as_in_a_list_marker_returns_as_but_pointer_on_a_returns_nothing() {
        let tokens = [token("A)As", 100.0, 50.0)];
        assert_eq!(best_at(&tokens, 135).unwrap().text, "As");
        assert!(best_at(&tokens, 102).is_none());
    }

    #[test]
    fn being_is_not_merged_with_a_previous_list_marker() {
        let tokens = [token("A)As", 100.0, 50.0), token("being", 155.0, 45.0)];
        let best = best_at(&tokens, 170).unwrap();
        assert_eq!(best.text, "being");
        assert!(!best.text.contains("AAs"));
    }

    #[test]
    fn merges_ocr_fragments_of_war_and_on_without_crossing_a_word_space() {
        let war = [token("\"wa", 100.0, 12.0), token("r", 114.0, 6.0)];
        assert_eq!(best_at(&war, 104).unwrap().text, "war");
        let on = [token("0", 100.0, 6.0), token("n", 108.0, 7.0)];
        assert_eq!(best_at(&on, 102).unwrap().text, "on");
        let normal = [token("As", 100.0, 20.0), token("being", 126.0, 45.0)];
        assert_eq!(best_at(&normal, 108).unwrap().text, "As");
    }

    #[test]
    fn restores_quoted_words_and_boundary_ocr_noise() {
        let quoted = [token("\"circular\"", 100.0, 64.0)];
        assert_eq!(best_at(&quoted, 130).unwrap().text, "circular");
        let confusion = [token("1ess", 100.0, 30.0)];
        assert_eq!(best_at(&confusion, 110).unwrap().text, "less");
        let urban = [token("CCurban", 100.0, 55.0)];
        assert_eq!(best_at(&urban, 120).unwrap().text, "urban");
    }

    #[test]
    fn combines_boundary_trimming_with_spelling_correction() {
        let noisy = [token("mxning'", 100.0, 50.0)];
        assert_eq!(best_at(&noisy, 120).unwrap().text, "mining");
    }

    #[test]
    fn rebuilds_identification_across_a_hyphenated_line_break() {
        let tokens = [token("iden-", 100.0, 30.0)];
        let next = [token("tification", 100.0, 60.0)];
        let generated = generate_ocr_candidates_at(&tokens, 0, 110, 110, Some(&next));
        let best = choose_best(rank_generated(&generated, fake_evidence)).unwrap();
        assert_eq!(best.text, "identification");
    }

    #[test]
    fn rejects_single_letters_list_markers_and_weak_two_letter_abbreviations() {
        assert!(best_at(&[token("A)", 100.0, 14.0)], 103).is_none());
        assert!(best_at(&[token("...", 100.0, 14.0)], 104).is_none());
        assert!(best_at(&[token("wa", 100.0, 12.0)], 104).is_none());
    }

    #[test]
    fn joins_a_word_split_by_an_internal_space_in_one_ocr_token() {
        let tokens = [token("pollut ion", 100.0, 190.0)];
        let got = best_at(&tokens, 150).map(|candidate| candidate.text);
        assert_eq!(got.as_deref(), Some("pollution"));
    }

    #[test]
    fn regression_set_covers_one_hundred_realistic_cases() {
        let mut checked = 0_usize;

        // Quoted, wrapped, or punctuated words: the clean span must win.
        for word in [
            "less",
            "linear",
            "circular",
            "identification",
            "mining",
            "urban",
            "insufficient",
            "being",
            "war",
            "information",
        ] {
            for pattern in ["\"{word}\"", "{word}.", "({word})", "“{word}”"] {
                let raw = pattern.replace("{word}", word);
                let width = (raw.chars().count() * 9 + 12) as f32;
                let tokens = [token(&raw, 100.0, width)];
                let got = best_at(&tokens, 120).map(|candidate| candidate.text);
                assert_eq!(got.as_deref(), Some(word), "case: {raw}");
                checked += 1;
            }
        }

        // One or two OCR character confusions.
        for (raw, expected) in [
            ("1ess", "less"),
            ("1inear", "linear"),
            ("0n", "on"),
            ("0ther", "other"),
            ("5ystem", "system"),
            ("5mall", "small"),
            ("resu1t", "result"),
            ("ana1ysis", "analysis"),
            ("se1ection", "selection"),
            ("imp1ementation", "implementation"),
            ("rea11y", "really"),
            ("wi11", "will"),
            ("fo11ow", "follow"),
            ("1anguage", "language"),
            ("1arge", "large"),
            ("examp1e", "example"),
            ("identxfication", "identification"),
            ("mxning", "mining"),
            ("m1ning", "mining"),
            ("w0rd", "word"),
        ] {
            let characters = raw.chars().count();
            let width = (characters * 9 + 12) as f32;
            let letter_index = raw
                .chars()
                .position(|character| character.is_ascii_alphabetic())
                .expect("case must contain a letter");
            let pointer_x =
                (100.0 + (letter_index as f32 + 0.5) * width / characters as f32).round() as i32;
            let tokens = [token(raw, 100.0, width)];
            let got = best_at(&tokens, pointer_x).map(|candidate| candidate.text);
            assert_eq!(got.as_deref(), Some(expected), "case: {raw}");
            checked += 1;
        }

        // Ordered-list markers and punctuation must not poison the word.
        for (raw, pointer_x, expected) in [
            ("A)As", 135, Some("As")),
            ("B)Because", 135, Some("Because")),
            ("C)Circular", 135, Some("Circular")),
            ("1)Introduction", 135, Some("Introduction")),
            ("2)Methods", 135, Some("Methods")),
            ("3)Results", 135, Some("Results")),
            ("'less'", 120, Some("less")),
            ("less,", 120, Some("less")),
            ("less;", 120, Some("less")),
            ("less:", 120, Some("less")),
        ] {
            let width = (raw.chars().count() * 9 + 12) as f32;
            let tokens = [token(raw, 100.0, width)];
            let got = best_at(&tokens, pointer_x).map(|candidate| candidate.text);
            assert_eq!(got.as_deref(), expected, "case: {raw}");
            checked += 1;
        }

        // Split OCR fragments of a single word, but never two real words.
        let fragment_cases: Vec<(Vec<OcrToken>, i32, &str)> = vec![
            (
                vec![token("\"wa", 100.0, 24.0), token("r", 126.0, 8.0)],
                104,
                "war",
            ),
            (
                vec![token("0", 100.0, 8.0), token("n", 110.0, 8.0)],
                102,
                "on",
            ),
            (
                vec![token("mini", 100.0, 36.0), token("ng", 138.0, 18.0)],
                104,
                "mining",
            ),
            (
                vec![token("circul", 100.0, 54.0), token("ar", 156.0, 18.0)],
                104,
                "circular",
            ),
            (
                vec![token("line", 100.0, 36.0), token("ar", 138.0, 18.0)],
                104,
                "linear",
            ),
            (
                vec![token("be", 100.0, 18.0), token("ing", 120.0, 27.0)],
                102,
                "being",
            ),
            (
                vec![token("ur", 100.0, 18.0), token("ban", 120.0, 27.0)],
                102,
                "urban",
            ),
            (
                vec![token("identifica", 100.0, 90.0), token("tion", 192.0, 36.0)],
                104,
                "identification",
            ),
            (
                vec![token("w", 100.0, 8.0), token("ar", 110.0, 18.0)],
                102,
                "war",
            ),
            (
                vec![token("m", 100.0, 8.0), token("ining", 110.0, 45.0)],
                102,
                "mining",
            ),
        ];
        for (tokens, pointer_x, expected) in fragment_cases {
            let selected = tokens
                .iter()
                .position(|token| token.bounds.contains_with_margin(pointer_x, 110, 1.0))
                .expect("pointer must be inside the selected token");
            let generated = generate_ocr_candidates_at(&tokens, selected, pointer_x, 110, None);
            let got = choose_best(rank_generated(&generated, fake_evidence))
                .map(|candidate| candidate.text);
            assert_eq!(got.as_deref(), Some(expected));
            checked += 1;
        }
        let normal = [token("As", 100.0, 20.0), token("being", 122.0, 45.0)];
        assert_eq!(best_at(&normal, 108).unwrap().text, "As");

        // Nothing here is a word: list letters, numbering, punctuation, blank.
        for raw in [
            "A)", "B)", "C)", "D)", "1)", "2)", "3)", "...", "--", "·", "…", "123", "_", "|", "'",
            "\"", "()", "[]", "{}", "0",
        ] {
            let width = (raw.chars().count() * 9 + 12) as f32;
            let tokens = [token(raw, 100.0, width)];
            assert!(best_at(&tokens, 104).is_none(), "case: {raw}");
            checked += 1;
        }

        assert_eq!(
            checked, 100,
            "the regression table should stay at 100 cases"
        );
    }

    #[test]
    fn keeps_a_true_unknown_word_only_when_the_raw_glyphs_are_complete() {
        let tokens = [token("nanogenerator", 100.0, 120.0)];
        let best = best_at(&tokens, 140).unwrap();
        assert_eq!(best.text, "nanogenerator");
        assert!(!best.dictionary_hit);
        assert!(best.confidence >= 0.68);
    }
}
