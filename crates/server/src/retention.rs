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
