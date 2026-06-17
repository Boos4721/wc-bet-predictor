use crate::predictor::ApiConfig;
use std::path::Path;

pub fn load(path: &str) -> std::io::Result<Option<ApiConfig>> {
    if !Path::new(path).exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(path)?;
    if s.trim().is_empty() {
        return Ok(None);
    }
    let cfg = serde_json::from_str(&s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(cfg))
}

pub fn save(path: &str, cfg: &ApiConfig) -> std::io::Result<()> {
    let s = serde_json::to_string_pretty(cfg)?;
    std::fs::write(path, s)
}

pub fn default_path() -> String {
    "config.local.json".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::predictor::{ApiConfig, ApiProtocol};

    #[test]
    fn save_then_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("wcbp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.local.json");
        let p = path.to_str().unwrap();

        assert!(load(p).unwrap().is_none());

        let cfg = ApiConfig { base_url: "https://api.anthropic.com".into(),
            api_key: "sk-x".into(), model: "claude-fable-5".into(),
            protocol: ApiProtocol::Anthropic };
        save(p, &cfg).unwrap();

        let got = load(p).unwrap().unwrap();
        assert_eq!(got.model, "claude-fable-5");
        assert!(matches!(got.protocol, ApiProtocol::Anthropic));
    }
}
