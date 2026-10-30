use std::collections::BTreeSet;

pub const FALLBACK_FONTS: &[&str] = &[
    "Segoe UI",
    "Microsoft YaHei UI",
    "Georgia",
    "Cambria",
    "Times New Roman",
    "Consolas",
    "SimSun",
    "KaiTi",
];

pub fn clean_registered_font_name(value: &str) -> Option<String> {
    let mut name = value.trim();
    for suffix in [
        " (TrueType)",
        " (OpenType)",
        " (All res)",
        " (PostScript)",
        " (Variable)",
    ] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.trim();
            break;
        }
    }
    if name.is_empty()
        || name.starts_with('@')
        || name.to_ascii_lowercase().contains("bold")
        || name.to_ascii_lowercase().contains("italic")
    {
        None
    } else {
        Some(name.to_owned())
    }
}

#[cfg(windows)]
pub fn installed_fonts() -> Vec<String> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER, enums::HKEY_LOCAL_MACHINE};

    let mut fonts = BTreeSet::new();
    let locations = [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts",
        ),
        (
            HKEY_CURRENT_USER,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts",
        ),
    ];
    for (root, path) in locations {
        if let Ok(key) = RegKey::predef(root).open_subkey(path) {
            for (name, _) in key.enum_values().flatten() {
                if let Some(clean) = clean_registered_font_name(&name) {
                    fonts.insert(clean);
                }
            }
        }
    }
    for fallback in FALLBACK_FONTS {
        fonts.insert((*fallback).to_owned());
    }
    fonts.into_iter().collect()
}

#[cfg(not(windows))]
pub fn installed_fonts() -> Vec<String> {
    FALLBACK_FONTS
        .iter()
        .map(|font| (*font).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_font_format_suffix() {
        assert_eq!(
            clean_registered_font_name("Georgia (TrueType)").as_deref(),
            Some("Georgia")
        );
    }

    #[test]
    fn skips_vertical_and_style_variant_entries() {
        assert_eq!(clean_registered_font_name("@Microsoft YaHei"), None);
        assert_eq!(clean_registered_font_name("Arial Bold (TrueType)"), None);
    }
}
