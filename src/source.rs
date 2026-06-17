use crate::domain::{Match, Odds};

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("解析失败:{0}")]
    Parse(String),
}

pub trait MatchSource {
    fn load(&self) -> Result<Vec<Match>, SourceError>;
}

pub struct PasteSource { pub raw: String }

impl MatchSource for PasteSource {
    fn load(&self) -> Result<Vec<Match>, SourceError> {
        let trimmed = self.raw.trim_start();
        if trimmed.starts_with('[') {
            serde_json::from_str(trimmed).map_err(|e| SourceError::Parse(e.to_string()))
        } else {
            parse_pipe(&self.raw)
        }
    }
}

fn parse_pipe(raw: &str) -> Result<Vec<Match>, SourceError> {
    let mut out = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() { continue; }
        let f: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if f.len() < 8 {
            return Err(SourceError::Parse(format!("第 {} 行字段不足(需 >=8)", i + 1)));
        }
        let num = |s: &str, name: &str| s.parse::<f64>()
            .map_err(|_| SourceError::Parse(format!("第 {} 行 {} 非数字", i + 1, name)));
        out.push(Match {
            id: f[0].into(), league: f[1].into(),
            home: f[2].into(), away: f[3].into(),
            kickoff: f[4].into(),
            odds: Odds { home: num(f[5], "主赔")?, draw: num(f[6], "平赔")?, away: num(f[7], "客赔")? },
            handicap: f.get(8).and_then(|s| s.parse::<i32>().ok()),
            hhad_odds: None,
            hhad_line: None,
            pm_score: None,
            pm_halftime: None,
        });
    }
    if out.is_empty() {
        return Err(SourceError::Parse("无有效赛事".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_array() {
        let raw = r#"[{"id":"周日001","league":"世界杯","home":"A","away":"B",
            "kickoff":"2026-06-20T19:00:00","odds":{"home":2.1,"draw":3.2,"away":3.5},"handicap":null}]"#;
        let ms = PasteSource { raw: raw.into() }.load().unwrap();
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].home, "A");
    }

    #[test]
    fn parses_pipe_text() {
        let raw = "周日001|世界杯|A|B|2026-06-20T19:00:00|2.1|3.2|3.5";
        let ms = PasteSource { raw: raw.into() }.load().unwrap();
        assert_eq!(ms[0].odds.draw, 3.2);
        assert_eq!(ms[0].handicap, None);
    }

    #[test]
    fn pipe_text_with_handicap() {
        let raw = "周日002|世界杯|C|D|2026-06-20T22:00:00|1.8|3.4|4.0|-1";
        let ms = PasteSource { raw: raw.into() }.load().unwrap();
        assert_eq!(ms[0].handicap, Some(-1));
    }

    #[test]
    fn bad_line_reports_index() {
        let raw = "周日001|world|A"; // 字段不足
        let err = PasteSource { raw: raw.into() }.load().unwrap_err();
        assert!(format!("{err}").contains("第 1 行"));
    }
}
