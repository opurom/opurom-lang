//! Locale and theme token conflict gate.
//!
//! Locale tags follow the Unicode language identifier rules that Flutter's
//! engine `Locale` documents: a lowercase primary language subtag of two to
//! eight letters, region and script subtags of bounded length
//! (`bin/cache/pkg/sky_engine/lib/ui/platform_dispatcher.dart:2878-2902`,
//! Flutter 3.50.0). Opurom accepts the BCP-47 shape
//! `^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$` and rejects anything else with a
//! positioned diagnostic. Theme tokens come from two sources: inline
//! `tokens { name: "#RRGGBB" }` entries and imported JSON files. This gate pins
//! the override order, the duplicate rules and the invalid-tag diagnostics.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

static CASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_opuromc"))
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn out_dir(label: &str) -> PathBuf {
    let dir = workspace().join("target/test-out").join(format!(
        "{}-{}",
        label,
        CASE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).expect("create test output dir");
    dir
}

fn compile(src: &PathBuf, out: &PathBuf) -> std::process::Output {
    bin()
        .arg("compile")
        .arg(src)
        .arg("-o")
        .arg(out)
        .output()
        .expect("run opuromc compile")
}

fn check(src: &PathBuf) -> String {
    let result = bin()
        .arg("check")
        .arg(src)
        .output()
        .expect("run opuromc check");
    assert_eq!(result.status.code(), Some(1), "source must be rejected");
    String::from_utf8_lossy(&result.stderr).into_owned()
}

#[test]
fn inline_token_override_order_is_last_wins() {
    let dir = out_dir("theme-token-order");
    let src = dir.join("order.opu");
    let out = dir.join("order.opuir.json");
    fs::write(
        &src,
        "app Theme {\n  theme { tokens { primary: \"#111111\" primary: \"#222222\" success: \"#333333\" } }\n  Text(\"x\")\n}\n",
    )
    .expect("write source");
    let result = compile(&src, &out);
    assert!(
        result.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&out).expect("read IR")).expect("parse IR");
    assert_eq!(doc["theme"]["tokens"]["primary"], "#222222");
    assert_eq!(doc["theme"]["tokens"]["success"], "#333333");
}

#[test]
fn imported_tokens_are_sorted_and_conflict_with_inline_tokens() {
    let dir = out_dir("theme-token-import");
    let theme_dir = dir.join("theme");
    fs::create_dir_all(&theme_dir).expect("create theme dir");
    fs::write(&theme_dir.join("b.json"), "{\"md-bbb\": \"#111111\"}").expect("write b.json");
    fs::write(&theme_dir.join("a.json"), "{\"md-aaa\": \"#222222\"}").expect("write a.json");

    let ok = dir.join("ok.opu");
    let ok_ir = dir.join("ok.opuir.json");
    fs::write(
        &ok,
        "app Theme {\n  theme { tokens { import \"theme/b.json\" import \"theme/a.json\" } }\n  Text(\"x\")\n}\n",
    )
    .expect("write source");
    let result = compile(&ok, &ok_ir);
    assert!(
        result.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: serde_json::Value =
        serde_json::from_slice(&fs::read(&ok_ir).expect("read IR")).expect("parse IR");
    assert_eq!(doc["theme"]["tokens"]["md-aaa"], "#222222");
    assert_eq!(doc["theme"]["tokens"]["md-bbb"], "#111111");

    let conflict = dir.join("conflict.opu");
    let conflict_ir = dir.join("conflict.opuir.json");
    fs::write(
        &conflict,
        "app Theme {\n  theme { tokens { import \"theme/a.json\" md-aaa: \"#654321\" } }\n  Text(\"x\")\n}\n",
    )
    .expect("write source");
    let rejected = compile(&conflict, &conflict_ir);
    assert_eq!(rejected.status.code(), Some(1), "conflict must fail");
    let stderr = String::from_utf8_lossy(&rejected.stderr);
    assert!(
        stderr.contains("duplicate token 'md-aaa'") && stderr.contains("  help: "),
        "conflict diagnostic: {stderr}"
    );
    assert!(!conflict_ir.exists(), "rejected theme must not write IR");
}

#[test]
fn imported_token_outside_md_namespace_is_rejected() {
    let dir = out_dir("theme-token-namespace");
    let theme_dir = dir.join("theme");
    fs::create_dir_all(&theme_dir).expect("create theme dir");
    fs::write(
        &theme_dir.join("mixed.json"),
        "{\"md-ok\": \"#123456\", \"other\": \"#654321\"}",
    )
    .expect("write mixed.json");
    let src = dir.join("mixed.opu");
    let out = dir.join("mixed.opuir.json");
    fs::write(
        &src,
        "app Theme {\n  theme { tokens { import \"theme/mixed.json\" } }\n  Text(\"x\")\n}\n",
    )
    .expect("write source");
    let result = compile(&src, &out);
    assert_eq!(result.status.code(), Some(1), "non-md token must fail");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("imported token 'other' must use the 'md-' namespace"),
        "namespace diagnostic: {stderr}"
    );
    assert!(!out.exists(), "rejected import must not write IR");
}

#[test]
fn invalid_locale_tags_are_rejected_with_a_stable_pattern() {
    let dir = out_dir("locale-tag");
    for tag in ["EN", "abcd", "e", "en-abcdefghij", "EN-us"] {
        let src = dir.join(format!("{}.opu", tag.replace('-', "_")));
        let out = dir.join(format!("{}.opuir.json", tag.replace('-', "_")));
        fs::write(
            &src,
            format!(
                "app Locale {{\n  locale {{ active: \"{tag}\", supported: [\"{tag}\"] }}\n  Text(\"x\")\n}}\n"
            ),
        )
        .expect("write source");
        let stderr = check(&src);
        assert!(
            stderr.contains(&format!("invalid locale tag '{tag}'"))
                && stderr.contains("^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$"),
            "locale tag diagnostic for {tag}: {stderr}"
        );
        assert!(!out.exists(), "rejected locale must not write IR");
    }
}

#[test]
fn valid_locale_tags_are_accepted() {
    let dir = out_dir("locale-tag-ok");
    for tag in ["en", "zh-CN", "zh-Hant-TW", "en-Latn"] {
        let src = dir.join(format!("{}.opu", tag.replace('-', "_")));
        let out = dir.join(format!("{}.opuir.json", tag.replace('-', "_")));
        fs::write(
            &src,
            format!(
                "app Locale {{\n  locale {{ active: \"{tag}\", supported: [\"{tag}\"] }}\n  Text(\"x\")\n}}\n"
            ),
        )
        .expect("write source");
        let result = compile(&src, &out);
        assert!(
            result.status.success(),
            "{tag} must compile: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let doc: serde_json::Value =
            serde_json::from_slice(&fs::read(&out).expect("read IR")).expect("parse IR");
        assert_eq!(doc["locale"]["active"], tag);
    }
}

#[test]
fn locale_consistency_rules_are_enforced() {
    let dir = out_dir("locale-consistency");
    let dup = dir.join("dup.opu");
    fs::write(
        &dup,
        "app Locale {\n  locale { active: \"en\", supported: [\"en\", \"en\"] }\n  Text(\"x\")\n}\n",
    )
    .expect("write source");
    assert!(
        check(&dup).contains("duplicate supported locale 'en'"),
        "duplicate supported locale must be rejected"
    );

    let dup_active = dir.join("dup-active.opu");
    fs::write(
        &dup_active,
        "app Locale {\n  locale { active: \"en\", supported: [\"en\"] active: \"fr\" }\n  Text(\"x\")\n}\n",
    )
    .expect("write source");
    assert!(
        check(&dup_active).contains("duplicate 'active' in locale"),
        "duplicate active must be rejected"
    );

    let missing = dir.join("missing.opu");
    fs::write(
        &missing,
        "app Locale {\n  locale { active: \"fr\", supported: [\"en\"] }\n  Text(\"x\")\n}\n",
    )
    .expect("write source");
    let stderr = check(&missing);
    assert!(
        stderr.contains("active locale 'fr' is not in 'supported'")
            && stderr.contains("add 'fr' to the supported list"),
        "unsupported active must be rejected: {stderr}"
    );
}

#[test]
fn duplicate_catalog_key_in_one_locale_is_rejected() {
    let dir = out_dir("locale-catalog");
    let src = dir.join("catalog.opu");
    let out = dir.join("catalog.opuir.json");
    fs::write(
        &src,
        "app Locale {\n  catalog en { key: \"v\" key: \"w\" }\n  Text(\"x\")\n}\n",
    )
    .expect("write source");
    let result = compile(&src, &out);
    assert_eq!(result.status.code(), Some(1), "duplicate key must fail");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("duplicate catalog key 'key' in 'en'")
            && stderr.contains("write each key once per locale"),
        "catalog diagnostic: {stderr}"
    );
    assert!(!out.exists(), "rejected catalog must not write IR");
}

#[test]
fn long_token_and_catalog_keys_lower_deterministically() {
    let dir = out_dir("theme-token-long-key");
    let src = dir.join("long.opu");
    let first = dir.join("first.opuir.json");
    let second = dir.join("second.opuir.json");
    let long = "a".repeat(500);
    fs::write(
        &src,
        format!(
            "app Theme {{\n  theme {{ tokens {{ md-{long}: \"#123456\" }} }}\n  catalog en {{ {long}: \"v\" }}\n  Text(\"x\")\n}}\n"
        ),
    )
    .expect("write source");
    for out in [&first, &second] {
        let result = compile(&src, out);
        assert!(
            result.status.success(),
            "long key must compile: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(
        fs::read(&first).expect("read first IR"),
        fs::read(&second).expect("read second IR"),
        "long keys must lower deterministically"
    );
}
