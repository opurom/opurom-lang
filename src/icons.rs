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
