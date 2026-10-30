use std::{
    sync::{
        OnceLock,
        atomic::Ordering,
        mpsc::{self, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::{AppState, TranslationRequest, dictionary::clean_lookup_word, translate_selection};

const STABLE_RADIUS: i32 = 5;
const PREFLIGHT_MS: u64 = 100;
const HIDE_GRACE_MS: u64 = 180;
const DRAG_THRESHOLD: i32 = 7;

static EVENT_SENDER: OnceLock<Sender<MouseInput>> = OnceLock::new();

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HoverCandidate {
    Word { text: String, source: String },
    Ocr { x: i32, y: i32 },
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

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LookupLoadingPayload {
    source: String,
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
        use windows::{Globalization::Language, Media::Ocr::OcrEngine, core::HSTRING};

        Language::CreateLanguage(&HSTRING::from("en-US"))
            .ok()
            .and_then(|language| OcrEngine::TryCreateFromLanguage(&language).ok())
            .is_some()
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
        let mut stable_since = Instant::now();
        let mut down_at: Option<(i32, i32)> = None;
        let mut preflight: Option<HoverCandidate> = None;
        let mut resolved_for_dwell = false;
        let mut leave_since: Option<Instant> = None;

        loop {
            let event = receiver.recv_timeout(Duration::from_millis(20));
            match event {
                Ok(MouseInput {
                    kind: MouseInputKind::Move,
                    x,
                    y,
                }) => {
                    if exceeds_stability_radius(pointer, (x, y)) {
                        pointer = (x, y);
                        stable_since = Instant::now();
                        preflight = None;
                        resolved_for_dwell = false;
                    } else {
                        pointer = (x, y);
                    }

                    if overlay_pinned(&app) {
                        leave_since = None;
                    } else if overlay_is_visible(&app)
                        && !point_inside_overlay(&app, pointer)
                        && !point_inside_own_process(pointer)
                    {
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
                    down_at = Some((x, y));
                    stable_since = Instant::now();
                    preflight = None;
                    resolved_for_dwell = false;
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
                    if dragged {
                        handle_selection(&app, &automation, drag_origin.unwrap_or((x, y)), (x, y));
                    }
                    stable_since = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }

            if let Some(since) = leave_since
                && since.elapsed() >= Duration::from_millis(HIDE_GRACE_MS)
                && !overlay_pinned(&app)
                && !point_inside_overlay(&app, pointer)
            {
                hide_overlay(&app);
                leave_since = None;
            }

            if down_at.is_some() || point_inside_own_process(pointer) {
                continue;
            }

            let elapsed = stable_since.elapsed().as_millis() as u64;
            let delay = lookup_delay(&app);
            if elapsed >= PREFLIGHT_MS && preflight.is_none() && !resolved_for_dwell {
                preflight = uia_word_at(&automation, pointer);
            }
            if elapsed < delay || resolved_for_dwell {
                continue;
            }
            let Some(candidate) = preflight.take() else {
                continue;
            };
            resolved_for_dwell = true;

            let result = match candidate {
                HoverCandidate::Word { text, source } => {
                    lookup(&app, &text, &source).filter(|lookup| !lookup.entries.is_empty())
                }
                HoverCandidate::Ocr { x, y } => {
                    if !ocr_available() {
                        hide_overlay(&app);
                        continue;
                    }
                    show_loading(&app, "ocr");
                    match ocr_word_at(x, y) {
                        Ok(Some(word)) => {
                            lookup(&app, &word, "ocr").filter(|lookup| !lookup.entries.is_empty())
                        }
                        _ => None,
                    }
                }
            };
            if let Some(result) = result {
                show_lookup(&app, result, pointer);
            } else {
                hide_overlay(&app);
            }
            preflight = None;
        }
    }
}

fn lookup(app: &tauri::AppHandle, word: &str, source: &str) -> Option<crate::models::LookupResult> {
    let clean = clean_lookup_word(word).ok()?;
    let state = app.state::<AppState>();
    let result = state.dictionary.lock().ok()?.lookup(&clean, source).ok()?;
    if !result.entries.is_empty()
        && let Ok(storage) = state.storage.lock()
    {
        let _ = storage.record_lookup(&result);
    }
    Some(result)
}

fn lookup_delay(app: &tauri::AppHandle) -> u64 {
    app.state::<AppState>()
        .settings
        .lock()
        .map(|settings| settings.lookup_delay_ms)
        .unwrap_or(240)
}

fn overlay_pinned(app: &tauri::AppHandle) -> bool {
    app.state::<AppState>()
        .overlay_pinned
        .load(Ordering::Relaxed)
}

fn show_loading(app: &tauri::AppHandle, source: &str) {
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.emit(
            "eneye://lookup-loading",
            LookupLoadingPayload {
                source: source.into(),
            },
        );
        let _ = position_and_show_overlay(app);
    }
}

fn show_lookup(app: &tauri::AppHandle, lookup: crate::models::LookupResult, point: (i32, i32)) {
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.emit("eneye://lookup", lookup);
        position_overlay(app, point);
        let _ = window.show();
    }
}

fn show_selection(app: &tauri::AppHandle, payload: SelectionPayload, point: (i32, i32)) {
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

fn hide_overlay(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("overlay")
        && !overlay_pinned(app)
    {
        let _ = window.hide();
    }
}

fn position_and_show_overlay(app: &tauri::AppHandle) -> bool {
    #[cfg(windows)]
    {
        let mut point = windows::Win32::Foundation::POINT::default();
        if unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) }.is_ok() {
            position_overlay(app, (point.x, point.y));
            if let Some(window) = app.get_webview_window("overlay") {
                let _ = window.show();
                return true;
            }
        }
    }
    false
}

fn position_overlay(app: &tauri::AppHandle, point: (i32, i32)) {
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
    let mut x = point.0 + 18;
    let mut y = point.1 + 22;
    if x + WIDTH + GAP > right {
        x = point.0 - WIDTH - 18;
    }
    if y + HEIGHT + GAP > bottom {
        y = point.1 - HEIGHT - 18;
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
    point.0 >= position.x
        && point.1 >= position.y
        && point.0 < position.x + size.width as i32
        && point.1 < position.y + size.height as i32
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
fn uia_word_at(
    automation: &uiautomation::UIAutomation,
    point: (i32, i32),
) -> Option<HoverCandidate> {
    use uiautomation::{
        patterns::UITextPattern,
        types::{Point, TextUnit},
    };

    let point = Point::new(point.0, point.1);
    let walker = automation.get_control_view_walker().ok()?;
    let mut element = automation.element_from_point(point).ok()?;
    for _ in 0..7 {
        if element.is_password().unwrap_or(false) {
            return None;
        }
        if let Ok(pattern) = element.get_pattern::<UITextPattern>()
            && let Ok(range) = pattern.get_range_from_point(point)
        {
            let _ = range.expand_to_enclosing_unit(TextUnit::Word);
            if let Ok(text) = range.get_text(128)
                && let Ok(word) = clean_lookup_word(&text)
            {
                return Some(HoverCandidate::Word {
                    text: word,
                    source: "uia".into(),
                });
            }
        }
        let Ok(parent) = walker.get_parent(&element) else {
            break;
        };
        element = parent;
    }
    Some(HoverCandidate::Ocr {
        x: point.get_x(),
        y: point.get_y(),
    })
}

#[cfg(not(windows))]
fn uia_word_at(
    _automation: &uiautomation::UIAutomation,
    point: (i32, i32),
) -> Option<HoverCandidate> {
    Some(HoverCandidate::Ocr {
        x: point.0,
        y: point.1,
    })
}

#[cfg(windows)]
fn handle_selection(
    app: &tauri::AppHandle,
    automation: &uiautomation::UIAutomation,
    start: (i32, i32),
    end: (i32, i32),
) {
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
    if !settings.auto_translate {
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
    let keyboard = Keyboard::new();
    keyboard.send_keys("{ctrl}c").ok()?;
    thread::sleep(Duration::from_millis(70));
    let text = Clipboard::open().ok()?.get_text().ok();
    if let Ok(clipboard) = Clipboard::open() {
        let _ = clipboard.restore(snapshot);
    }
    text.filter(|text| !text.trim().is_empty())
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
fn ocr_word_at(x: i32, y: i32) -> anyhow::Result<Option<String>> {
    use uiautomation::{screenshots::Screenshot, types::Rect};

    let rect = Rect::new(x - 220, y - 70, x + 220, y + 70);
    let screenshot = Screenshot::capture_rect(rect)?;
    let bmp = screenshot.to_bmp_bytes();
    let result = recognize_bmp(&bmp)?;
    for line in result.Lines()? {
        for word in line.Words()? {
            let bounds = word.BoundingRect()?;
            if point_in_ocr_word(x, y, rect.get_left(), rect.get_top(), &bounds) {
                return Ok(clean_lookup_word(&word.Text()?.to_string()).ok());
            }
        }
    }
    Ok(None)
}

#[cfg(not(windows))]
fn ocr_word_at(_x: i32, _y: i32) -> anyhow::Result<Option<String>> {
    Ok(None)
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
    use windows::{
        Globalization::Language,
        Graphics::Imaging::BitmapDecoder,
        Media::Ocr::OcrEngine,
        Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
        core::HSTRING,
    };

    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::new()?;
    writer.WriteBytes(bmp)?;
    let buffer = writer.DetachBuffer()?;
    stream.WriteAsync(&buffer)?.join()?;
    stream.Seek(0)?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
    let bitmap = decoder.GetSoftwareBitmapAsync()?.join()?;
    let language = Language::CreateLanguage(&HSTRING::from("en-US"))?;
    let engine = OcrEngine::TryCreateFromLanguage(&language)?;
    Ok(engine.RecognizeAsync(&bitmap)?.join()?)
}

#[cfg(windows)]
fn point_in_ocr_word(
    x: i32,
    y: i32,
    crop_left: i32,
    crop_top: i32,
    bounds: &windows::Foundation::Rect,
) -> bool {
    let word_left = crop_left + bounds.X.round() as i32 - 3;
    let word_top = crop_top + bounds.Y.round() as i32 - 3;
    let word_right = word_left + bounds.Width.round() as i32 + 6;
    let word_bottom = word_top + bounds.Height.round() as i32 + 6;
    x >= word_left && x <= word_right && y >= word_top && y <= word_bottom
}

#[cfg(windows)]
fn run_mouse_hook() {
    use windows::Win32::{
        Foundation::{LPARAM, LRESULT, WPARAM},
        UI::WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, MSG, MSLLHOOKSTRUCT,
            SetWindowsHookExW, TranslateMessage, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP,
            WM_MOUSEMOVE,
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

    let hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) };
    let Ok(hook) = hook else {
        eprintln!("failed to install mouse hook: {:?}", hook.err());
        return;
    };
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
        let _ = unsafe { TranslateMessage(&message) };
        unsafe { DispatchMessageW(&message) };
    }
    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(hook) };
}

#[cfg(not(windows))]
fn run_mouse_hook() {}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn small_pointer_jitter_does_not_start_a_new_hover_cycle() {
        assert!(!exceeds_stability_radius((100, 100), (103, 104)));
        assert!(exceeds_stability_radius((100, 100), (106, 100)));
    }
}
