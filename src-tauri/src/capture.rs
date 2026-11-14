use std::{
    cell::RefCell,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use crate::recognition::{
    self, GeneratedCandidate, OcrBox, OcrToken, RecognitionCandidate, WordEvidence,
};
use crate::word_probe::{self, WordProbeReport};
use crate::{
    AppState, TranslationRequest, dictionary::clean_lookup_word, translate_selection,
    translation::TranslationKind,
};

const STABLE_RADIUS: i32 = 5;
const PREFLIGHT_MS: u64 = 100;
const WORD_ANCHOR_RADIUS: i32 = 24;
const DRAG_THRESHOLD: i32 = 7;

static EVENT_SENDER: OnceLock<Sender<MouseInput>> = OnceLock::new();
static TRANSLATION_OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiaOcrDecision {
    KeepUia,
    UseOcr,
    ClearUia,
}

fn decide_uia_ocr(
    uia_text: &str,
    best: &RecognitionCandidate,
    visual_prefix: bool,
    uia_evidence: &WordEvidence,
) -> UiaOcrDecision {
    if visual_prefix {
        // OCR seeing exactly the same word as UIA is agreement, not a visual
        // prefix. Chrome can flag a whole italic word as a continuation when
        // the neighbouring quote/space is close enough to produce ink beside
        // the token. Only discard the UIA word when the OCR token agrees but
        // the dictionary cannot confirm the result.
        if !uia_text.eq_ignore_ascii_case(&best.text) || uia_evidence.dictionary_hit {
            return UiaOcrDecision::KeepUia;
        }
        return UiaOcrDecision::ClearUia;
    }
    let uia_lower = uia_text.to_lowercase();
    let ocr_lower = best.text.to_lowercase();
    let uia_has_punctuation = uia_lower
        .chars()
        .any(|character| !character.is_ascii_alphabetic() && character != '\'' && character != '-');
    let ocr_extends =
        ocr_lower.chars().count() > uia_lower.chars().count() && ocr_lower.starts_with(&uia_lower);
    let uia_needs_help = !uia_evidence.dictionary_hit && uia_lower.chars().count() > 1;
    let confident = best.confidence >= 0.80 && best.dictionary_hit;
    if ocr_lower != uia_lower
        && (!uia_has_punctuation || ocr_extends)
        && (ocr_extends || (uia_needs_help && confident))
    {
        UiaOcrDecision::UseOcr
    } else {
        UiaOcrDecision::KeepUia
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    Hidden,
    Word,
    Translation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy)]
struct MouseInput {
    kind: MouseInputKind,
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MouseInputKind {
    Move,
    Down,
    Up,
    Escape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HoverCandidate {
    Word { text: String, source: String },
    Ocr { x: i32, y: i32 },
}

struct OcrCompletion {
    generation: u64,
    point: (i32, i32),
    started: Instant,
    report: WordProbeReport,
    candidates: Vec<GeneratedCandidate>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SelectionPayload {
    pub request_id: String,
    pub text: String,
    pub source: String,
    pub source_language: String,
    pub target_language: String,
}

pub fn start(app: tauri::AppHandle) {
    let (sender, receiver) = mpsc::channel();
    EVENT_SENDER.set(sender).ok();

    thread::Builder::new()
        .name("eneye-mouse-hook".into())
        .spawn(run_mouse_hook)
        .expect("failed to start mouse hook thread");

    thread::Builder::new()
        .name("eneye-capture-worker".into())
        .spawn(move || run_capture_worker(app, receiver))
        .expect("failed to start capture worker");
}

pub fn ocr_available() -> bool {
    #[cfg(windows)]
    {
        create_ocr_engine().is_some()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn translate_focused_selection(app: tauri::AppHandle) {
    thread::Builder::new()
        .name("eneye-focused-selection".into())
        .spawn(move || {
            #[cfg(windows)]
            {
                let Ok(automation) = uiautomation::UIAutomation::new() else {
                    return;
                };
                let Some(text) = uia_selection_focused(&automation).or_else(copy_selection_text)
                else {
                    return;
                };
                let point = current_cursor_position();
                // 快捷键是用户显式操作，只受「启用 AI 翻译」总开关约束，
                // 不应被「自动翻译选区」影响。
                show_translation_for_text(&app, text, "uia", point, TranslationTrigger::Hotkey);
            }
        })
        .ok();
}

fn run_capture_worker(app: tauri::AppHandle, receiver: mpsc::Receiver<MouseInput>) {
    #[cfg(not(windows))]
    {
        let _ = (app, receiver);
    }

    #[cfg(windows)]
    {
        let automation = match uiautomation::UIAutomation::new() {
            Ok(automation) => automation,
            Err(error) => {
                eprintln!("failed to initialize UI Automation: {error}");
                return;
            }
        };
        let mut pointer = (0, 0);
        let mut stability_anchor = (0, 0);
        let mut stable_since = Instant::now();
        let mut down_at: Option<(i32, i32)> = None;
        let mut down_on_overlay = false;
        let mut preflight: Option<(HoverCandidate, WordProbeReport)> = None;
        let mut resolved_for_dwell = false;
        let mut leave_since: Option<Instant> = None;
        let mut applied_offset = overlay_offset(&app);
        let mut applied_ignore_cursor: Option<bool> = None;
        let (ocr_sender, ocr_receiver) = mpsc::channel::<OcrCompletion>();
        let mut ocr_generation = 0_u64;

        loop {
            while let Ok(completion) = ocr_receiver.try_recv() {
                if completion.generation != ocr_generation
                    || !cursor_still_matches(completion.point)
                {
                    continue;
                }
                let mut report = completion.report;
                let result = resolve_ocr(
                    &app,
                    &mut report,
                    completion.point,
                    completion.started,
                    completion.candidates,
                    completion.error,
                );
                store_probe_report(&app, &report);
                if let Some(result) = result {
                    leave_since = None;
                    show_lookup(&app, result, pointer);
                } else if !keeps_existing_card(&app) {
                    hide_overlay(&app);
                }
                preflight = None;
            }

            let current_offset = overlay_offset(&app);
            if current_offset != applied_offset {
                applied_offset = current_offset;
                if let Some(anchor) = overlay_anchor(&app) {
                    position_overlay(&app, anchor);
                }
            }

            let event = receiver.recv_timeout(Duration::from_millis(20));
            match event {
                Ok(MouseInput {
                    kind: MouseInputKind::Move,
                    x,
                    y,
                }) => {
                    if exceeds_stability_radius(stability_anchor, (x, y)) {
                        pointer = (x, y);
                        stability_anchor = pointer;
                        stable_since = Instant::now();
                        preflight = None;
                        resolved_for_dwell = false;
                        ocr_generation = ocr_generation.wrapping_add(1);
                    } else {
                        pointer = (x, y);
                    }

                    if should_start_leave_timer(
                        overlay_mode(&app),
                        overlay_pinned(&app),
                        overlay_is_visible(&app),
                        point_inside_card(&app, pointer),
                        point_inside_own_process(pointer),
                        pointer,
                        overlay_anchor(&app),
                    ) {
                        leave_since.get_or_insert_with(Instant::now);
                    } else {
                        leave_since = None;
                    }
                }
                Ok(MouseInput {
                    kind: MouseInputKind::Down,
                    x,
                    y,
                }) => {
                    pointer = (x, y);
                    stability_anchor = pointer;
                    down_at = Some((x, y));
                    down_on_overlay = overlay_is_visible(&app) && point_inside_card(&app, (x, y));
                    leave_since = None;
                    stable_since = Instant::now();
                    preflight = None;
                    resolved_for_dwell = false;
                    ocr_generation = ocr_generation.wrapping_add(1);
                }
                Ok(MouseInput {
                    kind: MouseInputKind::Up,
                    x,
                    y,
                }) => {
                    pointer = (x, y);
                    let drag_origin = down_at.take();
                    let dragged = drag_origin
                        .map(|(start_x, start_y)| {
                            (x - start_x).abs() >= DRAG_THRESHOLD
                                || (y - start_y).abs() >= DRAG_THRESHOLD
                        })
                        .unwrap_or(false);
                    let end_on_overlay =
                        overlay_is_visible(&app) && point_inside_card(&app, (x, y));
                    let start = drag_origin.unwrap_or((x, y));
                    let selection_blocked = selection_should_be_ignored(
                        down_on_overlay,
                        end_on_overlay,
                        point_inside_own_process(start),
                        point_inside_own_process((x, y)),
                    );
                    if dragged && !selection_blocked {
                        handle_selection(&app, &automation, start, (x, y));
                    }
                    if down_on_overlay {
                        leave_since = None;
                    }
                    down_on_overlay = false;
                    if app
                        .state::<AppState>()
                        .overlay_drag_pending
                        .swap(false, Ordering::AcqRel)
                    {
                        remember_dragged_anchor(&app);
                    }
                    stability_anchor = pointer;
                    stable_since = Instant::now();
                }
                Ok(MouseInput {
                    kind: MouseInputKind::Escape,
                    ..
                }) => {
                    if captures_escape(overlay_mode(&app)) {
                        hide_overlay(&app);
                        leave_since = None;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }

            if let Some(since) = leave_since
                && should_hide_after_leave(
                    down_at.is_some(),
                    since.elapsed(),
                    word_linger(&app),
                    overlay_pinned(&app),
                    point_inside_card(&app, pointer),
                )
            {
                hide_overlay(&app);
                leave_since = None;
            }

            sync_overlay_cursor_passthrough(
                &app,
                pointer,
                down_at.is_some(),
                &mut applied_ignore_cursor,
            );

            if down_at.is_some()
                || point_inside_own_process(pointer)
                || (overlay_is_visible(&app) && point_inside_card(&app, pointer))
                || overlay_mode(&app) == OverlayMode::Translation
            {
                continue;
            }

            let elapsed = stable_since.elapsed().as_millis() as u64;
            let delay = lookup_delay(&app);
            if elapsed >= PREFLIGHT_MS && preflight.is_none() && !resolved_for_dwell {
                let probe_point = pointer;
                let mut accessible = word_probe::probe_accessible(&automation, pointer);
                if !cursor_still_matches(probe_point) {
                    pointer = current_cursor_position();
                    stability_anchor = pointer;
                    stable_since = Instant::now();
                    preflight = None;
                    resolved_for_dwell = false;
                    continue;
                }
                // UIA often returns the first fragment of a word split across a
                // line break. Verify the visible pixels and prefer a longer OCR
                // word that extends the UIA word.
                if let Some((uia_text, _)) = accessible.word.clone()
                    && ocr_available()
                    && let Ok(candidates) = ocr_word_candidates_at(pointer.0, pointer.1)
                {
                    let ranked = rank_ocr_candidates(&app, &candidates);
                    let best = recognition::choose_best(ranked.clone());
                    accessible.report.candidates = ranked;
                    if let Some(best) = best {
                        let visual_prefix = candidates.iter().any(|candidate| {
                            candidate.visual_continuation
                                && candidate.text.eq_ignore_ascii_case(&best.text)
                        });
                        let uia_evidence = dictionary_evidence(&app, &uia_text);
                        match decide_uia_ocr(&uia_text, &best, visual_prefix, &uia_evidence) {
                            UiaOcrDecision::ClearUia => accessible.word = None,
                            UiaOcrDecision::UseOcr => {
                                accessible.word = Some((best.text.clone(), "ocr"));
                                accessible.report.word = Some(best.text.clone());
                                accessible.report.source = Some("ocr".into());
                                accessible.report.confidence = Some(best.confidence);
                                accessible.report.reason = Some(best.reason.clone());
                            }
                            UiaOcrDecision::KeepUia => {}
                        }
                    }
                }

                let exact_text_control = accessible.exact_text_control;
                let no_word_report = accessible.report.clone();
                let candidate = match accessible.word {
                    None if exact_text_control => {
                        if let Ok(mut last_probe) = app.state::<AppState>().last_word_probe.lock() {
                            *last_probe = no_word_report;
                        }
                        preflight = None;
                        resolved_for_dwell = true;
                        if !keeps_existing_card(&app) {
                            hide_overlay(&app);
                        }
                        continue;
                    }
                    Some((text, "msaa")) => HoverCandidate::Word {
                        text,
                        source: "msaa".into(),
                    },
                    Some((text, "win32")) => HoverCandidate::Word {
                        text,
                        source: "win32".into(),
                    },
                    Some((text, "ocr")) => HoverCandidate::Word {
                        text,
                        source: "ocr".into(),
                    },
                    Some((text, _)) => HoverCandidate::Word {
                        text,
                        source: "uia".into(),
                    },
                    None => HoverCandidate::Ocr {
                        x: pointer.0,
                        y: pointer.1,
                    },
                };
                if let Ok(mut last_probe) = app.state::<AppState>().last_word_probe.lock() {
                    *last_probe = accessible.report.clone();
                }
                preflight = Some((candidate, accessible.report));
            }
            if elapsed < delay || resolved_for_dwell {
                continue;
            }
            let Some((candidate, mut report)) = preflight.take() else {
                continue;
            };
            resolved_for_dwell = true;

            let result = match candidate {
                HoverCandidate::Word { text, source } => lookup(&app, &text, &source),
                HoverCandidate::Ocr { x, y } => {
                    if !ocr_available() {
                        word_probe::update_ocr_stage(
                            &mut report,
                            false,
                            Duration::ZERO,
                            "系统没有可用的 OCR 语言包".into(),
                        );
                        store_probe_report(&app, &report);
                        if !keeps_existing_card(&app) {
                            hide_overlay(&app);
                        }
                        continue;
                    }
                    let generation = ocr_generation;
                    let sender = ocr_sender.clone();
                    let thread_report = report.clone();
                    let spawn = thread::Builder::new()
                        .name("eneye-ocr".into())
                        .spawn(move || {
                            let started = Instant::now();
                            let (mut candidates, error) = match ocr_word_candidates_at(x, y) {
                                Ok(candidates) => (candidates, None),
                                Err(error) => (Vec::new(), Some(error.to_string())),
                            };
                            let has_full_raw = candidates.iter().any(|candidate| {
                                candidate.raw_exact && candidate.text.chars().count() >= 4
                            });
                            if !has_full_raw && let Ok(cleaned) = ocr_word_candidates_at_clean(x, y)
                            {
                                candidates.extend(cleaned);
                            }
                            let _ = sender.send(OcrCompletion {
                                generation,
                                point: (x, y),
                                started,
                                report: thread_report,
                                candidates,
                                error,
                            });
                        });
                    if let Err(error) = spawn {
                        word_probe::update_ocr_stage(
                            &mut report,
                            false,
                            Duration::ZERO,
                            format!("OCR 线程启动失败: {error}"),
                        );
                        store_probe_report(&app, &report);
                        if !keeps_existing_card(&app) {
                            hide_overlay(&app);
                        }
                    }
                    continue;
                }
            };
            if !cursor_still_matches(pointer) {
                if !keeps_existing_card(&app) {
                    hide_overlay(&app);
                }
                continue;
            }
            store_probe_report(&app, &report);
            if let Some(result) = result {
                leave_since = None;
                show_lookup(&app, result, pointer);
            } else if !keeps_existing_card(&app) {
                hide_overlay(&app);
            }
            preflight = None;
        }
    }
}

fn store_probe_report(app: &tauri::AppHandle, report: &WordProbeReport) {
    if let Ok(mut last_probe) = app.state::<AppState>().last_word_probe.lock() {
        *last_probe = report.clone();
    }
}

fn dictionary_evidence(app: &tauri::AppHandle, word: &str) -> WordEvidence {
    app.state::<AppState>()
        .dictionary
        .lock()
        .map(|dictionary| dictionary.evidence(word))
        .unwrap_or_default()
}

fn rank_ocr_candidates(
    app: &tauri::AppHandle,
    generated: &[GeneratedCandidate],
) -> Vec<RecognitionCandidate> {
    let state = app.state::<AppState>();
    let Ok(dictionary) = state.dictionary.lock() else {
        return Vec::new();
    };
    recognition::rank_generated(generated, |text| dictionary.evidence(text))
}

fn resolve_ocr(
    app: &tauri::AppHandle,
    report: &mut WordProbeReport,
    point: (i32, i32),
    started: Instant,
    candidates: Vec<GeneratedCandidate>,
    error: Option<String>,
) -> Option<crate::models::LookupResult> {
    report.total_ms = started.elapsed().as_millis() as u64;
    if let Some(error) = error {
        word_probe::update_ocr_stage(report, false, started.elapsed(), error);
        return None;
    }
    if candidates.is_empty() {
        word_probe::update_ocr_stage(
            report,
            false,
            started.elapsed(),
            "OCR 未识别到鼠标下的英文单词".into(),
        );
        return None;
    }
    let mut candidates = candidates;
    if candidates
        .iter()
        .any(|candidate| candidate.visual_continuation)
        && let Ok(extra) = ocr_word_candidates_at_clean(point.0, point.1)
    {
        candidates.extend(extra);
    }
    let ranked = rank_ocr_candidates(app, &candidates);
    report.candidates = ranked.clone();
    let Some(best) = recognition::choose_best(ranked) else {
        report.confidence = None;
        report.reason = None;
        word_probe::update_ocr_stage(
            report,
            false,
            started.elapsed(),
            "OCR 候选都不可靠，已隐藏".into(),
        );
        return None;
    };
    if candidates.iter().any(|candidate| {
        candidate.visual_continuation && candidate.text.eq_ignore_ascii_case(&best.text)
    }) {
        report.word = None;
        report.source = None;
        report.confidence = Some(best.confidence);
        report.reason = Some(format!(
            "{}；右侧仍有字形，当前只是词的前半段，已隐藏",
            best.reason
        ));
        word_probe::update_ocr_stage(
            report,
            false,
            started.elapsed(),
            "词形向右延伸，前缀不完整，已隐藏".into(),
        );
        return None;
    }
    let confidence = best.confidence;
    let reason = best.reason.clone();
    let mut word = best.text;
    let mut lookup_result = lookup(app, &word, "ocr");
    if lookup_result
        .as_ref()
        .is_none_or(|result| result.entries.is_empty())
        && let Some(corrected) = correct_ocr_word_from_title(&word, point.0, point.1)
        && corrected != word.to_ascii_lowercase()
    {
        word = corrected;
        lookup_result = lookup(app, &word, "ocr");
    }
    let Some(lookup_result) = lookup_result else {
        report.word = None;
        report.source = None;
        report.confidence = Some(confidence);
        report.reason = Some(format!("{reason}；候选不是可查询词，已隐藏"));
        word_probe::update_ocr_stage(
            report,
            false,
            started.elapsed(),
            "候选不是可查询词，已隐藏".into(),
        );
        return None;
    };
    let known = !lookup_result.entries.is_empty();
    let outcome = if known { "命中" } else { "识别" };
    word_probe::update_ocr_stage(
        report,
        true,
        started.elapsed(),
        format!("{outcome} {word} · {:.0}%", confidence * 100.0),
    );
    report.word = Some(word);
    report.source = Some("ocr".into());
    report.confidence = Some(confidence);
    report.reason = Some(reason);
    Some(lookup_result)
}

fn find_lookup(
    app: &tauri::AppHandle,
    word: &str,
    source: &str,
) -> Option<crate::models::LookupResult> {
    let clean = clean_lookup_word(word).ok()?;
    app.state::<AppState>()
        .dictionary
        .lock()
        .ok()?
        .lookup(&clean, source)
        .ok()
}

fn lookup(app: &tauri::AppHandle, word: &str, source: &str) -> Option<crate::models::LookupResult> {
    let result = find_lookup(app, word, source)?;
    if !result.entries.is_empty()
        && let Ok(storage) = app.state::<AppState>().storage.lock()
    {
        let _ = storage.record_lookup(&result);
    }
    Some(result)
}

fn anchor_from_card_top_left(bounds: OverlayBounds, offset: (i32, i32)) -> (i32, i32) {
    (bounds.x - offset.0, bounds.y - offset.1)
}

fn remember_dragged_anchor(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };
    let (Ok(position), Ok(scale)) = (window.outer_position(), window.scale_factor()) else {
        return;
    };
    let padding = (CARD_STAGE_PADDING * scale).round() as i32;
    let card = OverlayBounds {
        x: position.x + padding,
        y: position.y + padding,
        width: 1,
        height: 1,
    };
    let anchor = anchor_from_card_top_left(card, overlay_offset(app));
    if let Ok(mut current) = app.state::<AppState>().overlay_anchor.lock() {
        *current = Some(anchor);
    }
}

fn overlay_offset(app: &tauri::AppHandle) -> (i32, i32) {
    app.state::<AppState>()
        .settings
        .lock()
        .map(|settings| {
            (
                settings.overlay_offset_x_px as i32,
                settings.overlay_offset_y_px as i32,
            )
        })
        .unwrap_or((20, 20))
}

fn lookup_delay(app: &tauri::AppHandle) -> u64 {
    app.state::<AppState>()
        .settings
        .lock()
        .map(|settings| settings.lookup_delay_ms)
        .unwrap_or(240)
}

fn word_linger(app: &tauri::AppHandle) -> u64 {
    app.state::<AppState>()
        .settings
        .lock()
        .map(|settings| settings.word_card_linger_ms)
        .unwrap_or(1500)
}

fn overlay_mode(app: &tauri::AppHandle) -> OverlayMode {
    app.state::<AppState>()
        .overlay_mode
        .lock()
        .map(|mode| *mode)
        .unwrap_or(OverlayMode::Hidden)
}

fn overlay_anchor(app: &tauri::AppHandle) -> Option<(i32, i32)> {
    app.state::<AppState>()
        .overlay_anchor
        .lock()
        .ok()
        .and_then(|anchor| *anchor)
}

fn set_overlay_state(app: &tauri::AppHandle, mode: OverlayMode, anchor: Option<(i32, i32)>) {
    let state = app.state::<AppState>();
    if let Ok(mut current_mode) = state.overlay_mode.lock() {
        *current_mode = mode;
    }
    if let Ok(mut current_anchor) = state.overlay_anchor.lock() {
        *current_anchor = anchor;
    }
    TRANSLATION_OVERLAY_ACTIVE.store(captures_escape(mode), Ordering::Release);
}

fn overlay_pinned(app: &tauri::AppHandle) -> bool {
    app.state::<AppState>()
        .overlay_pinned
        .load(Ordering::Relaxed)
}

fn show_lookup(app: &tauri::AppHandle, lookup: crate::models::LookupResult, point: (i32, i32)) {
    app.state::<AppState>()
        .overlay_drag_pending
        .store(false, Ordering::Release);
    set_overlay_state(app, OverlayMode::Word, Some(point));
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.emit("eneye://lookup", lookup);
        position_overlay(app, point);
        let _ = window.show();
        force_show_overlay(&window);
    }
}

fn show_selection(app: &tauri::AppHandle, payload: SelectionPayload, point: (i32, i32)) {
    app.state::<AppState>()
        .overlay_drag_pending
        .store(false, Ordering::Release);
    set_overlay_state(app, OverlayMode::Translation, Some(point));
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.emit("eneye://selection", payload.clone());
        position_overlay(app, point);
        let _ = window.show();
        force_show_overlay(&window);
    }

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();
        let request = TranslationRequest {
            request_id: payload.request_id,
            text: payload.text,
            source_language: payload.source_language,
            target_language: payload.target_language,
            source: payload.source,
            kind: TranslationKind::Translate,
        };
        let _ = translate_selection(app_handle.clone(), request, state).await;
    });
}

fn keeps_existing_card(app: &tauri::AppHandle) -> bool {
    keeps_card(
        overlay_mode(app),
        overlay_is_visible(app),
        overlay_pinned(app),
    )
}

fn keeps_card(mode: OverlayMode, visible: bool, pinned: bool) -> bool {
    visible && !pinned && matches!(mode, OverlayMode::Word | OverlayMode::Translation)
}

fn hide_overlay(app: &tauri::AppHandle) {
    let can_hide = !overlay_pinned(app) || overlay_mode(app) == OverlayMode::Translation;
    if let Some(window) = app.get_webview_window("overlay")
        && can_hide
    {
        let _ = window.hide();
        app.state::<AppState>()
            .overlay_drag_pending
            .store(false, Ordering::Release);
        set_overlay_state(app, OverlayMode::Hidden, None);
        app.state::<AppState>()
            .overlay_pinned
            .store(false, Ordering::Relaxed);
    }
}

pub(crate) fn mark_overlay_hidden() {
    TRANSLATION_OVERLAY_ACTIVE.store(false, Ordering::Release);
}

fn captures_escape(mode: OverlayMode) -> bool {
    mode == OverlayMode::Translation
}

const CARD_STAGE_PADDING: f64 = 8.0;
const DEFAULT_CARD_WIDTH: f64 = 360.0;
const DEFAULT_CARD_HEIGHT: f64 = 520.0;
const CARD_MIN_HEIGHT: f64 = 120.0;
const CARD_MAX_HEIGHT: f64 = 520.0;
const CARD_GAP: i32 = 12;

#[cfg(windows)]
fn monitor_work_area(point: (i32, i32)) -> Option<(i32, i32, i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };

    unsafe {
        let monitor = MonitorFromPoint(
            POINT {
                x: point.0,
                y: point.1,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        if monitor.is_invalid() {
            return None;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        Some((
            info.rcWork.left,
            info.rcWork.top,
            info.rcWork.right,
            info.rcWork.bottom,
        ))
    }
}

#[cfg(not(windows))]
fn monitor_work_area(_point: (i32, i32)) -> Option<(i32, i32, i32, i32)> {
    None
}

#[cfg(windows)]
fn monitor_scale(point: (i32, i32)) -> Option<f64> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    let monitor = unsafe {
        MonitorFromPoint(
            POINT {
                x: point.0,
                y: point.1,
            },
            MONITOR_DEFAULTTONEAREST,
        )
    };
    if monitor.is_invalid() {
        return None;
    }
    let mut dpi_x = 96u32;
    let mut dpi_y = 96u32;
    unsafe {
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).ok()?;
    }
    if dpi_x == 0 {
        return None;
    }
    Some(f64::from(dpi_x) / 96.0)
}

#[cfg(not(windows))]
fn monitor_scale(_point: (i32, i32)) -> Option<f64> {
    None
}

fn physical_metrics(
    scale: f64,
    card_height: u32,
    offset: (i32, i32),
) -> (i32, (u32, u32), (i32, i32)) {
    let padding = (CARD_STAGE_PADDING * scale).round() as i32;
    let card = (
        (DEFAULT_CARD_WIDTH * scale).round() as u32,
        (f64::from(card_height) * scale).round() as u32,
    );
    let offset = (
        (f64::from(offset.0) * scale).round() as i32,
        (f64::from(offset.1) * scale).round() as i32,
    );
    (padding, card, offset)
}

fn place_card(
    point: (i32, i32),
    card: (u32, u32),
    offset: (i32, i32),
    work_area: (i32, i32, i32, i32),
) -> (i32, i32) {
    let (left, top, right, bottom) = work_area;
    let width = card.0 as i32;
    let height = card.1 as i32;
    let mut x = point.0 + offset.0;
    let mut y = point.1 + offset.1;
    if x + width + CARD_GAP > right {
        x = point.0 - width - offset.0;
    }
    if y + height + CARD_GAP > bottom {
        y = point.1 - height - offset.1;
    }
    let max_x = (right - width - CARD_GAP).max(left + CARD_GAP);
    let max_y = (bottom - height - CARD_GAP).max(top + CARD_GAP);
    (
        x.clamp(left + CARD_GAP, max_x),
        y.clamp(top + CARD_GAP, max_y),
    )
}

pub(crate) fn position_overlay(app: &tauri::AppHandle, point: (i32, i32)) {
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };
    // Use the target monitor's DPI, not the window's current monitor: moving an
    // existing overlay to a 150% screen must scale the offset and card metrics
    // even before the window has physically arrived there.
    let scale = monitor_scale(point)
        .or_else(|| window.scale_factor().ok())
        .unwrap_or(1.0);
    let stored = app
        .state::<AppState>()
        .overlay_card_bounds
        .lock()
        .ok()
        .and_then(|bounds| *bounds);
    // Card height is reported in CSS pixels by the overlay. The window is sized in
    // logical pixels as well, so the WebView keeps a stable CSS viewport on every DPI.
    let card_height = stored
        .map(|bounds| {
            bounds
                .height
                .clamp(CARD_MIN_HEIGHT as u32, CARD_MAX_HEIGHT as u32)
        })
        .unwrap_or(DEFAULT_CARD_HEIGHT as u32);
    let (padding, physical_card, physical_offset) =
        physical_metrics(scale, card_height, overlay_offset(app));
    let window_size = tauri::LogicalSize::new(
        DEFAULT_CARD_WIDTH + CARD_STAGE_PADDING * 2.0,
        card_height as f64 + CARD_STAGE_PADDING * 2.0,
    );
    let _ = window.set_size(window_size);
    let work_area = monitor_work_area(point).unwrap_or((0, 0, 1920, 1080));
    let (card_x, card_y) = place_card(point, physical_card, physical_offset, work_area);
    let _ = window.set_position(tauri::PhysicalPosition::new(
        card_x - padding,
        card_y - padding,
    ));
}

pub(crate) fn update_card_bounds(app: &tauri::AppHandle, bounds: Option<OverlayBounds>) {
    let state = app.state::<AppState>();
    let dragging = state.overlay_drag_pending.load(Ordering::Acquire);
    if dragging && let Some(bounds) = bounds {
        let anchor = anchor_from_card_top_left(bounds, overlay_offset(app));
        if let Ok(mut current) = state.overlay_anchor.lock() {
            *current = Some(anchor);
        }
    }
    let previous = state
        .overlay_card_bounds
        .lock()
        .ok()
        .and_then(|current| *current);
    if let Ok(mut current) = state.overlay_card_bounds.lock() {
        *current = bounds;
    }
    // Width is fixed by the stage; only a height change needs a window resize.
    let height_changed = match (previous, bounds) {
        (Some(previous), Some(bounds)) => previous.height != bounds.height,
        (None, Some(_)) => true,
        _ => false,
    };
    if height_changed
        && !dragging
        && overlay_is_visible(app)
        && !overlay_pinned(app)
        && let Some(anchor) = overlay_anchor(app)
    {
        position_overlay(app, anchor);
    }
}

#[cfg(windows)]
fn force_show_overlay(window: &tauri::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_TOPMOST, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        SetWindowPos, ShowWindow,
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

#[cfg(not(windows))]
fn force_show_overlay(_window: &tauri::WebviewWindow) {}

fn overlay_is_visible(app: &tauri::AppHandle) -> bool {
    let Some(window) = app.get_webview_window("overlay") else {
        return false;
    };
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        return unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool() };
    }
    window.is_visible().unwrap_or(false)
}

fn sync_overlay_cursor_passthrough(
    app: &tauri::AppHandle,
    point: (i32, i32),
    pointer_down: bool,
    applied: &mut Option<bool>,
) {
    if pointer_down {
        return;
    }
    let ignore = !overlay_is_visible(app) || !point_inside_card(app, point);
    if *applied == Some(ignore) {
        return;
    }
    if let Some(window) = app.get_webview_window("overlay")
        && window.set_ignore_cursor_events(ignore).is_ok()
    {
        *applied = Some(ignore);
    }
}

fn point_inside_card(app: &tauri::AppHandle, point: (i32, i32)) -> bool {
    let Some(window) = app.get_webview_window("overlay") else {
        return false;
    };
    let (Ok(position), Ok(size), Ok(scale)) = (
        window.outer_position(),
        window.inner_size(),
        window.scale_factor(),
    ) else {
        return false;
    };
    let padding = (CARD_STAGE_PADDING * scale).round() as i32;
    let width = (size.width as i32 - padding * 2).max(1) as u32;
    let height = (size.height as i32 - padding * 2).max(1) as u32;
    point_in_window_bounds(
        (position.x + padding, position.y + padding),
        (width, height),
        point,
    )
}

fn point_in_window_bounds(position: (i32, i32), size: (u32, u32), point: (i32, i32)) -> bool {
    point.0 >= position.0
        && point.1 >= position.1
        && point.0 < position.0 + size.0 as i32
        && point.1 < position.1 + size.1 as i32
}

#[cfg(windows)]
fn point_inside_own_process(point: (i32, i32)) -> bool {
    use windows::Win32::{
        Foundation::POINT,
        UI::WindowsAndMessaging::{GetWindowThreadProcessId, WindowFromPoint},
    };

    let hwnd = unsafe {
        WindowFromPoint(POINT {
            x: point.0,
            y: point.1,
        })
    };
    if hwnd.0.is_null() {
        return false;
    }
    let mut process_id = 0_u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    }
    process_id == std::process::id()
}

#[cfg(not(windows))]
fn point_inside_own_process(_point: (i32, i32)) -> bool {
    false
}

#[cfg(windows)]
fn handle_selection(
    app: &tauri::AppHandle,
    automation: &uiautomation::UIAutomation,
    start: (i32, i32),
    end: (i32, i32),
) {
    // Region drags in a screenshot overlay are not text selections. Running the
    // clipboard fallback there cancels the capture and restores the previous
    // clipboard image a moment later, which looks like "paste is one image
    // behind" in every app.
    if is_screenshot_overlay_at(start) || is_screenshot_overlay_at(end) {
        return;
    }
    if point_inside_own_process(start) || point_inside_own_process(end) {
        return;
    }
    if overlay_is_visible(app) && (point_inside_card(app, start) || point_inside_card(app, end)) {
        return;
    }
    if let Some(text) = uia_selection(automation, start) {
        show_translation_for_text(app, text, "uia", end, TranslationTrigger::DragSelect);
    } else if let Some(text) = copy_selection_text() {
        show_translation_for_text(app, text, "clipboard", end, TranslationTrigger::DragSelect);
    } else if let Some(text) = ocr_selection(start, end) {
        show_translation_for_text(app, text, "ocr", end, TranslationTrigger::DragSelect);
    }
}

#[cfg(not(windows))]
fn handle_selection(
    _app: &tauri::AppHandle,
    _automation: &uiautomation::UIAutomation,
    _start: (i32, i32),
    _end: (i32, i32),
) {
}

#[cfg(windows)]
fn uia_selection(automation: &uiautomation::UIAutomation, point: (i32, i32)) -> Option<String> {
    use uiautomation::{patterns::UITextPattern, types::Point};

    let point = Point::new(point.0, point.1);
    let walker = automation.get_control_view_walker().ok()?;
    let mut element = automation.element_from_point(point).ok()?;
    for _ in 0..7 {
        if element.is_password().unwrap_or(false) {
            return None;
        }
        if let Ok(pattern) = element.get_pattern::<UITextPattern>()
            && let Ok(ranges) = pattern.get_selection()
        {
            let text = ranges
                .iter()
                .filter_map(|range| range.get_text(2000).ok())
                .collect::<String>();
            if !text.trim().is_empty() {
                return Some(text.trim().to_owned());
            }
        }
        element = walker.get_parent(&element).ok()?;
    }
    None
}

#[cfg(windows)]
fn uia_selection_focused(automation: &uiautomation::UIAutomation) -> Option<String> {
    use uiautomation::patterns::UITextPattern;

    let walker = automation.get_control_view_walker().ok()?;
    let mut element = automation.get_focused_element().ok()?;
    for _ in 0..7 {
        if element.is_password().unwrap_or(false) {
            return None;
        }
        if let Ok(pattern) = element.get_pattern::<UITextPattern>()
            && let Ok(ranges) = pattern.get_selection()
        {
            let text = ranges
                .iter()
                .filter_map(|range| range.get_text(2000).ok())
                .collect::<String>();
            if !text.trim().is_empty() {
                return Some(text.trim().to_owned());
            }
        }
        element = walker.get_parent(&element).ok()?;
    }
    None
}

/// 选区翻译的触发来源。两种来源的门禁条件不同：
/// - DragSelect：鼠标拖选后自动弹出译文，额外受「自动翻译选区」约束；
/// - Hotkey：用户按 Ctrl+Alt+T 的显式请求，只受「启用 AI 翻译」总开关约束。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranslationTrigger {
    DragSelect,
    Hotkey,
}

/// 判断本次选区翻译是否放行。入参为两个开关的当前值与触发来源。
fn translation_allowed(
    master_enabled: bool,
    auto_translate: bool,
    trigger: TranslationTrigger,
) -> bool {
    match trigger {
        TranslationTrigger::Hotkey => master_enabled,
        TranslationTrigger::DragSelect => master_enabled && auto_translate,
    }
}

fn show_translation_for_text(
    app: &tauri::AppHandle,
    text: String,
    source: &str,
    point: (i32, i32),
    trigger: TranslationTrigger,
) {
    if !is_usable_selection(&text) {
        return;
    }
    let settings = app
        .state::<AppState>()
        .settings
        .lock()
        .map(|settings| settings.clone())
        .unwrap_or_default();
    if !translation_allowed(
        settings.ai_translation_enabled,
        settings.auto_translate,
        trigger,
    ) {
        return;
    }
    hide_overlay(app);
    show_selection(
        app,
        SelectionPayload {
            request_id: uuid::Uuid::new_v4().to_string(),
            text,
            source: source.into(),
            source_language: settings.source_language,
            target_language: settings.target_language,
        },
        point,
    );
}

#[cfg(windows)]
fn current_cursor_position() -> (i32, i32) {
    let mut point = windows::Win32::Foundation::POINT::default();
    if unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) }.is_ok() {
        (point.x, point.y)
    } else {
        (100, 100)
    }
}

#[cfg(windows)]
fn copy_selection_text() -> Option<String> {
    use uiautomation::{clipboards::Clipboard, inputs::Keyboard};

    let snapshot = Clipboard::open().ok()?.snapshot(true).ok()?;
    let sequence_before = clipboard_sequence();
    let keyboard = Keyboard::new();
    keyboard.send_keys("{ctrl}c").ok()?;
    thread::sleep(Duration::from_millis(70));
    let text = Clipboard::open()
        .ok()
        .and_then(|clipboard| clipboard.get_text().ok());
    let sequence_after = clipboard_sequence();
    if should_restore_clipboard(
        sequence_before,
        sequence_after,
        clipboard_sequence(),
        text.as_deref(),
    ) && let Ok(clipboard) = Clipboard::open()
    {
        let _ = clipboard.restore(snapshot);
    }
    text.filter(|text| !text.trim().is_empty())
}

/// Restore the user's original clipboard only while it still contains the text
/// our synthetic Ctrl+C just copied. If another program (for example the
/// screenshot tool) wrote to the clipboard meanwhile, that newer content wins.
fn should_restore_clipboard(
    sequence_before: u32,
    sequence_after: u32,
    sequence_now: u32,
    text: Option<&str>,
) -> bool {
    text.is_some_and(|text| !text.trim().is_empty())
        && sequence_after != sequence_before
        && sequence_now == sequence_after
}

fn is_screenshot_process(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    matches!(
        name,
        "screenclippinghost"
            | "snippingtool"
            | "screensketch"
            | "snipaste"
            | "pixpin"
            | "sharex"
            | "greenshot"
            | "picpick"
    )
}

#[cfg(windows)]
fn is_screenshot_overlay_at(point: (i32, i32)) -> bool {
    process_name_at(point).is_some_and(|name| is_screenshot_process(&name))
}

#[cfg(windows)]
fn process_name_at(point: (i32, i32)) -> Option<String> {
    use windows::{
        Win32::{
            Foundation::{CloseHandle, POINT},
            System::Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
            UI::WindowsAndMessaging::{GetWindowThreadProcessId, WindowFromPoint},
        },
        core::PWSTR,
    };

    let hwnd = unsafe {
        WindowFromPoint(POINT {
            x: point.0,
            y: point.1,
        })
    };
    if hwnd.0.is_null() {
        return None;
    }
    let mut process_id = 0_u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    }
    if process_id == 0 {
        return None;
    }
    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let mut buffer = [0_u16; 1024];
    let mut length = buffer.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    result.ok()?;
    let path = String::from_utf16_lossy(&buffer[..length as usize]);
    Some(path.rsplit(['\\', '/']).next()?.to_ascii_lowercase())
}

#[cfg(windows)]
fn clipboard_sequence() -> u32 {
    unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() }
}

#[cfg(not(windows))]
fn copy_selection_text() -> Option<String> {
    None
}

fn is_usable_selection(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 2000 {
        return false;
    }
    let latin_words = text
        .split(|character: char| !character.is_ascii_alphabetic() && character != '\'')
        .filter(|word| word.len() >= 2)
        .count();
    let cjk = text
        .chars()
        .filter(|character| {
            matches!(
                *character as u32,
                0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff
            )
        })
        .count();
    latin_words >= 2 || cjk >= 4
}

fn exceeds_stability_radius(previous: (i32, i32), current: (i32, i32)) -> bool {
    (current.0 - previous.0).abs() > STABLE_RADIUS || (current.1 - previous.1).abs() > STABLE_RADIUS
}

#[cfg(windows)]
fn cursor_still_matches(expected: (i32, i32)) -> bool {
    !exceeds_stability_radius(expected, current_cursor_position())
}

#[cfg(not(windows))]
fn cursor_still_matches(_expected: (i32, i32)) -> bool {
    true
}

fn outside_word_anchor(pointer: (i32, i32), anchor: Option<(i32, i32)>) -> bool {
    anchor
        .map(|anchor| {
            (pointer.0 - anchor.0).abs() > WORD_ANCHOR_RADIUS
                || (pointer.1 - anchor.1).abs() > WORD_ANCHOR_RADIUS
        })
        .unwrap_or(true)
}

fn should_hide_after_leave(
    dragging: bool,
    elapsed: Duration,
    linger_ms: u64,
    pinned: bool,
    inside_overlay: bool,
) -> bool {
    !dragging && !pinned && !inside_overlay && elapsed >= Duration::from_millis(linger_ms)
}

#[allow(clippy::too_many_arguments)]
fn selection_should_be_ignored(
    down_on_overlay: bool,
    end_on_overlay: bool,
    start_in_own_process: bool,
    end_in_own_process: bool,
) -> bool {
    down_on_overlay || end_on_overlay || start_in_own_process || end_in_own_process
}

fn should_start_leave_timer(
    mode: OverlayMode,
    pinned: bool,
    visible: bool,
    inside_overlay: bool,
    inside_own_process: bool,
    pointer: (i32, i32),
    anchor: Option<(i32, i32)>,
) -> bool {
    mode == OverlayMode::Word
        && !pinned
        && visible
        && !inside_overlay
        && !inside_own_process
        && outside_word_anchor(pointer, anchor)
}

#[cfg(windows)]
fn ocr_word_candidates_at(x: i32, y: i32) -> anyhow::Result<Vec<GeneratedCandidate>> {
    ocr_word_candidates_at_with(x, y, false)
}

#[cfg(windows)]
fn ocr_word_candidates_at_clean(x: i32, y: i32) -> anyhow::Result<Vec<GeneratedCandidate>> {
    ocr_word_candidates_at_with(x, y, true)
}

#[cfg(windows)]
fn ocr_word_candidates_at_with(
    x: i32,
    y: i32,
    strip_small_glyphs: bool,
) -> anyhow::Result<Vec<GeneratedCandidate>> {
    use uiautomation::{screenshots::Screenshot, types::Rect};

    const OCR_SCALE: f32 = 2.0;
    let (virtual_left, virtual_top, virtual_width, virtual_height) = virtual_screen_bounds();
    let left = (x - 600).max(virtual_left);
    let top = (y - 80).max(virtual_top);
    let right = (x + 220).min(virtual_left + virtual_width);
    let bottom = (y + 80).min(virtual_top + virtual_height);
    let rect = Rect::new(left, top, right, bottom);
    let screenshot = Screenshot::capture_rect(rect)?;
    let raw = screenshot.to_bmp_bytes();
    let raw = if strip_small_glyphs {
        strip_small_ocr_glyphs(&raw)?
    } else {
        raw
    };
    let bmp = upscale_ocr_bitmap(&raw, OCR_SCALE as u32)?;
    let result = recognize_bmp(&bmp)?;
    let mut lines: Vec<Vec<OcrToken>> = Vec::new();
    let mut selected = None;
    for line in result.Lines()? {
        let mut tokens = Vec::new();
        for word in line.Words()? {
            let bounds = word.BoundingRect()?;
            let token = OcrToken {
                text: word.Text()?.to_string(),
                bounds: OcrBox {
                    x: left as f32 + bounds.X / OCR_SCALE,
                    y: top as f32 + bounds.Y / OCR_SCALE,
                    width: bounds.Width / OCR_SCALE,
                    height: bounds.Height / OCR_SCALE,
                },
            };
            if selected.is_none() && token.bounds.contains_with_margin(x, y, 4.0) {
                selected = Some((lines.len(), tokens.len()));
            }
            tokens.push(token);
        }
        lines.push(tokens);
    }

    let Some((line_index, word_index)) = selected else {
        return Ok(Vec::new());
    };
    let next_line = lines.get(line_index + 1).map(Vec::as_slice);
    let mut candidates =
        recognition::generate_ocr_candidates_at(&lines[line_index], word_index, x, y, next_line);
    let token = &lines[line_index][word_index];
    let token_letters = token
        .text
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .count();
    let ink_left = has_letter_ink_left(&bmp, left, top, OCR_SCALE, token);
    let ink_right = has_letter_ink_right(&bmp, left, top, OCR_SCALE, token);
    for candidate in &mut candidates {
        let candidate_letters = candidate
            .text
            .chars()
            .filter(|character| character.is_ascii_alphabetic())
            .count();
        // A token that starts mid-word cannot be repaired by looking only to
        // its right. A token that ends mid-word may still be extended by a
        // merge/hyphen candidate, so only same-width candidates are marked.
        if ink_left || (ink_right && candidate_letters <= token_letters) {
            candidate.visual_continuation = true;
        }
    }
    Ok(candidates)
}

#[cfg(windows)]
fn virtual_screen_bounds() -> (i32, i32, i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    let left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let top = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    if width <= 0 || height <= 0 {
        (0, 0, 1920, 1080)
    } else {
        (left, top, width, height)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InkSide {
    Left,
    Right,
}

/// True when glyph ink starts immediately beside the OCR token and is tall/wide
/// enough to be a letter, not a quote, period, or underline.
#[cfg(windows)]
fn has_letter_ink_right(bmp: &[u8], left: i32, top: i32, scale: f32, token: &OcrToken) -> bool {
    has_letter_ink_side(bmp, left, top, scale, token, InkSide::Right)
}

/// True when the OCR token starts in the middle of a longer visible word.
#[cfg(windows)]
fn has_letter_ink_left(bmp: &[u8], left: i32, top: i32, scale: f32, token: &OcrToken) -> bool {
    has_letter_ink_side(bmp, left, top, scale, token, InkSide::Left)
}

#[cfg(windows)]
fn has_letter_ink_side(
    bmp: &[u8],
    left: i32,
    top: i32,
    scale: f32,
    token: &OcrToken,
    side: InkSide,
) -> bool {
    let Ok(image) = image::load_from_memory(bmp) else {
        return false;
    };
    let mut gray = image.to_luma8();
    let (width, height) = gray.dimensions();
    if width < 8 || height < 8 {
        return false;
    }
    let mean = gray.as_raw().iter().map(|value| *value as u64).sum::<u64>()
        / (width as u64 * height as u64);
    if mean < 100 {
        for value in gray.as_mut() {
            *value = 255 - *value;
        }
    }

    let token_height = (token.bounds.height * scale).round().max(8.0) as i32;
    if token_height < 8 {
        return false;
    }
    let y0 = ((token.bounds.y - top as f32) * scale).round() as i32;
    let band_top = y0 + (token_height as f32 * 0.10) as i32;
    let band_bottom = y0 + (token_height as f32 * 0.78) as i32;
    let edge = match side {
        InkSide::Right => ((token.bounds.right() - left as f32) * scale).round() as i32,
        InkSide::Left => ((token.bounds.x - left as f32) * scale).round() as i32,
    };
    if edge < 0 || edge >= width as i32 || band_top < 0 || band_bottom >= height as i32 {
        return false;
    }

    let max_search = (token_height as f32 * 0.55).max(6.0) as i32;
    let mut first_ink = None;
    let mut separated = side == InkSide::Right;
    let start = if side == InkSide::Right { 0 } else { 1 };
    for step in start..max_search {
        let x = match side {
            InkSide::Right => edge + step,
            InkSide::Left => edge - step,
        };
        if x < 0 || x >= width as i32 {
            break;
        }
        let mut count = 0;
        for y in band_top..=band_bottom {
            if gray.get_pixel(x as u32, y as u32)[0] < 160 {
                count += 1;
            }
        }
        if count >= 2 {
            // On the left, ink touching the token box is the first glyph
            // itself; a real preceding glyph must be separated by a gap.
            if separated {
                first_ink = Some(step);
                break;
            }
        } else {
            separated = true;
        }
    }
    let Some(first_ink) = first_ink else {
        return false;
    };
    let max_first_ink = if side == InkSide::Left {
        token_height as f32 * 0.45
    } else {
        token_height as f32 * 0.22
    };
    if first_ink as f32 > max_first_ink {
        return false;
    }

    let extension_width = (token_height as f32 * 0.9) as i32;
    let mut dark_pixels = 0;
    for step in first_ink..first_ink + extension_width {
        let x = match side {
            InkSide::Right => edge + step,
            InkSide::Left => edge - step,
        };
        if x < 0 || x >= width as i32 {
            break;
        }
        for y in band_top..=band_bottom {
            if gray.get_pixel(x as u32, y as u32)[0] < 160 {
                dark_pixels += 1;
            }
        }
    }
    dark_pixels as f32 >= token_height as f32 * 0.45
}

#[cfg(not(windows))]
fn ocr_word_candidates_at(_x: i32, _y: i32) -> anyhow::Result<Vec<GeneratedCandidate>> {
    Ok(Vec::new())
}

#[cfg(not(windows))]
fn ocr_word_candidates_at_clean(_x: i32, _y: i32) -> anyhow::Result<Vec<GeneratedCandidate>> {
    Ok(Vec::new())
}

#[cfg(windows)]
fn ocr_selection(start: (i32, i32), end: (i32, i32)) -> Option<String> {
    use uiautomation::{screenshots::Screenshot, types::Rect};

    let left = start.0.min(end.0) - 16;
    let top = start.1.min(end.1) - 16;
    let right = start.0.max(end.0) + 16;
    let bottom = start.1.max(end.1) + 16;
    let screenshot = Screenshot::capture_rect(Rect::new(left, top, right, bottom)).ok()?;
    let bmp = screenshot.to_bmp_bytes();
    let result = recognize_bmp(&bmp).ok()?;
    let text = result
        .Lines()
        .ok()?
        .into_iter()
        .filter_map(|line| line.Text().ok())
        .map(|text| text.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(not(windows))]
fn ocr_selection(_start: (i32, i32), _end: (i32, i32)) -> Option<String> {
    None
}

#[cfg(windows)]
fn recognize_bmp(bmp: &[u8]) -> anyhow::Result<windows::Media::Ocr::OcrResult> {
    recognize_image_bytes(bmp)
}

#[cfg(windows)]
fn recognize_image_bytes(bytes: &[u8]) -> anyhow::Result<windows::Media::Ocr::OcrResult> {
    use windows::{
        Graphics::Imaging::BitmapDecoder,
        Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
    };

    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::new()?;
    writer.WriteBytes(bytes)?;
    let buffer = writer.DetachBuffer()?;
    stream.WriteAsync(&buffer)?.join()?;
    stream.Seek(0)?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
    let bitmap = decoder.GetSoftwareBitmapAsync()?.join()?;
    OCR_ENGINE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(
                create_ocr_engine()
                    .context("Windows OCR 不可用；请在系统语言设置中安装英文 OCR 可选功能")?,
            );
        }
        let engine = slot.as_ref().expect("OCR 引擎刚刚初始化");
        Ok(engine.RecognizeAsync(&bitmap)?.join()?)
    })
}

#[cfg(windows)]
thread_local! {
    static OCR_ENGINE: RefCell<Option<windows::Media::Ocr::OcrEngine>> = const { RefCell::new(None) };
}

#[cfg(windows)]
fn create_ocr_engine() -> Option<windows::Media::Ocr::OcrEngine> {
    use windows::{Globalization::Language, Media::Ocr::OcrEngine, core::HSTRING};

    if let Ok(language) = Language::CreateLanguage(&HSTRING::from("en-US"))
        && let Ok(engine) = OcrEngine::TryCreateFromLanguage(&language)
    {
        return Some(engine);
    }
    if let Ok(engine) = OcrEngine::TryCreateFromUserProfileLanguages() {
        return Some(engine);
    }
    OcrEngine::AvailableRecognizerLanguages()
        .ok()?
        .into_iter()
        .find_map(|language| OcrEngine::TryCreateFromLanguage(&language).ok())
}

#[cfg(windows)]
fn correct_ocr_word_from_title(word: &str, x: i32, y: i32) -> Option<String> {
    let title = window_title_at(x, y)?;
    correct_ocr_word_against_title(word, &title)
}

#[cfg(not(windows))]
fn correct_ocr_word_from_title(_word: &str, _x: i32, _y: i32) -> Option<String> {
    None
}

#[cfg(windows)]
fn window_title_at(x: i32, y: i32) -> Option<String> {
    use windows::Win32::{
        Foundation::POINT,
        UI::WindowsAndMessaging::{GA_ROOT, GetAncestor, GetWindowTextW, WindowFromPoint},
    };

    let hwnd = unsafe { WindowFromPoint(POINT { x, y }) };
    if hwnd.0.is_null() {
        return None;
    }
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    let hwnd = if root.0.is_null() { hwnd } else { root };
    let mut buffer = [0_u16; 1024];
    let len = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    (len > 0).then(|| String::from_utf16_lossy(&buffer[..len as usize]))
}

fn correct_ocr_word_against_title(word: &str, title: &str) -> Option<String> {
    let target = word.to_ascii_lowercase();
    if target.len() < 5
        || !target
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return None;
    }
    title
        .split(|character: char| !character.is_ascii_alphabetic())
        .filter(|token| token.len() == target.len())
        .map(str::to_ascii_lowercase)
        .find(|token| {
            token != &target
                && token
                    .chars()
                    .zip(target.chars())
                    .filter(|(left, right)| left != right)
                    .count()
                    == 1
        })
}

fn upscale_ocr_bitmap(bytes: &[u8], scale: u32) -> anyhow::Result<Vec<u8>> {
    use image::{ImageFormat, imageops::FilterType};
    use std::io::Cursor;

    let image = image::load_from_memory_with_format(bytes, ImageFormat::Bmp)?;
    let resized = image.resize_exact(
        image.width() * scale,
        image.height() * scale,
        FilterType::Lanczos3,
    );
    let mut output = Cursor::new(Vec::new());
    resized.write_to(&mut output, ImageFormat::Bmp)?;
    Ok(output.into_inner())
}

fn strip_small_ocr_glyphs(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    use image::{DynamicImage, ImageFormat, Luma};
    use std::io::Cursor;

    let source = image::load_from_memory_with_format(bytes, ImageFormat::Bmp)?.to_luma8();
    let (width, height) = source.dimensions();
    if width < 4 || height < 4 {
        return Ok(bytes.to_vec());
    }
    let mut gray = source;
    let mean = gray.as_raw().iter().map(|value| *value as u64).sum::<u64>()
        / (width as u64 * height as u64);
    if mean < 128 {
        for value in gray.as_mut() {
            *value = 255 - *value;
        }
    }

    let mut foreground = vec![false; (width * height) as usize];
    for (index, value) in gray.as_raw().iter().enumerate() {
        foreground[index] = *value < 160;
    }
    let mut visited = vec![false; foreground.len()];
    let mut components = Vec::new();
    for start in 0..foreground.len() {
        if !foreground[start] || visited[start] {
            continue;
        }
        let mut stack = vec![start];
        visited[start] = true;
        let mut pixels = Vec::new();
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (usize::MAX, 0, usize::MAX, 0);
        while let Some(index) = stack.pop() {
            pixels.push(index);
            let x = index % width as usize;
            let y = index / width as usize;
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            for dy in -1_i32..=1 {
                for dx in -1_i32..=1 {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                        continue;
                    }
                    let neighbor = ny as usize * width as usize + nx as usize;
                    if foreground[neighbor] && !visited[neighbor] {
                        visited[neighbor] = true;
                        stack.push(neighbor);
                    }
                }
            }
        }
        components.push((max_y - min_y + 1, max_x - min_x + 1, pixels.len(), pixels));
    }
    let text_height = components
        .iter()
        .filter(|(_, _, area, _)| *area >= 8)
        .map(|(height, _, _, _)| *height)
        .max()
        .unwrap_or(0);
    if text_height == 0 {
        return Ok(bytes.to_vec());
    }
    for (component_height, component_width, area, pixels) in components {
        if component_height < text_height * 62 / 100
            && component_width < text_height * 70 / 100
            && area < text_height * text_height * 35 / 100
        {
            for index in pixels {
                let x = (index % width as usize) as u32;
                let y = (index / width as usize) as u32;
                gray.put_pixel(x, y, Luma([255]));
            }
        }
    }
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(gray).write_to(&mut output, ImageFormat::Bmp)?;
    Ok(output.into_inner())
}

#[cfg(windows)]
fn run_mouse_hook() {
    use windows::Win32::{
        Foundation::{LPARAM, LRESULT, WPARAM},
        UI::Input::KeyboardAndMouse::VK_ESCAPE,
        UI::WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
            MSLLHOOKSTRUCT, SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL, WH_MOUSE_LL,
            WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
        },
    };

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= HC_ACTION as i32 {
            let message = wparam.0 as u32;
            let kind = match message {
                WM_MOUSEMOVE => Some(MouseInputKind::Move),
                WM_LBUTTONDOWN => Some(MouseInputKind::Down),
                WM_LBUTTONUP => Some(MouseInputKind::Up),
                _ => None,
            };
            if let (Some(kind), Some(sender)) = (kind, EVENT_SENDER.get()) {
                let data = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
                let _ = sender.send(MouseInput {
                    kind,
                    x: data.pt.x,
                    y: data.pt.y,
                });
            }
        }
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= HC_ACTION as i32
            && wparam.0 as u32 == WM_KEYDOWN
            && TRANSLATION_OVERLAY_ACTIVE.load(Ordering::Acquire)
        {
            let data = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            if data.vkCode == u32::from(VK_ESCAPE.0) {
                if let Some(sender) = EVENT_SENDER.get() {
                    let _ = sender.send(MouseInput {
                        kind: MouseInputKind::Escape,
                        x: 0,
                        y: 0,
                    });
                }
                return LRESULT(1);
            }
        }
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    let mouse_hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) };
    if let Err(error) = &mouse_hook {
        eprintln!("failed to install mouse hook: {error:?}");
    }
    let keyboard_hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0) };
    if let Err(error) = &keyboard_hook {
        eprintln!("failed to install translation Escape hook: {error:?}");
    }
    let Some(mouse_hook) = mouse_hook.ok() else {
        if keyboard_hook.is_err() {
            return;
        }
        let mut message = MSG::default();
        while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
            let _ = unsafe { TranslateMessage(&message) };
            unsafe { DispatchMessageW(&message) };
        }
        if let Ok(keyboard_hook) = keyboard_hook {
            let _ = unsafe {
                windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(keyboard_hook)
            };
        }
        return;
    };
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
        let _ = unsafe { TranslateMessage(&message) };
        unsafe { DispatchMessageW(&message) };
    }
    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(mouse_hook) };
    if let Ok(keyboard_hook) = keyboard_hook {
        let _ =
            unsafe { windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(keyboard_hook) };
    }
}

#[cfg(not(windows))]
fn run_mouse_hook() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_translation_ignores_the_auto_translate_toggle() {
        assert!(translation_allowed(true, false, TranslationTrigger::Hotkey));
        assert!(translation_allowed(true, true, TranslationTrigger::Hotkey));
    }

    #[test]
    fn drag_translation_still_requires_auto_translate() {
        assert!(!translation_allowed(
            true,
            false,
            TranslationTrigger::DragSelect
        ));
        assert!(translation_allowed(
            true,
            true,
            TranslationTrigger::DragSelect
        ));
    }

    #[test]
    fn master_switch_disables_every_translation_trigger() {
        for trigger in [TranslationTrigger::Hotkey, TranslationTrigger::DragSelect] {
            assert!(!translation_allowed(false, true, trigger));
            assert!(!translation_allowed(false, false, trigger));
        }
    }

    fn ocr_candidate(text: &str, confidence: f64, dictionary_hit: bool) -> RecognitionCandidate {
        RecognitionCandidate {
            text: text.into(),
            provider: "ocr".into(),
            raw_token: Some(text.into()),
            pointer_inside: true,
            pointer_on_letter: true,
            merge_count: 0,
            crossed_space: false,
            crossed_punctuation: false,
            edit_distance: 0,
            dictionary_hit,
            frq: None,
            bnc: None,
            collins: 0,
            oxford: false,
            confidence,
            reason: "test".into(),
        }
    }

    #[test]
    fn uia_complete_word_wins_over_a_visual_ocr_prefix() {
        let evidence = WordEvidence {
            dictionary_hit: true,
            frq: Some(1200),
            bnc: None,
            collins: 4,
            oxford: true,
        };
        let prefix = ocr_candidate("poll", 0.92, true);
        assert_eq!(
            decide_uia_ocr("pollution", &prefix, true, &evidence),
            UiaOcrDecision::KeepUia
        );
        assert_eq!(
            decide_uia_ocr("poll", &prefix, true, &evidence),
            UiaOcrDecision::KeepUia,
            "OCR and UIA agreeing on a dictionary word is not evidence of truncation"
        );
        let unknown = WordEvidence {
            dictionary_hit: false,
            ..evidence
        };
        assert_eq!(
            decide_uia_ocr("poll", &prefix, true, &unknown),
            UiaOcrDecision::ClearUia
        );
        assert_eq!(
            decide_uia_ocr("pollution", &prefix, false, &evidence),
            UiaOcrDecision::KeepUia
        );

        let same = ocr_candidate("being", 0.91, true);
        assert_eq!(
            decide_uia_ocr("being", &same, true, &evidence),
            UiaOcrDecision::KeepUia,
            "a complete italic word must not be discarded when OCR agrees"
        );

        let full = ocr_candidate("pollution", 0.94, true);
        let missing = WordEvidence {
            dictionary_hit: false,
            ..evidence
        };
        assert_eq!(
            decide_uia_ocr("poll", &full, false, &missing),
            UiaOcrDecision::UseOcr
        );
    }

    #[test]
    fn corrects_one_character_ocr_confusion_from_the_window_title() {
        assert_eq!(
            correct_ocr_word_against_title("deepseel", "DeepSeek Harness"),
            Some("deepseek".into())
        );
        assert_eq!(
            correct_ocr_word_against_title("insufficient", "Battery-Free PDF"),
            None
        );
        assert_eq!(
            correct_ocr_word_against_title("cat", "DeepSeek Harness"),
            None
        );
    }

    #[test]
    fn selection_threshold_accepts_sentences_but_not_single_words() {
        assert!(is_usable_selection("A quiet reading room"));
        assert!(is_usable_selection(
            "\u{8fd9}\u{662f}\u{4e00}\u{6bb5}\u{4e2d}\u{6587}"
        ));
        assert!(!is_usable_selection("hello"));
        assert!(!is_usable_selection("1234"));
    }

    #[test]
    fn skips_drag_selection_inside_screenshot_tools() {
        assert!(is_screenshot_process("ScreenClippingHost.exe"));
        assert!(is_screenshot_process("SnippingTool"));
        assert!(is_screenshot_process("PixPin.exe"));
        assert!(!is_screenshot_process("notepad.exe"));
        assert!(!is_screenshot_process("Weixin.exe"));
    }

    #[test]
    fn only_restores_the_clipboard_while_ctrl_c_still_owns_it() {
        assert!(should_restore_clipboard(10, 11, 11, Some("selected text")));
        assert!(!should_restore_clipboard(10, 11, 12, Some("selected text")));
        assert!(!should_restore_clipboard(10, 11, 11, Some("   ")));
        assert!(!should_restore_clipboard(10, 11, 11, None));
        assert!(!should_restore_clipboard(10, 10, 10, Some("old clipboard")));
    }

    #[test]
    fn small_pointer_jitter_does_not_start_a_new_hover_cycle() {
        assert!(!exceeds_stability_radius((100, 100), (103, 104)));
        assert!(exceeds_stability_radius((100, 100), (106, 100)));
        assert!(!exceeds_stability_radius((103, 104), (106, 104)));
        assert!(exceeds_stability_radius((100, 100), (106, 104)));
    }

    #[test]
    fn word_card_keeps_open_while_pointer_remains_near_source_word() {
        assert!(!outside_word_anchor((120, 110), Some((100, 100))));
        assert!(outside_word_anchor((130, 100), Some((100, 100))));
    }

    #[test]
    fn translation_card_never_starts_an_automatic_close_timer() {
        assert!(!should_start_leave_timer(
            OverlayMode::Translation,
            false,
            true,
            false,
            false,
            (400, 400),
            Some((100, 100)),
        ));
        assert!(should_start_leave_timer(
            OverlayMode::Word,
            false,
            true,
            false,
            false,
            (400, 400),
            Some((100, 100)),
        ));
    }

    #[test]
    fn dragging_the_card_never_starts_the_auto_hide_timer() {
        assert!(!should_hide_after_leave(
            true,
            Duration::from_secs(5),
            250,
            false,
            false
        ));
        assert!(should_hide_after_leave(
            false,
            Duration::from_millis(250),
            250,
            false,
            false
        ));
        assert!(!should_hide_after_leave(
            false,
            Duration::from_millis(249),
            250,
            false,
            false
        ));
        assert!(!should_hide_after_leave(
            false,
            Duration::from_secs(5),
            250,
            false,
            true
        ));
        assert!(!should_hide_after_leave(
            false,
            Duration::from_secs(5),
            250,
            true,
            false
        ));
    }

    #[test]
    fn drags_on_the_overlay_never_become_text_selections() {
        assert!(selection_should_be_ignored(true, false, false, false));
        assert!(selection_should_be_ignored(false, true, false, false));
        assert!(selection_should_be_ignored(false, false, true, false));
        assert!(selection_should_be_ignored(false, false, false, true));
        assert!(!selection_should_be_ignored(false, false, false, false));
    }

    #[test]
    fn places_the_card_at_the_requested_dual_axis_offset() {
        let work = (0, 0, 1920, 1080);
        assert_eq!(
            place_card((1000, 300), (320, 400), (20, 20), work),
            (1020, 320)
        );
        assert_eq!(
            place_card((800, 300), (320, 400), (240, 240), work),
            (1040, 540)
        );
    }

    #[test]
    fn scales_card_and_offset_for_the_target_monitor() {
        assert_eq!(
            physical_metrics(1.0, 520, (20, 20)),
            (8, (360, 520), (20, 20))
        );
        assert_eq!(
            physical_metrics(1.5, 520, (20, 20)),
            (12, (540, 780), (30, 30))
        );
        assert_eq!(
            physical_metrics(2.0, 520, (240, 120)),
            (16, (720, 1040), (480, 240))
        );
    }

    #[test]
    fn flips_and_clamps_the_card_at_screen_edges() {
        let work = (0, 0, 1920, 1080);
        assert_eq!(
            place_card((1800, 300), (320, 400), (20, 20), work),
            (1460, 320)
        );
        assert_eq!(
            place_card((800, 900), (320, 400), (20, 20), work),
            (820, 480)
        );
        assert_eq!(place_card((5, 5), (320, 400), (20, 20), work), (25, 25));
    }

    #[test]
    fn overlay_rectangle_owns_the_pointer() {
        let position = (100, 100);
        let size = (336, 536);
        assert!(point_in_window_bounds(position, size, (100, 100)));
        assert!(point_in_window_bounds(position, size, (435, 635)));
        assert!(!point_in_window_bounds(position, size, (99, 100)));
        assert!(!point_in_window_bounds(position, size, (436, 100)));
        assert!(!point_in_window_bounds(position, size, (100, 636)));
    }

    #[test]
    fn transparent_stage_gutter_is_not_part_of_the_card() {
        let card = (100, 100);
        let size = (320, 520);
        assert!(point_in_window_bounds(card, size, (419, 619)));
        assert!(!point_in_window_bounds(card, size, (420, 619)));
        assert!(!point_in_window_bounds(card, size, (419, 620)));
    }

    #[test]
    fn a_dragged_card_becomes_the_new_anchor() {
        let bounds = OverlayBounds {
            x: 500,
            y: 300,
            width: 320,
            height: 520,
        };
        assert_eq!(anchor_from_card_top_left(bounds, (20, 20)), (480, 280));
        assert_eq!(
            place_card(
                anchor_from_card_top_left(bounds, (20, 20)),
                (bounds.width, bounds.height),
                (20, 20),
                (0, 0, 1920, 1080)
            ),
            (500, 300)
        );
    }

    #[test]
    fn places_the_card_on_a_negative_coordinate_monitor() {
        let work = (-1920, 0, -100, 1040);
        assert_eq!(
            place_card((-1500, 200), (320, 400), (20, 20), work),
            (-1480, 220)
        );
        assert_eq!(
            place_card((-350, 200), (320, 400), (20, 20), work),
            (-690, 220)
        );
    }

    #[test]
    fn empty_lookup_keeps_a_visible_card_until_the_linger_timer() {
        assert!(keeps_card(OverlayMode::Word, true, false));
        assert!(keeps_card(OverlayMode::Translation, true, false));
        assert!(!keeps_card(OverlayMode::Hidden, false, false));
        assert!(!keeps_card(OverlayMode::Word, false, false));
        assert!(!keeps_card(OverlayMode::Word, true, true));
    }

    #[test]
    fn escape_is_captured_only_for_the_translation_card() {
        assert!(captures_escape(OverlayMode::Translation));
        assert!(!captures_escape(OverlayMode::Word));
        assert!(!captures_escape(OverlayMode::Hidden));
    }

    #[cfg(windows)]
    #[test]
    fn uses_any_installed_windows_ocr_language() {
        assert!(create_ocr_engine().is_some());
    }

    #[cfg(windows)]
    #[test]
    fn sees_letter_ink_right_of_a_prefix_but_not_a_quote() {
        let image = include_bytes!("../tests/fixtures/ocr-pollution.png");
        let prefix = OcrToken {
            text: "poll".into(),
            bounds: OcrBox {
                x: 22.0,
                y: 18.0,
                width: 60.0,
                height: 33.0,
            },
        };
        assert!(has_letter_ink_right(image, 0, 0, 1.0, &prefix));
        let complete = OcrToken {
            text: "pollution".into(),
            bounds: OcrBox {
                x: 22.0,
                y: 18.0,
                width: 194.0,
                height: 33.0,
            },
        };
        assert!(!has_letter_ink_right(image, 0, 0, 1.0, &complete));
    }

    #[cfg(windows)]
    #[test]
    fn sees_letter_ink_left_when_a_token_starts_mid_word() {
        let image = include_bytes!("../tests/fixtures/ocr-pollution.png");
        let suffix = OcrToken {
            text: "ollution".into(),
            bounds: OcrBox {
                x: 70.0,
                y: 18.0,
                width: 140.0,
                height: 33.0,
            },
        };
        assert!(has_letter_ink_left(image, 0, 0, 1.0, &suffix));
        let complete = OcrToken {
            text: "pollution".into(),
            bounds: OcrBox {
                x: 22.0,
                y: 18.0,
                width: 194.0,
                height: 33.0,
            },
        };
        assert!(!has_letter_ink_left(image, 0, 0, 1.0, &complete));
    }

    #[cfg(windows)]
    #[test]
    fn real_pollution_fixture_never_selects_a_prefix() {
        let result =
            recognize_image_bytes(include_bytes!("../tests/fixtures/ocr-pollution.png")).unwrap();
        let mut tokens = Vec::new();
        for line in result.Lines().unwrap() {
            for word in line.Words().unwrap() {
                let bounds = word.BoundingRect().unwrap();
                tokens.push(OcrToken {
                    text: word.Text().unwrap().to_string(),
                    bounds: OcrBox {
                        x: bounds.X,
                        y: bounds.Y,
                        width: bounds.Width,
                        height: bounds.Height,
                    },
                });
            }
        }
        let selected = tokens
            .iter()
            .position(|token| token.text.to_ascii_lowercase().contains("pollution"))
            .unwrap();
        let y = tokens[selected].bounds.y + tokens[selected].bounds.height / 2.0;
        for pointer in [30.0_f32, 50.0, 80.0, 110.0, 140.0, 165.0] {
            let generated = recognition::generate_ocr_candidates_at(
                &tokens,
                selected,
                pointer.round() as i32,
                y.round() as i32,
                None,
            );
            let ranked = recognition::rank_generated(&generated, |word| {
                let hit = word == "pollution" || word == "poll";
                WordEvidence {
                    dictionary_hit: hit,
                    frq: hit.then_some(1200),
                    bnc: None,
                    collins: if hit { 4 } else { 0 },
                    oxford: hit,
                }
            });
            assert_eq!(
                recognition::choose_best(ranked).map(|candidate| candidate.text),
                Some("pollution".into()),
                "pointer {pointer}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn underlined_pollution_fixture_keeps_the_whole_word() {
        let result = recognize_image_bytes(include_bytes!(
            "../tests/fixtures/ocr-pollution-underlined.png"
        ))
        .unwrap();
        let text = result
            .Lines()
            .unwrap()
            .into_iter()
            .filter_map(|line| line.Text().ok())
            .map(|text| text.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.to_ascii_lowercase().contains("pollution"),
            "Windows OCR did not keep the underlined word: {text:?}"
        );
        let prefix = OcrToken {
            text: "poll".into(),
            bounds: OcrBox {
                x: 22.0,
                y: 18.0,
                width: 60.0,
                height: 33.0,
            },
        };
        assert!(has_letter_ink_right(
            include_bytes!("../tests/fixtures/ocr-pollution-underlined.png"),
            0,
            0,
            1.0,
            &prefix
        ));
    }

    #[cfg(windows)]
    #[test]
    fn notepad_fixture_ocr_recovers_pollution_at_every_letter() {
        let result =
            recognize_image_bytes(include_bytes!("../tests/fixtures/notepad-pollution.png"))
                .unwrap();
        let mut tokens = Vec::new();
        for line in result.Lines().unwrap() {
            for word in line.Words().unwrap() {
                let bounds = word.BoundingRect().unwrap();
                tokens.push(OcrToken {
                    text: word.Text().unwrap().to_string(),
                    bounds: OcrBox {
                        x: bounds.X,
                        y: bounds.Y,
                        width: bounds.Width,
                        height: bounds.Height,
                    },
                });
            }
        }
        let selected = tokens
            .iter()
            .position(|token| {
                token.bounds.x >= 200.0
                    && token.bounds.x < 260.0
                    && token.bounds.y < 30.0
                    && token.text.chars().count() >= 5
            })
            .expect("the pollution OCR token");
        let y = (tokens[selected].bounds.y + tokens[selected].bounds.height / 2.0).round() as i32;
        let left = tokens[selected].bounds.x;
        let width = tokens[selected].bounds.width;
        for fraction in [0.05_f32, 0.15, 0.30, 0.45, 0.60, 0.75, 0.90] {
            let pointer_x = (left + width * fraction).round() as i32;
            let generated =
                recognition::generate_ocr_candidates_at(&tokens, selected, pointer_x, y, None);
            let ranked = recognition::rank_generated(&generated, |word| {
                let hit = word == "pollution" || word == "poll";
                WordEvidence {
                    dictionary_hit: hit,
                    frq: hit.then_some(1200),
                    bnc: None,
                    collins: if hit { 4 } else { 0 },
                    oxford: hit,
                }
            });
            assert_eq!(
                recognition::choose_best(ranked).map(|candidate| candidate.text),
                Some("pollution".into()),
                "pointer fraction {fraction}; OCR token {:?}",
                tokens[selected].text
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_ocr_recognizes_english_from_an_image() {
        let result =
            recognize_image_bytes(include_bytes!("../tests/fixtures/ocr-english.png")).unwrap();
        let text = result
            .Lines()
            .unwrap()
            .into_iter()
            .filter_map(|line| line.Text().ok())
            .map(|text| text.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.to_ascii_lowercase().contains("serendipity"),
            "Windows OCR did not recognize the fixture: {text:?}"
        );
    }
}
