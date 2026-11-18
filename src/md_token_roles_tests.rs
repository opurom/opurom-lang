use crate::md_token_roles::{is_color_role, COLOR_ROLE_COUNT};

#[test]
fn role_set_matches_the_material_token_manifest() {
    assert_eq!(COLOR_ROLE_COUNT, 49);
    assert!(is_color_role("primary"));
    assert!(is_color_role("surface-container-highest"));
    assert!(!is_color_role("primaryx"));
}
