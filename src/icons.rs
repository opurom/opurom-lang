// The canonical built-in icon name set. The renderer's icon module mirrors this
// list verbatim, so the compiler and the renderer agree on every accepted name;
// an unknown name is rejected here instead of silently drawing a fallback glyph.
pub const ICON_NAMES: &[&str] = &[
    "home",
    "settings",
    "star",
    "search",
    "plus",
    "minus",
    "check",
    "close",
    "arrow_back",
    "arrow_forward",
    "refresh",
    "lock",
    "user",
    "bell",
    "heart",
    "info",
    "warning",
    "chevron_right",
    "chevron_down",
    "download",
    "upload",
    "play",
    "pause",
    "edit",
    "delete",
    "eye",
    "calendar",
    "location",
    "folder",
    "mail",
    "chat",
];

pub fn is_icon(name: &str) -> bool {
    ICON_NAMES.contains(&name)
}

/// The help line for an unknown name: a sample name and a truncated set, so a
/// long list stays readable. The total count is always shown.
pub fn icon_help() -> String {
    let shown = ICON_NAMES
        .iter()
        .take(8)
        .map(|n| format!("'{n}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "write a built-in icon name such as '{}'; the set has {} names: {}, ...",
        ICON_NAMES[0],
        ICON_NAMES.len(),
        shown
    )
}

/// The canonical 24x24 stroke path for a built-in icon name. The data is the
/// same geometry the web DOM emitter ships, so every emitter draws one glyph
/// from one source. `None` means the name is not in the canonical set.
pub fn icon_svg_path(name: &str) -> Option<&'static str> {
    Some(match name {
        "home" => "M3 11 L12 3 L21 11 V20 H15 V14 H9 V20 H3 Z",
        "settings" => {
            "M12 8 A4 4 0 1 0 12 16 A4 4 0 1 0 12 8 Z M12 2 V5 M12 19 V22 M2 12 H5 M19 12 H22 \
             M4.9 4.9 L6.6 6.6 M17.4 17.4 L19.1 19.1 M19.1 4.9 L17.4 6.6 M6.6 17.4 L4.9 19.1"
        }
        "star" => "M12 3 L14.9 9.1 L21.5 10.1 L16.6 14.7 L17.9 21.2 L12 17.7 L6.1 21.2 L7.4 14.7 L2.5 10.1 L9.1 9.1 Z",
        "search" => "M11 4 A7 7 0 1 0 11 18 A7 7 0 1 0 11 4 Z M20 20 L16 16",
        "plus" => "M12 5 V19 M5 12 H19",
        "minus" => "M5 12 H19",
        "check" => "M5 12.5 L10 17.5 L19 7.5",
        "close" => "M6 6 L18 18 M18 6 L6 18",
        "arrow_back" => "M19 12 H5 M11 6 L5 12 L11 18",
        "arrow_forward" => "M5 12 H19 M13 6 L19 12 L13 18",
        "refresh" => "M20 12 A8 8 0 1 1 18.4 5.6 M20 4 V8 H16",
        "lock" => "M7 11 V8 A5 5 0 0 1 17 8 V11 H20 V20 H4 V11 H7 Z",
        "user" => "M12 12 A4 4 0 1 0 12 4 A4 4 0 1 0 12 12 Z M4 21 C4 16.5 8 14 12 14 C16 14 20 16.5 20 21",
        "bell" => "M6 16 V11 A6 6 0 0 1 18 11 V16 L20 20 H4 L6 16 Z M10 20 A2 2 0 0 0 14 20",
        "heart" => "M12 20 C4 14 4 8.5 8 6.5 C10.5 5 12 6.5 12 8.5 C12 6.5 13.5 5 16 6.5 C20 8.5 20 14 12 20 Z",
        "info" => "M12 4 A8 8 0 1 0 12 20 A8 8 0 1 0 12 4 Z M12 11 V17 M12 8 V8.2",
        "warning" => "M12 4 L22 20 H2 Z M12 10 V15 M12 17.5 V18",
        "chevron_right" => "M9 6 L15 12 L9 18",
        "chevron_down" => "M6 9 L12 15 L18 9",
        "download" => "M12 3 V14 M7 11 L12 16 L17 11 M5 20 H19",
        "upload" => "M12 20 V7 M7 10 L12 5 L17 10 M5 20 H19",
        "play" => "M7 5 L19 12 L7 19 Z",
        "pause" => "M8 5 V19 M16 5 V19",
        "edit" => "M4 20 H16 M14.5 5.5 L20 11 L12 19 H7.5 V13 Z",
        "delete" => "M5 7 H19 M9 7 V5 H15 V7 M10 7 V20 H14 V7",
        "eye" => "M2 12 C5 6 19 6 22 12 C19 18 5 18 2 12 Z M12 9.5 V14.5",
        "calendar" => "M4 6 H20 V20 H4 Z M4 10 H20 M9 4 V8 M15 4 V8",
        "location" => "M12 21 C6 14.5 6 9 12 4.5 C18 9 18 14.5 12 21 Z M12 13 A3 3 0 1 0 12 19 A3 3 0 1 0 12 13",
        "folder" => "M3 7 H9 L11 9 H21 V20 H3 Z",
        "mail" => "M3 6 H21 V18 H3 Z M3.5 7 L12 13 L20.5 7",
        "chat" => "M4 5 H20 V16 H13 L9 20 V16 H4 Z",
        _ => return None,
    })
}
