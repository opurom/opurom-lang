//! Design-token role names shared by the compiler front end.
//!
//! Source of truth: `opurom-ui/theme/material-tokens.json` (49 roles per mode),
//! generated from Flutter framework 3.50.0 (`ab2912035e`)
//! `packages/flutter/lib/src/material/color_scheme.dart:447-1155`.
//! This module exists so the compiler accepts exactly the role set that the
//! renderer resolves in `opurom-render/src/ir.rs::m3_color`; no runtime
//! interpretation is introduced.

pub const COLOR_ROLES: &[&str] = &[
    "background",
    "error",
    "error-container",
    "inverse-primary",
    "inverse-surface",
    "on-background",
    "on-error",
    "on-error-container",
    "on-inverse-surface",
    "on-primary",
    "on-primary-container",
    "on-primary-fixed",
    "on-primary-fixed-variant",
    "on-secondary",
    "on-secondary-container",
    "on-secondary-fixed",
    "on-secondary-fixed-variant",
    "on-surface",
    "on-surface-variant",
    "on-tertiary",
    "on-tertiary-container",
    "on-tertiary-fixed",
    "on-tertiary-fixed-variant",
    "outline",
    "outline-variant",
    "primary",
    "primary-container",
    "primary-fixed",
    "primary-fixed-dim",
    "scrim",
    "secondary",
    "secondary-container",
    "secondary-fixed",
    "secondary-fixed-dim",
    "shadow",
    "surface",
    "surface-bright",
    "surface-container",
    "surface-container-high",
    "surface-container-highest",
    "surface-container-low",
    "surface-container-lowest",
    "surface-dim",
    "surface-tint",
    "surface-variant",
    "tertiary",
    "tertiary-container",
    "tertiary-fixed",
    "tertiary-fixed-dim",
];

pub const COLOR_ROLE_COUNT: usize = COLOR_ROLES.len();

pub fn is_color_role(role: &str) -> bool {
    COLOR_ROLES.contains(&role)
}
