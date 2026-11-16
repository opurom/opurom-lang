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
        "home" => "M4 11 L12 4 L20 11 M6 10 V20 H18 V10 M10 20 V14 H14 V20",
        "settings" => "M4 7 H20 M4 17 H20 M9 7 A2 2 0 1 0 9 3 A2 2 0 1 0 9 7 M15 17 A2 2 0 1 0 15 13 A2 2 0 1 0 15 17",
        "star" => "M12 3 L14.9 9.2 L21.5 10 L16.7 14.6 L18 21 L12 17.8 L6 21 L7.3 14.6 L2.5 10 L9.1 9.2 Z",
        "search" => "M11 4 A7 7 0 1 0 11 18 A7 7 0 1 0 11 4 M16.5 16.5 L21 21",
        "plus" => "M12 4 V20 M4 12 H20",
        "minus" => "M4 12 H20",
        "check" => "M4 12 L10 18 L20 6",
        "close" => "M5 5 L19 19 M19 5 L5 19",
        "arrow_back" => "M20 12 H4 M10 6 L4 12 L10 18",
        "arrow_forward" => "M4 12 H20 M14 6 L20 12 L14 18",
        "refresh" => "M20 12 A8 8 0 0 1 6 17 M4 12 A8 8 0 0 1 18 7 M20 3 V8 H15 M4 21 V16 H9",
        "lock" => "M5 11 H19 A2 2 0 0 1 21 13 V19 A2 2 0 0 1 19 21 H5 A2 2 0 0 1 3 19 V13 A2 2 0 0 1 5 11 Z M8 11 V7 A4 4 0 0 1 16 7 V11",
        "user" => "M12 4 A4 4 0 1 0 12 12 A4 4 0 1 0 12 4 M4 21 A8 8 0 0 1 20 21",
        "bell" => "M18 8 A6 6 0 0 0 6 8 C6 14 4 16 4 16 H20 C20 16 18 14 18 8 Z M10 19 A2 2 0 0 0 14 19",
        "heart" => "M12 21 C12 21 3 14.5 3 8.5 A4.5 4.5 0 0 1 12 6 A4.5 4.5 0 0 1 21 8.5 C21 14.5 12 21 12 21 Z",
        "info" => "M12 3 A9 9 0 1 0 12 21 A9 9 0 1 0 12 3 M12 11 V17 M12 7 V7.01",
        "warning" => "M12 3 L22 20 H2 Z M12 9 V14 M12 17 V17.01",
        "chevron_right" => "M9 5 L16 12 L9 19",
        "chevron_down" => "M5 9 L12 16 L19 9",
        "download" => "M12 3 V15 M7 11 L12 16 L17 11 M4 20 H20",
        "upload" => "M12 16 V4 M7 9 L12 4 L17 9 M4 20 H20",
        "play" => "M7 4 L20 12 L7 20 Z",
        "pause" => "M8 4 V20 M16 4 V20",
        "edit" => "M4 20 L5 15 L16 4 L20 8 L9 19 L4 20 Z M14 6 L18 10",
        "delete" => "M4 7 H20 M9 7 V4 H15 V7 M6 7 L7 21 H17 L18 7 M10 11 V17 M14 11 V17",
        "eye" => "M2 12 C5 6 19 6 22 12 C19 18 5 18 2 12 Z M12 9 A3 3 0 1 0 12 15 A3 3 0 1 0 12 9",
        "calendar" => "M4 6 H20 V20 H4 Z M4 11 H20 M8 3 V7 M16 3 V7",
        "location" => "M12 21 C12 21 5 14 5 10 A7 7 0 1 1 19 10 C19 14 12 21 12 21 Z M12 7 A3 3 0 1 0 12 13 A3 3 0 1 0 12 7",
        "folder" => "M3 7 H9 L11 9 H21 V19 H3 Z",
        "mail" => "M3 5 H21 V19 H3 Z M3 6 L12 13 L21 6",
        "chat" => "M4 5 H20 V17 H12 L7 21 V17 H4 Z",
        _ => return None,
    })
}



