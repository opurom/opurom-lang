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
        "home" => "M14.5 20.5 V13.5 A1 1 0 0 0 14 12.5 H10 A1 1 0 0 0 9.5 13.5 V20.5 M4 11 A2 2 0 0 1 4.5 9.5 L11 4 A2 2 0 0 1 13 4 L19.5 9.5 A2 2 0 0 1 20 11 V19 A2 2 0 0 1 18 20.5 H6 A2 2 0 0 1 4 19 Z",
        "settings" => "M21 6.5 H11 M14.5 17.5 H4 M14.5 17.5 A3.5 3.5 0 1 0 21 17.5 A3.5 3.5 0 1 0 14.5 17.5 M3 6.5 A3.5 3.5 0 1 0 10 6.5 A3.5 3.5 0 1 0 3 6.5",
        "star" => "M11.5 4 A0.5 0.5 0 0 1 12.5 4 L14.5 8 A2 2 0 0 0 16 9 L20.5 9.5 A0.5 0.5 0 0 1 20.5 10.5 L17.5 13.5 A2 2 0 0 0 17 15.5 L17.5 20 A0.5 0.5 0 0 1 17 20.5 L13 18.5 A2 2 0 0 0 11 18.5 L7 20.5 A0.5 0.5 0 0 1 6.5 20 L7 15.5 A2 2 0 0 0 6.5 13.5 L3.5 10.5 A0.5 0.5 0 0 1 3.5 9.5 L8 9 A2 2 0 0 0 9.5 8 Z",
        "search" => "M3 11 A8 8 0 1 0 19 11 A8 8 0 1 0 3 11 M21 21 L16.5 16.5",
        "plus" => "M3 12 H21 M12 3 V21",
        "minus" => "M3 12 H21",
        "check" => "M21 6 L8.5 18 L3 12.5",
        "close" => "M21 3 L3 21 M3 3 L21 21",
        "arrow_back" => "M12 21 L3 12 L12 3 M21 12 H3",
        "arrow_forward" => "M3 12 H21 M12 3 L21 12 L12 21",
        "refresh" => "M4.5 12 A7.5 7.5 0 0 1 12 4.5 A8 8 0 0 1 17.5 7 L19.5 8.5 M19.5 4.5 V8.5 H15.5 M19.5 12 A7.5 7.5 0 0 1 12 19.5 A8 8 0 0 1 6.5 17 L4.5 15.5 M8.5 15.5 H4.5 V19.5",
        "lock" => "M6 11 H18 A2 2 0 0 1 20 12.5 V19 A2 2 0 0 1 18 20.5 H6 A2 2 0 0 1 4 19 V12.5 A2 2 0 0 1 6 11 Z M7.5 11 V7.5 A4.5 4.5 0 0 1 16.5 7.5 V11",
        "user" => "M19 21 V19 A4 4 0 0 0 15 15 H9 A4 4 0 0 0 5 19 V21 M8 7 A4 4 0 1 0 16 7 A4 4 0 1 0 8 7",
        "bell" => "M10.5 19.5 A1.5 1.5 0 0 0 13.5 19.5 M4.5 14.5 A1 1 0 0 0 5 16 H19 A1 1 0 0 0 19.5 14.5 C18.5 13.5 17 12 17 8 A5 5 0 0 0 7 8 C7 12 5.5 13.5 4.5 14.5",
        "heart" => "M17.5 14.5 C19 13 20 11.5 20 10 A4.5 4.5 0 0 0 15.5 5.5 C14 5.5 13 5.5 11.5 7 L10.5 5.5 A4.5 4.5 0 0 0 3.5 10 C3.5 11.5 4.5 13 6 14.5 L11.5 20 Z",
        "info" => "M3 12 A9 9 0 1 0 21 12 A9 9 0 1 0 3 12 M12 15.5 V12 M12 8.5 H12",
        "warning" => "M20.5 17.5 L13.5 5.5 A1.5 1.5 0 0 0 10.5 5.5 L3.5 17.5 A1.5 1.5 0 0 0 5 20 H19 A1.5 1.5 0 0 0 20.5 17.5 M12 9.5 V13 M12 16.5 H12",
        "chevron_right" => "M7.5 21 L16.5 12 L7.5 3",
        "chevron_down" => "M3 7.5 L12 16.5 L21 7.5",
        "download" => "M20.5 14.5 V18.5 A2 2 0 0 1 18.5 20.5 H5.5 A2 2 0 0 1 3.5 18.5 V14.5 M7 10 L12 14.5 L17 10 M12 14.5 L12 3",
        "upload" => "M20.5 14.5 V18.5 A2 2 0 0 1 18.5 20.5 H5.5 A2 2 0 0 1 3.5 18.5 V14.5 M17 8 L12 3 L7 8 M12 3 L12 14.5",
        "play" => "M5 3 L19 12 L5 21 L5 3 Z",
        "pause" => "M15.5 3 H17.5 A1 1 0 0 1 18.5 4.5 V19.5 A1 1 0 0 1 17.5 21 H15.5 A1 1 0 0 1 14 19.5 V4.5 A1 1 0 0 1 15.5 3 Z M6.5 3 H8.5 A1 1 0 0 1 10 4.5 V19.5 A1 1 0 0 1 8.5 21 H6.5 A1 1 0 0 1 5.5 19.5 V4.5 A1 1 0 0 1 6.5 3 Z",
        "edit" => "M20.5 7.5 A1 1 0 0 0 16.5 3.5 L4.5 15.5 A2 2 0 0 0 4.5 16.5 L3 20.5 A0.5 0.5 0 0 0 3.5 21 L7.5 19.5 A2 2 0 0 0 8.5 19.5 Z M14.5 5.5 L18.5 9.5",
        "delete" => "M4 6.5 H20 M18.5 6.5 V19 C18.5 20 17.5 21 16.5 21 H7.5 C6.5 21 5.5 20 5.5 19 V6.5 M8.5 6.5 V5 C8.5 4 9.5 3 10 3 H14 C14.5 3 15.5 4 15.5 5 V6.5",
        "eye" => "M3.5 12.5 A1 1 0 0 1 3.5 11.5 A9.5 9.5 0 0 1 20.5 11.5 A1 1 0 0 1 20.5 12.5 A9.5 9.5 0 0 1 3.5 12.5 M9.5 12 A2.5 2.5 0 1 0 14.5 12 A2.5 2.5 0 1 0 9.5 12",
        "calendar" => "M8.5 3 V6.5 M15.5 3 V6.5 M6 5 H18 A2 2 0 0 1 20 6.5 V19 A2 2 0 0 1 18 20.5 H6 A2 2 0 0 1 4 19 V6.5 A2 2 0 0 1 6 5 Z M4 10 H20",
        "location" => "M19 10 C19 14.5 14 19 12.5 20.5 A1 1 0 0 1 11.5 20.5 C10 19 5 14.5 5 10 A7 7 0 0 1 19 10 M9.5 10 A2.5 2.5 0 1 0 14.5 10 A2.5 2.5 0 1 0 9.5 10",
        "folder" => "M19 19.5 A1.5 1.5 0 0 0 20.5 17.5 V9 A1.5 1.5 0 0 0 19 7.5 H12 A1.5 1.5 0 0 1 10.5 6.5 L10 5.5 A1.5 1.5 0 0 0 8.5 4.5 H5 A1.5 1.5 0 0 0 3.5 6.5 V17.5 A1.5 1.5 0 0 0 5 19.5 Z",
        "mail" => "M5 5 H19 A1.5 1.5 0 0 1 20.5 7 V17 A1.5 1.5 0 0 1 19 19 H5 A1.5 1.5 0 0 1 3.5 17 V7 A1.5 1.5 0 0 1 5 5 Z M20.5 7.5 L13 12.5 A1.5 1.5 0 0 1 11 12.5 L3.5 7.5",
        "chat" => "M20.5 15 A2 2 0 0 1 18.5 17 H7 L3.5 21 V5.5 A2 2 0 0 1 5.5 3.5 H18.5 A2 2 0 0 1 20.5 5.5 Z",
        _ => return None,
    })
}





