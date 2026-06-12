//! One-shot config schema migration v2 → v3 (spec §7): widget settings
//! move into the dedicated `widgets` store. The canonical case is the
//! weather widget's old global overlay fields.

use crate::config::{AppConfig, ConfigError, CURRENT_SCHEMA_VERSION};
use std::path::Path;

/// In-memory lift. Returns `true` if anything changed. Never removes the
/// legacy overlay fields (kept for one release; readers prefer the store).
pub fn migrate_to_v3(cfg: &mut AppConfig) -> bool {
    if cfg.schema_version >= CURRENT_SCHEMA_VERSION {
        return false;
    }
    if let Some((lat, lon)) = cfg.overlay.weather_location {
        let place = cfg.overlay.weather_place.clone().unwrap_or_default();
        let celsius = cfg.overlay.weather_celsius;
        let bag = cfg.widgets.global.entry("weather".to_string()).or_default();
        bag.entry("location".to_string()).or_insert_with(|| {
            serde_json::json!({
                "name": place,
                "lat": lat,
                "lon": lon,
            })
        });
        bag.entry("units".to_string())
            .or_insert_with(|| serde_json::json!(if celsius { "c" } else { "f" }));
    }
    cfg.schema_version = CURRENT_SCHEMA_VERSION;
    true
}

/// On-disk migration: writes `config.json.v2.bak` with the original bytes,
/// then atomically rewrites the file as v3. Idempotent. Call this from the
/// settings app at startup, before its first save.
pub fn migrate_file_to_v3(path: &Path) -> Result<bool, ConfigError> {
    let original = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(ConfigError::Io(e)),
    };
    let mut cfg: AppConfig = serde_json::from_str(&original).map_err(ConfigError::Parse)?;
    if !migrate_to_v3(&mut cfg) {
        return Ok(false);
    }
    let bak = path.with_extension("json.v2.bak");
    std::fs::write(&bak, &original).map_err(ConfigError::Io)?;
    let json = serde_json::to_string_pretty(&cfg).map_err(ConfigError::Parse)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(ConfigError::Io)?;
    std::fs::rename(&tmp, path).map_err(ConfigError::Io)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("oxidemx-migrate-{}-{}", name, std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join("config.json")
    }

    const V2: &str = r#"{
        "theme": "dracula",
        "overlay": {
            "weather_location": [59.91, 10.75],
            "weather_place": "Oslo, NO",
            "weather_celsius": true
        }
    }"#;

    #[test]
    fn in_memory_migration_lifts_weather_settings() {
        let mut cfg: AppConfig = serde_json::from_str(V2).unwrap();
        assert_eq!(cfg.schema_version, 2);
        assert!(migrate_to_v3(&mut cfg));
        assert_eq!(cfg.schema_version, 3);
        let bag = &cfg.widgets.global["weather"];
        assert_eq!(bag["location"]["lat"], serde_json::json!(59.91));
        assert_eq!(bag["location"]["name"], serde_json::json!("Oslo, NO"));
        assert_eq!(bag["units"], serde_json::json!("c"));
        // idempotent
        assert!(!migrate_to_v3(&mut cfg));
    }

    #[test]
    fn migration_does_not_clobber_existing_bags() {
        let mut cfg: AppConfig = serde_json::from_str(V2).unwrap();
        cfg.widgets.global.insert("weather".into(), {
            let mut b = crate::widgets::JsonBag::new();
            b.insert("units".into(), serde_json::json!("f"));
            b
        });
        migrate_to_v3(&mut cfg);
        // user's existing value wins over the lifted one
        assert_eq!(
            cfg.widgets.global["weather"]["units"],
            serde_json::json!("f")
        );
    }

    #[test]
    fn file_migration_writes_bak_and_rewrites() {
        let path = tmp("file");
        std::fs::write(&path, V2).unwrap();
        assert!(migrate_file_to_v3(&path).unwrap());
        let bak = path.with_extension("json.v2.bak");
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), V2);
        let migrated: AppConfig =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(migrated.schema_version, 3);
        // second run is a no-op
        assert!(!migrate_file_to_v3(&path).unwrap());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn load_from_serves_v3_view_without_touching_disk() {
        let path = tmp("load");
        std::fs::write(&path, V2).unwrap();
        let cfg = AppConfig::load_from(&path).unwrap();
        assert_eq!(cfg.schema_version, 3);
        assert!(cfg.widgets.global.contains_key("weather"));
        // file untouched
        assert_eq!(std::fs::read_to_string(&path).unwrap(), V2);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
