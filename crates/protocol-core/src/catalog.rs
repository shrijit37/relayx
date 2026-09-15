//! Vendored models.dev catalog -- typed Rust mirror of the JSON shape.
//!
//! Loaded once via `get_catalog()` using a `OnceLock`. The JSON file is
//! embedded at compile time with `include_str!`.

use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Modalities {
    pub input: Vec<String>,
    pub output: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Limit {
    pub context: u64,
    #[serde(default)]
    pub output: Option<u64>,
    #[serde(default)]
    pub input: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Cost {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Link {
    pub label: String,
    pub url: String,
    #[serde(default)]
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Benchmark {
    pub name: String,
    pub score: f64,
    #[serde(default)]
    pub metric: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub harness: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub dataset: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ModelDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    pub attachment: bool,
    pub reasoning: bool,
    pub tool_call: bool,
    #[serde(default)]
    pub structured_output: Option<bool>,
    #[serde(default)]
    pub temperature: Option<bool>,
    #[serde(default)]
    pub knowledge: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub last_updated: Option<String>,
    pub modalities: Modalities,
    pub open_weights: bool,
    pub limit: Limit,
    #[serde(default)]
    pub cost: Option<Cost>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub links: Option<Vec<Link>>,
    #[serde(default)]
    pub weights: Option<Vec<Link>>,
    #[serde(default)]
    pub benchmarks: Option<Vec<Benchmark>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProviderDef {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub env: Option<Vec<String>>,
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub doc: Option<String>,
    #[serde(default)]
    pub models: Option<HashMap<String, ModelDef>>,
}

/// Metadata about the vendored catalog snapshot.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CatalogSnapshotMeta {
    pub version: u64,
    pub last_updated: String,
    pub source: String,
}

impl Default for CatalogSnapshotMeta {
    fn default() -> Self {
        Self {
            version: 1,
            last_updated: String::from("unknown"),
            source: String::from("vendored"),
        }
    }
}

/// Snapshot of the model catalog keyed by `provider/model` id.
pub type CatalogSnapshot = HashMap<String, ModelDef>;

/// Top-level catalog structure matching the vendored JSON.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CatalogData {
    #[serde(default)]
    pub meta: CatalogSnapshotMeta,
    #[serde(default)]
    pub models: CatalogSnapshot,
    #[serde(default)]
    pub providers: Option<HashMap<String, ProviderDef>>,
}

impl CatalogData {
    /// Load the vendored catalog from the embedded JSON string.
    pub fn load_vendored() -> Result<Self, serde_json::Error> {
        let json = include_str!("../data/catalog.json");
        serde_json::from_str(json)
    }
}

/// Lazily-initialized global catalog. Loaded once on first access.
// ponytail: OnceLock is fine for read-only data; ArcSwap when hot-reload lands.
fn catalog_once() -> &'static OnceLock<CatalogData> {
    static LOCK: OnceLock<CatalogData> = OnceLock::new();
    &LOCK
}

/// Get a reference to the global catalog, initializing it on first call.
///
/// # Panics
///
/// Panics if the vendored JSON fails to parse (should never happen with a
/// checked-in file).
pub fn get_catalog() -> &'static CatalogData {
    catalog_once().get_or_init(|| match CatalogData::load_vendored() {
        Ok(data) => data,
        Err(e) => panic!(
            "vendored catalog.json failed to parse: {e} -- check crates/protocol-core/data/catalog.json"
        ),
    })
}

// -- Tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_vendored_parses() {
        let data = match CatalogData::load_vendored() {
            Ok(d) => d,
            Err(e) => panic!("catalog parse failed: {e}"),
        };
        assert!(
            data.models.len() > 100,
            "expected >100 models, got {}",
            data.models.len()
        );
    }

    #[test]
    fn get_catalog_returns_static_ref() {
        let a = get_catalog();
        let b = get_catalog();
        let a_ptr = a as *const CatalogData;
        let b_ptr = b as *const CatalogData;
        assert_eq!(a_ptr, b_ptr);
    }

    #[test]
    fn known_models_exist() {
        let cat = get_catalog();
        assert!(
            cat.models.contains_key("openai/gpt-4o"),
            "openai/gpt-4o missing"
        );
        assert!(
            cat.models.contains_key("anthropic/claude-sonnet-4-6"),
            "anthropic/claude-sonnet-4-6 missing"
        );
    }

    #[test]
    fn model_fields_deserialize() {
        let cat = get_catalog();
        let m = match cat.models.get("openai/gpt-4o") {
            Some(v) => v,
            None => panic!("openai/gpt-4o missing"),
        };
        assert!(!m.id.is_empty());
        assert!(m.limit.context > 0);
        assert!(!m.modalities.input.is_empty());
    }

    #[test]
    fn snapshot_meta_defaults_when_missing() {
        let cat = get_catalog();
        assert_eq!(cat.meta.source, "vendored");
        assert_eq!(cat.meta.version, 1);
    }
}
