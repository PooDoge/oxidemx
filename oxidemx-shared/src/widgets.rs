//! Two-bag widget settings store (spec §6/§7): `global` holds one bag per
//! widget id shared by every instance; `instances` holds partial override
//! bags keyed by `<page-slug>.slot<N>`. Reads merge defaults ← global
//! always, then the instance bag on top only when the slice's scope is
//! `Instance`; `scope` also selects where the options card writes.

use crate::config::WidgetScope;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One settings bag — JSON object semantics, deterministic order.
pub type JsonBag = serde_json::Map<String, serde_json::Value>;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WidgetStore {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub global: BTreeMap<String, JsonBag>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub instances: BTreeMap<String, BTreeMap<String, JsonBag>>,
}

impl WidgetStore {
    pub fn is_empty(&self) -> bool {
        self.global.is_empty() && self.instances.is_empty()
    }

    /// Effective settings for one widget instance (spec §6).
    pub fn resolve(
        &self,
        widget_id: &str,
        instance_key: Option<&str>,
        scope: WidgetScope,
        defaults: &JsonBag,
    ) -> JsonBag {
        let mut out = defaults.clone();
        if let Some(g) = self.global.get(widget_id) {
            for (k, v) in g {
                out.insert(k.clone(), v.clone());
            }
        }
        if scope == WidgetScope::Instance {
            if let Some(i) = instance_key
                .and_then(|key| self.instances.get(key))
                .and_then(|bags| bags.get(widget_id))
            {
                for (k, v) in i {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
        out
    }

    /// Global→slice toggle: seed the instance bag as a copy of the current
    /// resolved values so it diverges from there (spec §6 table).
    pub fn seed_instance(&mut self, widget_id: &str, instance_key: &str, defaults: &JsonBag) {
        let resolved = self.resolve(widget_id, Some(instance_key), WidgetScope::Global, defaults);
        self.instances
            .entry(instance_key.to_string())
            .or_default()
            .insert(widget_id.to_string(), resolved);
    }

    /// Slice moved/swapped/page renamed — carry its override bag along.
    pub fn rekey_instance(&mut self, old: &str, new: &str) {
        if let Some(bags) = self.instances.remove(old) {
            self.instances.insert(new.to_string(), bags);
        }
    }
}

/// `<page-slug>.slot<N>` — human-readable instance key (spec §7).
pub fn instance_key(page_name: &str, slot: usize) -> String {
    format!("{}.slot{slot}", slugify(page_name))
}

fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = true; // suppress leading dash
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "page".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WidgetScope;
    use serde_json::json;

    fn bag(pairs: &[(&str, serde_json::Value)]) -> JsonBag {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn resolution_merges_defaults_global_instance() {
        let defaults = bag(&[("units", json!("c")), ("refresh", json!(900))]);
        let mut store = WidgetStore::default();
        store
            .global
            .insert("weather".into(), bag(&[("units", json!("f"))]));
        store.instances.insert(
            "apps.slot4".into(),
            [("weather".to_string(), bag(&[("refresh", json!(300))]))]
                .into_iter()
                .collect(),
        );

        // instance scope: all three layers
        let r = store.resolve(
            "weather",
            Some("apps.slot4"),
            WidgetScope::Instance,
            &defaults,
        );
        assert_eq!(r.get("units"), Some(&json!("f"))); // global beat default
        assert_eq!(r.get("refresh"), Some(&json!(300))); // instance beat global

        // global scope: instance layer skipped
        let r = store.resolve(
            "weather",
            Some("apps.slot4"),
            WidgetScope::Global,
            &defaults,
        );
        assert_eq!(r.get("refresh"), Some(&json!(900))); // default survives
    }

    #[test]
    fn seed_instance_copies_resolved_values() {
        let defaults = bag(&[("units", json!("c"))]);
        let mut store = WidgetStore::default();
        store
            .global
            .insert("weather".into(), bag(&[("units", json!("f"))]));
        store.seed_instance("weather", "apps.slot4", &defaults);
        assert_eq!(
            store.instances["apps.slot4"]["weather"].get("units"),
            Some(&json!("f"))
        );
    }

    #[test]
    fn rekey_moves_the_whole_instance_bag() {
        let mut store = WidgetStore::default();
        store.instances.insert(
            "apps.slot4".into(),
            [("weather".to_string(), bag(&[("units", json!("f"))]))]
                .into_iter()
                .collect(),
        );
        store.rekey_instance("apps.slot4", "apps.slot2");
        assert!(!store.instances.contains_key("apps.slot4"));
        assert_eq!(
            store.instances["apps.slot2"]["weather"].get("units"),
            Some(&json!("f"))
        );
    }

    #[test]
    fn instance_key_format() {
        assert_eq!(instance_key("Apps", 4), "apps.slot4");
        assert_eq!(instance_key("My Dev Page!", 0), "my-dev-page.slot0");
    }
}
