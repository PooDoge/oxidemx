//! Shared Open-Meteo geocoding lookup.
//!
//! Used by BOTH the legacy weather-widget location search on the
//! Settings tab and the widget options card's `location` option
//! control (spec §5) — one implementation, two consumers. Runs
//! `curl` in a blocking task: settings has no HTTP client
//! dependency and this is a rare, user-initiated call.

/// One geocoding hit. `name` is the display label
/// ("Oslo, Oslo, NO") — also what gets persisted.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoHit {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

/// Resolve a city name via Open-Meteo's keyless geocoding API.
pub async fn search(query: String) -> Result<Vec<GeoHit>, String> {
    tokio::task::spawn_blocking(move || {
        let encoded = percent_encode(query.trim());
        let url = format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={encoded}&count=5&language=en&format=json"
        );
        let out = std::process::Command::new("curl")
            .args(["-sm", "8", &url])
            .output()
            .map_err(|e| format!("curl failed to start: {e}"))?;
        if !out.status.success() {
            return Err("geocoding request failed (offline?)".to_string());
        }
        let v: serde_json::Value = serde_json::from_slice(&out.stdout)
            .map_err(|e| format!("geocoding response unreadable: {e}"))?;
        let hits = parse_results(&v);
        if hits.is_empty() {
            Err("no places matched".to_string())
        } else {
            Ok(hits)
        }
    })
    .await
    .map_err(|e| format!("geocoding task failed: {e}"))?
}

fn percent_encode(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.~".contains(c) {
                c.to_string()
            } else {
                c.to_string()
                    .bytes()
                    .map(|b| format!("%{b:02X}"))
                    .collect()
            }
        })
        .collect()
}

/// Pull the hit list out of an Open-Meteo geocoding response body.
fn parse_results(v: &serde_json::Value) -> Vec<GeoHit> {
    let mut hits = Vec::new();
    for r in v["results"].as_array().into_iter().flatten() {
        let (Some(name), Some(lat), Some(lon)) = (
            r["name"].as_str(),
            r["latitude"].as_f64(),
            r["longitude"].as_f64(),
        ) else {
            continue;
        };
        let region = r["admin1"].as_str().unwrap_or("");
        let country = r["country_code"].as_str().unwrap_or("");
        let label = match (region.is_empty(), country.is_empty()) {
            (false, false) => format!("{name}, {region}, {country}"),
            (true, false) => format!("{name}, {country}"),
            _ => name.to_string(),
        };
        hits.push(GeoHit { name: label, lat, lon });
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_results_with_label_fallbacks() {
        let v = json!({ "results": [
            { "name": "Oslo", "latitude": 59.91, "longitude": 10.75,
              "admin1": "Oslo", "country_code": "NO" },
            { "name": "Nowhere", "latitude": 1.0, "longitude": 2.0,
              "country_code": "XX" },
            { "name": "Bare", "latitude": 3.0, "longitude": 4.0 },
            { "name": "Broken" }
        ]});
        let hits = parse_results(&v);
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].name, "Oslo, Oslo, NO");
        assert_eq!(hits[1].name, "Nowhere, XX");
        assert_eq!(hits[2].name, "Bare");
    }

    #[test]
    fn encodes_non_ascii() {
        assert_eq!(percent_encode("São Paulo"), "S%C3%A3o%20Paulo");
        assert_eq!(percent_encode("oslo"), "oslo");
    }
}
