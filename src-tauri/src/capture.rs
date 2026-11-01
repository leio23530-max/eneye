use std::{
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

use crate::word_probe::{self, WordProbeReport};
use crate::{AppState, TranslationRequest, dictionary::clean_lookup_word, translate_selection};

const STABLE_RADIUS: i32 = 5;
const PREFLIGHT_MS: u64 = 100;
const WORD_ANCHOR_RADIUS: i32 = 24;
const DRAG_THRESHOLD: i32 = 7;

static EVENT_SENDER: OnceLock<Sender<MouseInput>> = OnceLock::new();
static TRANSLATION_OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);

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
    candidates: Vec<String>,
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
                show_translation_for_text(&app, text, "uia", point);
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
                        point_inside_overlay(&app, pointer),
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
                    down_on_overlay =
                        overlay_is_visible(&app) && point_inside_overlay(&app, (x, y));
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
                        overlay_is_visible(&app) && point_inside_overlay(&app, (x, y));
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
                    point_inside_overlay(&app, pointer),
                )
            {
                hide_overlay(&app);
                leave_since = None;
            }

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
                    && let Some(ocr_word) = best_ocr_word(&app, &candidates)
                {
                    let uia_lower = uia_text.to_lowercase();
                    let ocr_lower = ocr_word.to_lowercase();
                    let uia_has_punctuation = uia_lower.contains('\'') || uia_lower.contains('-');
                    let ocr_extends = ocr_lower.chars().count() > uia_lower.chars().count()
                        && ocr_lower.starts_with(&uia_lower);
                    if ocr_lower != uia_lower && (!uia_has_punctuation || ocr_extends) {
                        accessible.word = Some((ocr_word, "ocr"));
                    }
                }

                let candidate = match accessible.word {
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
                    let thread_app = app.clone();
                    let thread_report = report.clone();
                    let spawn = thread::Builder::new()
                        .name("eneye-ocr".into())
                        .spawn(move || {
                            let started = Instant::now();
                            let (candidates, error) = match ocr_word_candidates_at(x, y) {
                                Ok(mut candidates) => {
                                    if best_ocr_word(&thread_app, &candidates).is_none()
                                        && let Ok(cleaned) = ocr_word_candidates_at_clean(x, y)
                                    {
                                        candidates.extend(cleaned);
                                        candidates.dedup();
                                    }
                                    (candidates, None)
                                }
                                Err(error) => (Vec::new(), Some(error.to_string())),
                            };
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

fn resolve_ocr(
    app: &tauri::AppHandle,
    report: &mut WordProbeReport,
    point: (i32, i32),
    started: Instant,
    candidates: Vec<String>,
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
    let mut word = best_ocr_word(app, &candidates)
        .or_else(|| candidates.last().cloned())
        .unwrap_or_default();
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
    word_probe::update_ocr_stage(report, true, started.elapsed(), format!("命中 {word}"));
    report.word = Some(word);
    report.source = Some("ocr".into());
    lookup_result
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

fn best_ocr_word(app: &tauri::AppHandle, candidates: &[String]) -> Option<String> {
    let mut best: Option<String> = None;
    for candidate in candidates {
        let Some(result) = find_lookup(app, candidate, "ocr") else {
            continue;
        };
        if result.entries.is_empty() {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|current| candidate.len() > current.len())
        {
            best = Some(candidate.clone());
        }
    }
    best
}

fn overlay_offset(app: &tauri::AppHandle) -> i32 {
    app.state::<AppState>()
        .settings
        .lock()
        .map(|settings| settings.overlay_offset_px as i32)
        .unwrap_or(20)
        .clamp(0, 64)
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
    set_overlay_state(app, OverlayMode::Word, Some(point));
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.emit("eneye://lookup", lookup);
        position_overlay(app, point);
        let _ = window.show();
    }
}

fn show_selection(app: &tauri::AppHandle, payload: SelectionPayload, point: (i32, i32)) {
    set_overlay_state(app, OverlayMode::Translation, Some(point));
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.emit("eneye://selection", payload.clone());
        position_overlay(app, point);
        let _ = window.show();
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

pub(crate) fn position_overlay(app: &tauri::AppHandle, point: (i32, i32)) {
    const WIDTH: i32 = 440;
    const HEIGHT: i32 = 610;
    const GAP: i32 = 12;
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };
    let monitor = app.available_monitors().ok().and_then(|monitors| {
        monitors.into_iter().find(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            point.0 >= position.x
                && point.1 >= position.y
                && point.0 < position.x + size.width as i32
                && point.1 < position.y + size.height as i32
        })
    });
    let (left, top, right, bottom) = monitor
        .map(|monitor| {
            let area = monitor.work_area();
            let position = area.position;
            let size = area.size;
            (
                position.x,
                position.y,
                position.x + size.width as i32,
                position.y + size.height as i32,
            )
        })
        .unwrap_or((0, 0, 1920, 1080));
    let offset = overlay_offset(app);
    let mut x = point.0 + offset;
    let mut y = point.1 + offset;
    if x + WIDTH + GAP > right {
        x = point.0 - WIDTH - offset;
    }
    if y + HEIGHT + GAP > bottom {
        y = point.1 - HEIGHT - offset;
    }
    x = x.clamp(left + GAP, (right - WIDTH - GAP).max(left + GAP));
    y = y.clamp(top + GAP, (bottom - HEIGHT - GAP).max(top + GAP));
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

fn overlay_is_visible(app: &tauri::AppHandle) -> bool {
    app.get_webview_window("overlay")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

fn point_inside_card(app: &tauri::AppHandle, point: (i32, i32)) -> bool {
    let bounds = app
        .state::<AppState>()
        .overlay_card_bounds
        .lock()
        .ok()
        .and_then(|bounds| *bounds);
    if let Some(bounds) = bounds {
        point_in_window_bounds((bounds.x, bounds.y), (bounds.width, bounds.height), point)
    } else {
        point_inside_overlay(app, point)
    }
}

fn point_inside_overlay(app: &tauri::AppHandle, point: (i32, i32)) -> bool {
    let Some(window) = app.get_webview_window("overlay") else {
        return false;
    };
    let Ok(position) = window.outer_position() else {
        return false;
    };
    let Ok(size) = window.inner_size() else {
        return false;
    };
    point_in_window_bounds((position.x, position.y), (size.width, size.height), point)
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
    if overlay_is_visible(app)
        && (point_inside_overlay(app, start) || point_inside_overlay(app, end))
    {
        return;
    }
    if let Some(text) = uia_selection(automation, start) {
        show_translation_for_text(app, text, "uia", end);
    } else if let Some(text) = copy_selection_text() {
        show_translation_for_text(app, text, "clipboard", end);
    } else if let Some(text) = ocr_selection(start, end) {
        show_translation_for_text(app, text, "ocr", end);
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

fn show_translation_for_text(
    app: &tauri::AppHandle,
    text: String,
    source: &str,
    point: (i32, i32),
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
    if !settings.ai_translation_enabled || !settings.auto_translate {
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
fn ocr_word_candidates_at(x: i32, y: i32) -> anyhow::Result<Vec<String>> {
    ocr_word_candidates_at_with(x, y, false)
}

#[cfg(windows)]
fn ocr_word_candidates_at_clean(x: i32, y: i32) -> anyhow::Result<Vec<String>> {
    ocr_word_candidates_at_with(x, y, true)
}

#[cfg(windows)]
fn ocr_word_candidates_at_with(
    x: i32,
    y: i32,
    strip_small_glyphs: bool,
) -> anyhow::Result<Vec<String>> {
    use uiautomation::{screenshots::Screenshot, types::Rect};

    const OCR_SCALE: u32 = 2;
    let left = (x - 600).max(0);
    let rect = Rect::new(left, y - 80, x + 220, y + 80);
    let screenshot = Screenshot::capture_rect(rect)?;
    let raw = screenshot.to_bmp_bytes();
    let raw = if strip_small_glyphs {
        strip_small_ocr_glyphs(&raw)?
    } else {
        raw
    };
    let bmp = upscale_ocr_bitmap(&raw, OCR_SCALE)?;
    let result = recognize_bmp(&bmp)?;
    let mut lines: Vec<(String, Vec<String>)> = Vec::new();
    let mut selected = None;
    for line in result.Lines()? {
        let line_text = line.Text()?.to_string();
        let mut words = Vec::new();
        for word in line.Words()? {
            let bounds = word.BoundingRect()?;
            if selected.is_none()
                && point_in_ocr_word(
                    x,
                    y,
                    rect.get_left(),
                    rect.get_top(),
                    &bounds,
                    OCR_SCALE as f32,
                )
            {
                selected = Some((lines.len(), words.len()));
            }
            words.push(word.Text()?.to_string());
        }
        lines.push((line_text, words));
    }

    let Some((line_index, word_index)) = selected else {
        return Ok(Vec::new());
    };
    let (line_text, words) = &lines[line_index];
    let mut candidates = Vec::new();
    let line_has_hyphen = line_text.contains('-') || words.iter().any(|word| word.contains('-'));
    if line_has_hyphen
        && let Some(hyphen_index) = adjacent_hyphen_index(words, word_index)
        && let Some((_, next_words)) = lines.get(line_index + 1)
    {
        candidates.extend(hyphenated_candidates(
            words,
            word_index,
            hyphen_index,
            next_words,
        ));
    }
    if let Some(raw) = words.get(word_index) {
        if let Ok(word) = clean_lookup_word(raw) {
            candidates.push(word);
        }
        candidates.extend(ocr_spelling_variants(raw));
        if raw.contains(['"', '“', '”']) {
            for segment in raw.split(|character: char| {
                !character.is_ascii_alphabetic() && character != '\'' && character != '-'
            }) {
                if segment.is_empty() {
                    continue;
                }
                if let Ok(word) = clean_lookup_word(segment) {
                    candidates.push(word);
                }
                candidates.extend(ocr_spelling_variants(segment));
            }
        }
    }
    candidates.dedup();
    Ok(candidates)
}

#[cfg(not(windows))]
fn ocr_word_candidates_at(_x: i32, _y: i32) -> anyhow::Result<Vec<String>> {
    Ok(Vec::new())
}

fn push_ocr_variant(variants: &mut Vec<String>, characters: &[char]) {
    if variants.len() >= 128 {
        return;
    }
    let candidate = characters.iter().collect::<String>();
    if let Ok(word) = clean_lookup_word(&candidate)
        && !variants.contains(&word)
    {
        variants.push(word);
    }
}

fn ocr_spelling_variants(text: &str) -> Vec<String> {
    let characters = text.chars().collect::<Vec<_>>();
    let options = characters
        .iter()
        .enumerate()
        .filter_map(|(index, character)| {
            let replacements: &[char] = match character {
                '1' => &['l', 'i'],
                'l' | 'L' => &['i'],
                'i' | 'I' => &['l'],
                '0' => &['o'],
                '5' | 'S' => &['s'],
                _ => &[],
            };
            (!replacements.is_empty()).then(|| (index, replacements.to_vec()))
        })
        .collect::<Vec<_>>();
    let mut variants = Vec::new();
    for (index, replacements) in &options {
        for replacement in replacements {
            let mut candidate = characters.clone();
            candidate[*index] = *replacement;
            push_ocr_variant(&mut variants, &candidate);
        }
    }
    for left in 0..options.len() {
        for right in (left + 1)..options.len() {
            for left_replacement in &options[left].1 {
                for right_replacement in &options[right].1 {
                    let mut candidate = characters.clone();
                    candidate[options[left].0] = *left_replacement;
                    candidate[options[right].0] = *right_replacement;
                    push_ocr_variant(&mut variants, &candidate);
                }
            }
        }
    }
    variants
}

fn letters_fragment(text: &str) -> String {
    text.chars().filter(char::is_ascii_alphabetic).collect()
}

fn adjacent_hyphen_index(words: &[String], selected: usize) -> Option<usize> {
    if words.get(selected)?.trim_end().ends_with('-') {
        return Some(selected);
    }
    if words.get(selected + 1)?.trim_end().ends_with('-') {
        return Some(selected + 1);
    }
    None
}

fn hyphenated_candidates(
    words: &[String],
    selected: usize,
    hyphen_index: usize,
    next_words: &[String],
) -> Vec<String> {
    if selected >= words.len() || hyphen_index < selected || hyphen_index >= words.len() {
        return Vec::new();
    }
    let mut bases = Vec::new();
    for start in [
        selected,
        selected.saturating_sub(1),
        selected.saturating_sub(2),
    ] {
        if start > hyphen_index {
            continue;
        }
        let base = words[start..=hyphen_index]
            .iter()
            .map(|word| letters_fragment(word))
            .collect::<String>();
        if base.len() >= 2 && !bases.contains(&base) {
            bases.push(base);
        }
    }
    let mut candidates = Vec::new();
    for base in bases {
        let mut suffix = String::new();
        for word in next_words.iter().take(8) {
            suffix.push_str(&letters_fragment(word));
            if suffix.len() >= 2 {
                let candidate = format!("{base}{suffix}");
                if !candidates.contains(&candidate) {
                    candidates.push(candidate);
                }
            }
        }
    }
    candidates
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
    let engine = create_ocr_engine()
        .context("Windows OCR 不可用；请在系统语言设置中安装英文 OCR 可选功能")?;
    Ok(engine.RecognizeAsync(&bitmap)?.join()?)
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
fn point_in_ocr_word(
    x: i32,
    y: i32,
    crop_left: i32,
    crop_top: i32,
    bounds: &windows::Foundation::Rect,
    scale: f32,
) -> bool {
    let word_left = crop_left + (bounds.X / scale).round() as i32 - 3;
    let word_top = crop_top + (bounds.Y / scale).round() as i32 - 3;
    let word_right = word_left + (bounds.Width / scale).round() as i32 + 6;
    let word_bottom = word_top + (bounds.Height / scale).round() as i32 + 6;
    x >= word_left && x <= word_right && y >= word_top && y <= word_bottom
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
    fn corrects_common_ocr_l_i_1_confusions() {
        assert!(
            ocr_spelling_variants("llnear")
                .iter()
                .any(|word| word == "linear")
        );
        assert!(
            ocr_spelling_variants("1ess")
                .iter()
                .any(|word| word == "less")
        );
        assert!(
            ocr_spelling_variants("1lnear")
                .iter()
                .any(|word| word == "linear")
        );
    }

    #[test]
    fn only_joins_words_adjacent_to_a_trailing_hyphen() {
        let words = ["Edge-cloud", "synergy:", "The", "data", "incor-"]
            .iter()
            .map(|word| word.to_string())
            .collect::<Vec<_>>();
        assert_eq!(adjacent_hyphen_index(&words, 0), None);
        let words = ["directional", "i", "den-"]
            .iter()
            .map(|word| word.to_string())
            .collect::<Vec<_>>();
        assert_eq!(adjacent_hyphen_index(&words, 1), Some(2));
        assert_eq!(adjacent_hyphen_index(&words, 2), Some(2));
        assert_eq!(adjacent_hyphen_index(&words, 0), None);
    }

    #[test]
    fn reconstructs_a_word_split_across_a_hyphenated_line_break() {
        let words = ["directional", "i", "den-"]
            .iter()
            .map(|word| word.to_string())
            .collect::<Vec<_>>();
        let next = ["ti", "fi", "cation", "processi", "ng"]
            .iter()
            .map(|word| word.to_string())
            .collect::<Vec<_>>();
        let candidates = hyphenated_candidates(&words, 2, 2, &next);
        assert!(candidates.iter().any(|word| word == "identification"));
        let candidates = hyphenated_candidates(&words, 1, 2, &next);
        assert!(candidates.iter().any(|word| word == "identification"));
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
    fn overlay_rectangle_owns_the_pointer() {
        let position = (100, 100);
        let size = (440, 610);
        assert!(point_in_window_bounds(position, size, (100, 100)));
        assert!(point_in_window_bounds(position, size, (539, 709)));
        assert!(!point_in_window_bounds(position, size, (99, 100)));
        assert!(!point_in_window_bounds(position, size, (540, 100)));
        assert!(!point_in_window_bounds(position, size, (100, 710)));
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
