use std::collections::HashSet;
use std::sync::OnceLock;

use serde_json::{Map as JsonMap, Value as Json};

use crate::diag::{DResult, Diagnostic};
use crate::lower::KINDS;

/// Vendored copy of opurom-spec/components.json so the crate builds standalone.
const CATALOG_JSON: &str = include_str!("catalog.json");

#[derive(Clone, Debug)]
pub struct Component {
    pub name: String,
    pub primitive: String,
    pub category: String,
    pub props: JsonMap<String, Json>,
}

#[derive(Debug)]
pub struct Catalog {
    pub components: Vec<Component>,
}

impl Catalog {
    pub fn get(&self, name: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.name == name)
    }
}

fn valid_name(s: &str) -> bool {
    let mut cs = s.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_uppercase())
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn valid_prop_key(s: &str) -> bool {
    let mut cs = s.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Validate the whole catalog once, at compile time: names, primitives and default props.
fn parse_catalog(text: &str) -> Result<Catalog, String> {
    let doc: Json = serde_json::from_str(text).map_err(|e| format!("invalid catalog JSON: {e}"))?;
    let list = doc
        .get("components")
        .and_then(Json::as_array)
        .ok_or_else(|| "catalog is missing the 'components' array".to_string())?;
    let mut seen = HashSet::new();
    let mut components = Vec::with_capacity(list.len());
    for (i, entry) in list.iter().enumerate() {
        let tuple = entry.as_array().filter(|t| t.len() == 4).ok_or_else(|| {
            format!("catalog entry {i} is not a [name, primitive, category, props] tuple")
        })?;
        let name = tuple[0]
            .as_str()
            .ok_or_else(|| format!("catalog entry {i} has a non-string name"))?;
        let primitive = tuple[1]
            .as_str()
            .ok_or_else(|| format!("catalog component '{name}' has a non-string primitive"))?;
        let category = tuple[2]
            .as_str()
            .ok_or_else(|| format!("catalog component '{name}' has a non-string category"))?;
        let props = tuple[3]
            .as_object()
            .ok_or_else(|| format!("catalog component '{name}' has malformed defaultProps"))?;
        if !valid_name(name) {
            return Err(format!("catalog entry {i} has an invalid name '{name}'"));
        }
        if !seen.insert(name.to_string()) {
            return Err(format!("duplicate catalog name '{name}'"));
        }
        if !KINDS.contains(&primitive) {
            return Err(format!(
                "catalog component '{name}' has unknown primitive '{primitive}'"
            ));
        }
        if category.is_empty() {
            return Err(format!("catalog component '{name}' has an empty category"));
        }
        for (key, value) in props {
            if !valid_prop_key(key)
                || !(value.is_string() || value.is_number() || value.is_boolean())
            {
                return Err(format!(
                    "catalog component '{name}' has malformed prop '{key}'"
                ));
            }
        }
        components.push(Component {
            name: name.to_string(),
            primitive: primitive.to_string(),
            category: category.to_string(),
            props: props.clone(),
        });
    }
    if let Some(count) = doc.get("count").and_then(Json::as_u64) {
        if count != components.len() as u64 {
            return Err(format!(
                "catalog count {count} does not match {} components",
                components.len()
            ));
        }
    }
    if let Some(primitives) = doc.get("primitives").and_then(Json::as_u64) {
        let found: HashSet<&str> = components.iter().map(|c| c.primitive.as_str()).collect();
        if primitives != found.len() as u64 || primitives != KINDS.len() as u64 {
            return Err(format!(
                "catalog declares {primitives} primitives but has {} distinct kinds",
                found.len()
            ));
        }
    }
    Ok(Catalog { components })
}

static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();

pub fn catalog() -> DResult<&'static Catalog> {
    CATALOG
        .get_or_init(|| parse_catalog(CATALOG_JSON))
        .as_ref()
        .map_err(|e| {
            Diagnostic::file(
                "components.json",
                e.clone(),
                "fix the vendored catalog in src/catalog.json",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(components: Json) -> String {
        json!({ "components": components }).to_string()
    }

    fn entry(name: &str, primitive: &str, props: Json) -> Json {
        json!([name, primitive, "layout", props])
    }

    #[test]
    fn vendored_catalog_is_valid_and_large() {
        let catalog = parse_catalog(CATALOG_JSON).expect("vendored catalog");
        assert_eq!(catalog.components.len(), 240);
        let unique: HashSet<&str> = catalog
            .components
            .iter()
            .map(|c| c.primitive.as_str())
            .collect();
        assert_eq!(unique.len(), 100);
        assert_eq!(catalog.get("VStack").unwrap().primitive, "Column");
        assert_eq!(catalog.get("DataTable").unwrap().primitive, "Table");
        assert_eq!(catalog.get("Hint").unwrap().primitive, "Tooltip");
        assert_eq!(
            catalog.get("PrimaryButton").unwrap().props["color"],
            "#097FEB"
        );
    }

    #[test]
    fn rejects_duplicate_names() {
        let text = doc(json!([
            entry("VStack", "Column", json!({})),
            entry("VStack", "Row", json!({})),
        ]));
        assert!(parse_catalog(&text)
            .unwrap_err()
            .contains("duplicate catalog name 'VStack'"));
    }

    #[test]
    fn rejects_unknown_primitives() {
        let text = doc(json!([entry("Fancy", "Canvas", json!({}))]));
        assert!(parse_catalog(&text)
            .unwrap_err()
            .contains("unknown primitive 'Canvas'"));
    }

    #[test]
    fn rejects_malformed_props() {
        for props in [
            json!([]),
            json!({"gap": null}),
            json!({"gap": {"nested": 1}}),
        ] {
            let text = doc(json!([entry("VStack", "Column", props)]));
            assert!(parse_catalog(&text).is_err(), "props must be flat scalars");
        }
    }
}
