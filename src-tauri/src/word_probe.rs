use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::dictionary::clean_lookup_word;
use crate::recognition::RecognitionCandidate;

#[cfg(windows)]
use windows::Win32::{
    System::Ole::{
        SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
        SafeArrayUnaccessData,
    },
    UI::Accessibility::IUIAutomationTextRange,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeStage {
    pub provider: String,
    pub available: bool,
    pub duration_ms: u64,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordProbeReport {
    pub word: Option<String>,
    pub source: Option<String>,
    pub total_ms: u64,
    pub stages: Vec<ProbeStage>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub candidates: Vec<RecognitionCandidate>,
    #[serde(default)]
    pub reason: Option<String>,
}

impl Default for WordProbeReport {
    fn default() -> Self {
        Self {
            word: None,
            source: None,
            total_ms: 0,
            confidence: None,
            candidates: Vec::new(),
            reason: None,
            stages: vec![
                ProbeStage {
                    provider: "uia".into(),
                    available: false,
                    duration_ms: 0,
                    detail: "尚未探测".into(),
                },
                ProbeStage {
                    provider: "msaa".into(),
                    available: false,
                    duration_ms: 0,
                    detail: "尚未探测".into(),
                },
                ProbeStage {
                    provider: "win32".into(),
                    available: false,
                    duration_ms: 0,
                    detail: "尚未探测".into(),
                },
                ProbeStage {
                    provider: "ocr".into(),
                    available: false,
                    duration_ms: 0,
                    detail: "尚未探测".into(),
                },
            ],
        }
    }
}

pub struct AccessibleProbe {
    pub word: Option<(String, &'static str)>,
    pub report: WordProbeReport,
}

pub fn probe_accessible(
    automation: &uiautomation::UIAutomation,
    point: (i32, i32),
) -> AccessibleProbe {
    let started = Instant::now();
    let mut report = WordProbeReport::default();

    let uia_started = Instant::now();
    let uia_word = probe_uia(automation, point);
    report.stages[0] = ProbeStage {
        provider: "uia".into(),
        available: uia_word.is_some(),
        duration_ms: elapsed_ms(uia_started),
        detail: uia_word
            .as_ref()
            .map(|(word, _)| format!("命中 {word}"))
            .unwrap_or_else(|| "未找到可读文本范围".into()),
    };
    if let Some((word, source)) = uia_word {
        report.word = Some(word.clone());
        report.source = Some(source.into());
        report.total_ms = elapsed_ms(started);
        return AccessibleProbe {
            word: Some((word, source)),
            report,
        };
    }

    let msaa_started = Instant::now();
    let msaa_word = probe_msaa(point);
    report.stages[1] = ProbeStage {
        provider: "msaa".into(),
        available: msaa_word.is_some(),
        duration_ms: elapsed_ms(msaa_started),
        detail: msaa_word
            .as_ref()
            .map(|word| format!("命中 {word}"))
            .unwrap_or_else(|| "AccessibleObjectFromPoint 未返回单词".into()),
    };
    if let Some(word) = msaa_word {
        report.word = Some(word.clone());
        report.source = Some("msaa".into());
        report.total_ms = elapsed_ms(started);
        return AccessibleProbe {
            word: Some((word, "msaa")),
            report,
        };
    }

    let win32_started = Instant::now();
    let win32_word = probe_win32_text(point);
    report.stages[2] = ProbeStage {
        provider: "win32".into(),
        available: win32_word.is_some(),
        duration_ms: elapsed_ms(win32_started),
        detail: win32_word
            .as_ref()
            .map(|word| format!("命中 {word}"))
            .unwrap_or_else(|| "目标窗口未提供可定位文本".into()),
    };
    if let Some(word) = win32_word {
        report.word = Some(word.clone());
        report.source = Some("win32".into());
        report.total_ms = elapsed_ms(started);
        return AccessibleProbe {
            word: Some((word, "win32")),
            report,
        };
    }

    report.total_ms = elapsed_ms(started);
    AccessibleProbe { word: None, report }
}

pub fn update_ocr_stage(
    report: &mut WordProbeReport,
    available: bool,
    duration: Duration,
    detail: String,
) {
    report.stages[3] = ProbeStage {
        provider: "ocr".into(),
        available,
        duration_ms: duration.as_millis() as u64,
        detail,
    };
    report.total_ms += duration.as_millis() as u64;
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}

#[cfg(windows)]
fn probe_uia(
    automation: &uiautomation::UIAutomation,
    point: (i32, i32),
) -> Option<(String, &'static str)> {
    use uiautomation::{
        patterns::{UITextPattern, UIValuePattern},
        types::Point,
    };

    let ui_point = Point::new(point.0, point.1);
    let element = automation.element_from_point(ui_point).ok()?;
    let walkers = [
        automation.get_control_view_walker().ok(),
        automation.get_raw_view_walker().ok(),
        automation.get_content_view_walker().ok(),
    ];

    for walker in walkers.into_iter().flatten() {
        let mut current = element.clone();
        for _ in 0..10 {
            if current.is_password().unwrap_or(false) {
                return None;
            }
            let bounds = current.get_bounding_rectangle().ok();

            // TextPattern's RangeFromPoint snaps to the nearest caret position when
            // the pointer is in blank space, and can point at the previous word near
            // a word boundary. Only accept a word whose own bounding rectangle
            // actually contains the pointer, and walk to adjacent words if the
            // provider snapped to a neighbour.
            if let Ok(pattern) = current.get_pattern::<UITextPattern>()
                && let Ok(range) = pattern.get_range_from_point(ui_point)
                && let Some(word) = word_from_text_range(&range, point)
            {
                return Some((word, "uia"));
            }

            // Name/Value fallbacks describe whole elements, so they can leak text
            // from a wide toolbar or container when the pointer is on blank space.
            // Restrict them to leaf-ish text controls and check the label width.
            if current
                .get_control_type()
                .map(is_text_like_control_type)
                .unwrap_or(false)
            {
                if let Ok(pattern) = current.get_pattern::<UIValuePattern>()
                    && let Ok(text) = pattern.get_value()
                    && let Some(word) =
                        word_from_single_label(&text, point, bounds.as_ref().map(rect_tuple))
                {
                    return Some((word, "uia-value"));
                }
                if let Ok(text) = current.get_name()
                    && let Some(word) =
                        word_from_single_label(&text, point, bounds.as_ref().map(rect_tuple))
                {
                    return Some((word, "uia-name"));
                }
            }

            if let Ok(parent) = walker.get_parent(&current) {
                current = parent;
            } else {
                break;
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn probe_uia(
    _automation: &uiautomation::UIAutomation,
    _point: (i32, i32),
) -> Option<(String, &'static str)> {
    None
}

#[cfg(windows)]
fn probe_msaa(point: (i32, i32)) -> Option<String> {
    use windows::Win32::{
        Foundation::POINT,
        System::Variant::VARIANT,
        UI::Accessibility::{AccessibleObjectFromPoint, IAccessible},
    };

    let mut accessible: Option<IAccessible> = None;
    let mut child = VARIANT::default();
    unsafe {
        AccessibleObjectFromPoint(
            POINT {
                x: point.0,
                y: point.1,
            },
            &mut accessible,
            &mut child,
        )
        .ok()?;
    }
    let accessible = accessible?;
    let mut left = 0;
    let mut top = 0;
    let mut width = 0;
    let mut height = 0;
    let bounds = unsafe {
        accessible
            .accLocation(&mut left, &mut top, &mut width, &mut height, &child)
            .ok()
            .map(|_| (left, top, width, height))
    };
    let name = unsafe { accessible.get_accName(&child) }
        .ok()
        .map(|value| value.to_string());
    let value = unsafe { accessible.get_accValue(&child) }
        .ok()
        .map(|value| value.to_string());
    name.into_iter()
        .chain(value)
        .find_map(|text| word_from_single_label(&text, point, bounds))
}

#[cfg(not(windows))]
fn probe_msaa(_point: (i32, i32)) -> Option<String> {
    None
}

#[cfg(windows)]
fn probe_win32_text(point: (i32, i32)) -> Option<String> {
    use windows::Win32::{
        Foundation::{HWND, POINT, RECT},
        UI::WindowsAndMessaging::{
            GetClassNameW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, WindowFromPoint,
        },
    };

    let hwnd = unsafe {
        WindowFromPoint(POINT {
            x: point.0,
            y: point.1,
        })
    };
    if hwnd == HWND::default() {
        return None;
    }
    let mut class_name = [0_u16; 256];
    let class_len = unsafe { GetClassNameW(hwnd, &mut class_name) };
    let class_name = String::from_utf16_lossy(&class_name[..class_len.max(0) as usize]);
    if !is_text_control_class(&class_name) {
        return None;
    }
    let text_len = unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize;
    if text_len == 0 {
        // Windows 11 Notepad's "Edit" control reports zero through
        // GetWindowTextLengthW while still serving WM_GETTEXT and exact
        // EM_CHARFROMPOS/EM_GETLINE positions. Read only the hovered line so
        // large documents never require copying the whole buffer.
        if is_edit_control_class(&class_name) {
            return word_from_edit_line(hwnd, point);
        }
        return None;
    }
    let mut text_buffer = vec![0_u16; text_len + 1];
    let copied = unsafe { GetWindowTextW(hwnd, &mut text_buffer) };
    if copied <= 0 {
        return None;
    }
    let text = String::from_utf16_lossy(&text_buffer[..copied as usize]);
    if is_edit_control_class(&class_name)
        && let Some(word) = word_from_edit_point(hwnd, &text, point)
    {
        return Some(word);
    }
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    word_from_text_at_x(
        &text,
        point,
        Some((
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
        )),
    )
}

/// Edit and RichEdit controls expose exact character positions even when they
/// are multiline and have no UI Automation TextPattern. This is the only
/// reliable word source for Notepad, whose full text is one long Name string.
#[cfg(windows)]
fn word_from_edit_point(
    hwnd: windows::Win32::Foundation::HWND,
    text: &str,
    point: (i32, i32),
) -> Option<String> {
    use windows::Win32::{
        Foundation::{LPARAM, POINT, WPARAM},
        Graphics::Gdi::ScreenToClient,
        UI::{Controls::EM_CHARFROMPOS, WindowsAndMessaging::SendMessageW},
    };

    let mut client = POINT {
        x: point.0,
        y: point.1,
    };
    if !unsafe { ScreenToClient(hwnd, &mut client) }.as_bool() {
        return None;
    }
    let lparam = (((client.y as u16 as u32) << 16) | (client.x as u16 as u32)) as isize;
    let index =
        unsafe { SendMessageW(hwnd, EM_CHARFROMPOS, Some(WPARAM(0)), Some(LPARAM(lparam))) }.0
            & 0xffff;
    if index < 0 {
        return None;
    }
    word_from_text_character_index(text, index as usize)
}

/// Reads only the hovered line from a control that hides its length from
/// GetWindowTextLengthW (Windows 11 Notepad does this while still answering
/// EM_CHARFROMPOS and WM_GETTEXT). Keeping this line-based avoids copying a
/// whole document on every hover.
#[cfg(windows)]
fn word_from_edit_line(
    hwnd: windows::Win32::Foundation::HWND,
    point: (i32, i32),
) -> Option<String> {
    use windows::Win32::{
        Foundation::{LPARAM, POINT, WPARAM},
        Graphics::Gdi::ScreenToClient,
        UI::{
            Controls::{EM_CHARFROMPOS, EM_LINEFROMCHAR, EM_LINEINDEX, EM_LINELENGTH},
            WindowsAndMessaging::{SendMessageW, WM_GETTEXT},
        },
    };

    let mut client = POINT {
        x: point.0,
        y: point.1,
    };
    if !unsafe { ScreenToClient(hwnd, &mut client) }.as_bool() {
        return None;
    }
    let lparam = (((client.y as u16 as u32) << 16) | (client.x as u16 as u32)) as isize;
    let index =
        unsafe { SendMessageW(hwnd, EM_CHARFROMPOS, Some(WPARAM(0)), Some(LPARAM(lparam))) }.0
            & 0xffff;
    if index < 0 {
        return None;
    }
    let line = unsafe {
        SendMessageW(
            hwnd,
            EM_LINEFROMCHAR,
            Some(WPARAM(index as usize)),
            Some(LPARAM(0)),
        )
    }
    .0;
    if line < 0 {
        return None;
    }
    let line_start = unsafe {
        SendMessageW(
            hwnd,
            EM_LINEINDEX,
            Some(WPARAM(line as usize)),
            Some(LPARAM(0)),
        )
    }
    .0;
    if line_start < 0 || index < line_start {
        return None;
    }
    let line_len = unsafe {
        SendMessageW(
            hwnd,
            EM_LINELENGTH,
            Some(WPARAM(line_start as usize)),
            Some(LPARAM(0)),
        )
    }
    .0;
    if line_len <= 0 {
        return None;
    }
    // WM_GETTEXT is the Unicode-safe path for this control; EM_GETLINE on
    // Windows 11 Notepad returns ANSI bytes even through SendMessageW. Copy
    // only the prefix through the hovered line, then slice that line out.
    let line_start = line_start as usize;
    let line_len = line_len as usize;
    let capacity = line_start.saturating_add(line_len).saturating_add(2);
    let mut buffer = vec![0_u16; capacity];
    let copied = unsafe {
        SendMessageW(
            hwnd,
            WM_GETTEXT,
            Some(WPARAM(capacity)),
            Some(LPARAM(buffer.as_mut_ptr() as isize)),
        )
    }
    .0;
    if copied <= 0 || (copied as usize) < line_start + line_len {
        return None;
    }
    let line_text = String::from_utf16_lossy(&buffer[line_start..line_start + line_len]);
    word_from_text_character_index(&line_text, (index as usize) - line_start)
}

fn word_from_text_character_index(text: &str, index: usize) -> Option<String> {
    let utf16 = text.encode_utf16().collect::<Vec<_>>();
    let character = *utf16.get(index)?;
    let is_word = |value: u16| {
        char::from_u32(value as u32).is_some_and(|character| {
            character.is_ascii_alphabetic() || character == '\'' || character == '-'
        })
    };
    if !is_word(character) {
        return None;
    }
    let mut start = index;
    while start > 0 && is_word(utf16[start - 1]) {
        start -= 1;
    }
    let mut end = index + 1;
    while end < utf16.len() && is_word(utf16[end]) {
        end += 1;
    }
    let word = String::from_utf16(&utf16[start..end]).ok()?;
    clean_lookup_word(&word).ok()
}

#[cfg(not(windows))]
fn probe_win32_text(_point: (i32, i32)) -> Option<String> {
    None
}

fn is_edit_control_class(class_name: &str) -> bool {
    let class_name = class_name.to_ascii_lowercase();
    class_name == "edit" || class_name.starts_with("richedit")
}

fn is_text_control_class(class_name: &str) -> bool {
    let class_name = class_name.to_ascii_lowercase();
    is_edit_control_class(&class_name)
        || class_name == "static"
        || class_name.contains("scintilla")
        || class_name.contains("textedit")
}

fn rect_tuple(rect: &uiautomation::types::Rect) -> (i32, i32, i32, i32) {
    (
        rect.get_left(),
        rect.get_top(),
        rect.get_width(),
        rect.get_height(),
    )
}

#[cfg(windows)]
fn word_from_text_range(
    range: &uiautomation::patterns::UITextRange,
    point: (i32, i32),
) -> Option<String> {
    use uiautomation::types::TextUnit;

    for offset in [0_i32, 1, -1, 2, -2] {
        let candidate = range.clone();
        if candidate.expand_to_enclosing_unit(TextUnit::Word).is_err() {
            return None;
        }
        if offset != 0 {
            if candidate.move_text(TextUnit::Word, offset).unwrap_or(0) == 0 {
                continue;
            }
            let _ = candidate.expand_to_enclosing_unit(TextUnit::Word);
        }
        let Ok(text) = candidate.get_text(96) else {
            continue;
        };
        let Ok(word) = clean_lookup_word(&text) else {
            continue;
        };
        let rects = text_range_rects(&candidate);
        if rects.iter().any(|rect| rect_contains_point(*rect, point)) {
            return Some(word);
        }
    }
    None
}

#[cfg(windows)]
fn text_range_rects(range: &uiautomation::patterns::UITextRange) -> Vec<(i32, i32, i32, i32)> {
    let raw: IUIAutomationTextRange = range.clone().into();
    let Ok(array) = (unsafe { raw.GetBoundingRectangles() }) else {
        return Vec::new();
    };
    let values = unsafe { read_safe_array_f64(array) };
    values
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|rect| rect[2] > 0.0 && rect[3] > 0.0)
        .map(|rect| {
            (
                rect[0].round() as i32,
                rect[1].round() as i32,
                rect[2].round().max(1.0) as i32,
                rect[3].round().max(1.0) as i32,
            )
        })
        .collect()
}

/// Takes ownership of a one-dimensional VT_R8 SAFEARRAY returned by UI Automation.
#[cfg(windows)]
unsafe fn read_safe_array_f64(array: *mut windows::Win32::System::Com::SAFEARRAY) -> Vec<f64> {
    use std::ffi::c_void;

    if array.is_null() {
        return Vec::new();
    }

    unsafe {
        let mut data: *mut c_void = std::ptr::null_mut();
        if SafeArrayAccessData(array, &mut data).is_err() {
            let _ = SafeArrayDestroy(array);
            return Vec::new();
        }
        let lower = SafeArrayGetLBound(array, 1).unwrap_or(0);
        let upper = SafeArrayGetUBound(array, 1).unwrap_or(-1);
        let len = (upper - lower + 1).max(0) as usize;
        let values = if data.is_null() || len == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(data as *const f64, len).to_vec()
        };
        let _ = SafeArrayUnaccessData(array);
        let _ = SafeArrayDestroy(array);
        values
    }
}

const WORD_RECT_TOLERANCE: i32 = 3;

fn rect_contains_point(rect: (i32, i32, i32, i32), point: (i32, i32)) -> bool {
    let (left, top, width, height) = rect;
    point.0 >= left - WORD_RECT_TOLERANCE
        && point.0 <= left + width + WORD_RECT_TOLERANCE
        && point.1 >= top - WORD_RECT_TOLERANCE
        && point.1 <= top + height + WORD_RECT_TOLERANCE
}

/// A whole-element accessible name can be much wider than the word it contains.
/// If a wide, short toolbar exposes a single-word name, the pointer being inside
/// the element does not mean it is on that word.
fn word_fits_bounds(word: &str, width: i32, height: i32) -> bool {
    if width <= 0 || height <= 0 {
        return false;
    }
    let glyphs = word.chars().count() as f32;
    let estimated_width = (glyphs * height as f32 * 0.62).max(height as f32 * 1.2);
    width as f32 <= estimated_width * 2.0 + height as f32 * 2.0
}

#[cfg(windows)]
fn is_text_like_control_type(control_type: uiautomation::types::ControlType) -> bool {
    use uiautomation::types::ControlType;

    matches!(
        control_type,
        ControlType::Text
            | ControlType::Edit
            | ControlType::Document
            | ControlType::Hyperlink
            | ControlType::Button
            | ControlType::CheckBox
            | ControlType::RadioButton
            | ControlType::ListItem
            | ControlType::DataItem
            | ControlType::TabItem
            | ControlType::MenuItem
            | ControlType::TreeItem
    )
}

fn word_from_single_label(
    text: &str,
    point: (i32, i32),
    bounds: Option<(i32, i32, i32, i32)>,
) -> Option<String> {
    if text.split_whitespace().count() != 1 {
        return None;
    }
    word_from_text_at_x(text, point, bounds)
}

fn word_from_text_at_x(
    text: &str,
    point: (i32, i32),
    bounds: Option<(i32, i32, i32, i32)>,
) -> Option<String> {
    let words = text
        .split_whitespace()
        .filter_map(|word| clean_lookup_word(word).ok())
        .collect::<Vec<_>>();
    if words.is_empty() || text.contains(['\n', '\r']) {
        return None;
    }
    let (left, top, width, height) = bounds?;
    if width <= 0
        || height <= 0
        || height > 56
        || point.0 < left - 4
        || point.0 > left + width + 4
        || point.1 < top - 4
        || point.1 > top + height + 4
    {
        return None;
    }
    if words.len() == 1 {
        let word = words.into_iter().next()?;
        return word_fits_bounds(&word, width, height).then_some(word);
    }
    if words.len() > 40 {
        return None;
    }
    let ratio = ((point.0 - left) as f32 / width as f32).clamp(0.0, 0.999);
    words
        .get((ratio * words.len() as f32).floor() as usize)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_pointer_to_word_inside_single_line_text() {
        let bounds = Some((100, 10, 400, 24));
        assert_eq!(
            word_from_text_at_x("alpha beta gamma", (110, 20), bounds).as_deref(),
            Some("alpha")
        );
        assert_eq!(
            word_from_text_at_x("alpha beta gamma", (250, 20), bounds).as_deref(),
            Some("beta")
        );
        assert_eq!(
            word_from_text_at_x("alpha beta gamma", (490, 20), bounds).as_deref(),
            Some("gamma")
        );
    }

    #[test]
    fn rejects_multiline_text_without_precise_character_position() {
        assert_eq!(
            word_from_text_at_x("hello world\nnext line", (150, 20), Some((0, 0, 500, 80))),
            None
        );
    }

    #[test]
    fn rejects_multiword_names_from_tall_containers() {
        assert_eq!(
            word_from_text_at_x("alpha beta gamma", (250, 200), Some((100, 100, 400, 180))),
            None
        );
    }

    #[test]
    fn rejects_a_single_word_name_when_the_pointer_is_on_blank_container_space() {
        assert_eq!(
            word_from_text_at_x("Git", (450, 350), Some((100, 100, 800, 500))),
            None
        );
    }

    #[test]
    fn ignores_multiword_element_labels() {
        assert_eq!(
            word_from_single_label(
                "deliver insufficient power",
                (560, 640),
                Some((500, 630, 300, 24))
            ),
            None
        );
        assert_eq!(
            word_from_single_label("deliver", (510, 640), Some((500, 630, 60, 24))).as_deref(),
            Some("deliver")
        );
    }

    #[test]
    fn rejects_a_short_wide_container_whose_name_is_a_single_word() {
        assert_eq!(
            word_from_text_at_x("Git", (450, 21), Some((100, 10, 300, 24))),
            None
        );
    }

    #[test]
    fn accepts_a_single_word_label_that_fits_its_bounds() {
        assert_eq!(
            word_from_text_at_x("Git", (120, 21), Some((100, 10, 40, 24))).as_deref(),
            Some("Git")
        );
    }

    #[test]
    fn text_range_must_contain_the_pointer_to_be_used() {
        let word = (100, 10, 40, 20);
        assert!(rect_contains_point(word, (100, 10)));
        assert!(rect_contains_point(word, (140, 30)));
        assert!(rect_contains_point(word, (98, 9)));
        assert!(!rect_contains_point(word, (96, 20)));
        assert!(!rect_contains_point(word, (150, 20)));
        assert!(!rect_contains_point(word, (120, 40)));
    }

    #[test]
    fn accepts_standard_edit_control_classes() {
        assert!(is_text_control_class("Edit"));
        assert!(is_text_control_class("RichEdit20W"));
        assert!(is_edit_control_class("Edit"));
        assert!(is_edit_control_class("RichEditD2DPT"));
        assert!(!is_edit_control_class("Static"));
        assert!(!is_text_control_class("Chrome_WidgetWin_1"));
    }

    #[test]
    fn maps_a_multiline_edit_character_to_its_whole_word() {
        let text = "pollution war on pollution\r\nA)As being a greater threat.\r\nnanogenerator";
        assert_eq!(
            word_from_text_character_index(text, 1).as_deref(),
            Some("pollution")
        );
        assert_eq!(
            word_from_text_character_index(text, 8).as_deref(),
            Some("pollution")
        );
        assert_eq!(
            word_from_text_character_index(text, 12).as_deref(),
            Some("war")
        );
        assert_eq!(
            word_from_text_character_index(text, 15).as_deref(),
            Some("on")
        );
        assert_eq!(
            word_from_text_character_index(text, 18).as_deref(),
            Some("pollution")
        );
        assert_eq!(
            word_from_text_character_index(text, 33).as_deref(),
            Some("being")
        );
        assert_eq!(
            word_from_text_character_index(text, 58).as_deref(),
            Some("nanogenerator")
        );
    }

    #[test]
    fn rejects_blank_space_and_list_markers_in_an_edit_control() {
        let text = "A)As being a greater threat.\r\npollution";
        assert_eq!(word_from_text_character_index(text, 1), None);
        assert_eq!(word_from_text_character_index(text, 4), None);
        assert_eq!(word_from_text_character_index(text, 28), None);
        assert_eq!(word_from_text_character_index(text, 1000), None);
    }

    #[test]
    fn keeps_internal_apostrophes_and_hyphens_for_edit_probe() {
        let text = "don't well-known";
        assert_eq!(
            word_from_text_character_index(text, 2).as_deref(),
            Some("don't")
        );
        assert_eq!(
            word_from_text_character_index(text, 11).as_deref(),
            Some("well-known")
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires an interactive Windows desktop"]
    fn real_edit_control_maps_every_pollution_letter_to_the_whole_word() {
        use windows::Win32::{
            Foundation::{LPARAM, POINT, WPARAM},
            Graphics::Gdi::{ClientToScreen, UpdateWindow},
            UI::{
                Controls::EM_POSFROMCHAR,
                WindowsAndMessaging::{
                    CreateWindowExW, DestroyWindow, ES_AUTOVSCROLL, ES_MULTILINE, SW_SHOW,
                    SendMessageW, SetWindowTextW, ShowWindow, WINDOW_STYLE, WS_BORDER, WS_CHILD,
                    WS_OVERLAPPEDWINDOW, WS_VISIBLE,
                },
            },
        };
        use windows::core::{HSTRING, w};

        let parent = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("Eneye probe test"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                320,
                320,
                640,
                360,
                None,
                None,
                None,
                None,
            )
        }
        .expect("probe parent window");
        let text = "pollution war on pollution\r\nA)As being a greater threat.";
        let edit = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("EDIT"),
                w!(""),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_BORDER
                    | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL) as u32),
                10,
                10,
                600,
                300,
                Some(parent),
                None,
                None,
                None,
            )
        }
        .expect("probe edit control");
        unsafe {
            SetWindowTextW(edit, &HSTRING::from(text)).expect("probe text");
            let _ = ShowWindow(parent, SW_SHOW);
            let _ = UpdateWindow(parent);
        }

        for index in 0..9 {
            let packed =
                unsafe { SendMessageW(edit, EM_POSFROMCHAR, Some(WPARAM(index)), Some(LPARAM(0))) }
                    .0;
            let x = (packed & 0xffff) as u16 as i16 as i32;
            let y = ((packed >> 16) & 0xffff) as u16 as i16 as i32;
            let mut point = POINT { x: x + 2, y: y + 5 };
            unsafe { ClientToScreen(edit, &mut point) }
                .ok()
                .expect("screen point");
            assert_eq!(
                word_from_edit_point(edit, text, (point.x, point.y)).as_deref(),
                Some("pollution"),
                "character {index} at ({x}, {y})"
            );
        }

        unsafe { DestroyWindow(parent) }.expect("destroy probe window");
    }
}
