use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::dictionary::clean_lookup_word;

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
}

impl Default for WordProbeReport {
    fn default() -> Self {
        Self {
            word: None,
            source: None,
            total_ms: 0,
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
        UI::WindowsAndMessaging::{GetClassNameW, GetWindowRect, GetWindowTextW, WindowFromPoint},
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
    let mut text_buffer = [0_u16; 8192];
    let text_len = unsafe { GetWindowTextW(hwnd, &mut text_buffer) };
    if text_len <= 0 {
        return None;
    }
    let text = String::from_utf16_lossy(&text_buffer[..text_len as usize]);
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

#[cfg(not(windows))]
fn probe_win32_text(_point: (i32, i32)) -> Option<String> {
    None
}

fn is_text_control_class(class_name: &str) -> bool {
    let class_name = class_name.to_ascii_lowercase();
    class_name == "edit"
        || class_name == "static"
        || class_name.starts_with("richedit")
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
        assert!(!is_text_control_class("Chrome_WidgetWin_1"));
    }
}
