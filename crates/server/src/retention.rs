// story: e35s01 — monthly JSONL rollups at the single write points: one
// append per event stream, month named from the write date (no timer),
// structured fields only so tokens never reach the log (1695cf1 rule).
use chrono::{DateTime, SecondsFormat, Utc};

/// Append one JSON line to `<kind>_YYYY-MM.jsonl` under `JJHUB_LOG_DIR`
/// (default `data/logs`). `fields` are written after `ts`, which always
/// comes first.
pub fn append(
    kind: &str,
    ts: DateTime<Utc>,
    fields: &serde_json::Map<String, serde_json::Value>,
) -> std::io::Result<std::path::PathBuf> {
    let dir = std::path::PathBuf::from(
        std::env::var("JJHUB_LOG_DIR").unwrap_or_else(|_| "data/logs".to_string()),
    );
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{kind}_{}.jsonl", ts.format("%Y-%m")));
    let mut line = String::with_capacity(128);
    line.push_str("{\"ts\":");
    line.push_str(&encode(&ts.to_rfc3339_opts(SecondsFormat::Secs, true))?);
    for (key, value) in fields {
        line.push(',');
        line.push_str(&encode(key)?);
        line.push(':');
        line.push_str(&encode(value)?);
    }
    line.push_str("}\n");
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    Ok(path)
}

/// One line per moderation action, hooked beside each audit INSERT: the
/// action name, the operator who acted, and the affected id (site-wide
/// actions carry no target).
pub fn append_moderation(action: &str, actor: uuid::Uuid, target: Option<uuid::Uuid>) {
    let mut fields = serde_json::Map::new();
    fields.insert("action".into(), action.into());
    fields.insert("actor".into(), actor.to_string().into());
    fields.insert(
        "target".into(),
        target.map_or(serde_json::Value::Null, |id| id.to_string().into()),
    );
    let _ = append("moderation_log", Utc::now(), &fields);
}

fn encode(value: &impl serde::Serialize) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}

/// One delivery-readout row for the console (story: e36s01). Lines written
/// before outcomes existed read back as `unknown`.
#[derive(serde::Serialize, Clone)]
pub struct DeliveryLine {
    pub ts: String,
    pub recipient: String,
    pub subject: String,
    pub outcome: String,
    pub reason: String,
}

/// The most recent `limit` mail_log lines from the current month's file,
/// oldest first. The file is the readout's source — one write point, one
/// fact.
pub fn recent_mail(limit: usize) -> Vec<DeliveryLine> {
    let dir = std::path::PathBuf::from(
        std::env::var("JJHUB_LOG_DIR").unwrap_or_else(|_| "data/logs".to_string()),
    );
    let path = dir.join(format!("mail_log_{}.jsonl", Utc::now().format("%Y-%m")));
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let rows: Vec<DeliveryLine> = contents
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .map(|value| DeliveryLine {
            ts: value["ts"].as_str().unwrap_or_default().to_string(),
            recipient: value["to"].as_str().unwrap_or_default().to_string(),
            subject: value["subject"].as_str().unwrap_or_default().to_string(),
            outcome: value["outcome"].as_str().unwrap_or("unknown").to_string(),
            reason: value["reason"].as_str().unwrap_or_default().to_string(),
        })
        .collect();
    rows.into_iter().rev().take(limit).rev().collect()
}
