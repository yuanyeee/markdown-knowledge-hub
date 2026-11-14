//! Open Knowledge Format v0.2 fields read from YAML frontmatter.
//! Unknown keys are ignored. A document stays indexed when the frontmatter cannot be read.

use crate::SearchHit;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    time::{SystemTime, UNIX_EPOCH},
};

pub const PARSER_VERSION: i64 = 6;

#[derive(Clone, Debug)]
pub struct Record {
    pub title: Option<String>,
    pub kind: Option<String>,
    pub description: Option<String>,
    pub resource: Option<String>,
    pub tags: Vec<String>,
    pub trust: String,
    pub stale_after: Option<String>,
    pub status: Option<String>,
    pub generated_by: Option<String>,
    pub sources: Vec<String>,
    pub error: Option<String>,
    pub okf_version: Option<String>,
    pub concept: bool,
}

impl Default for Record {
    fn default() -> Self {
        Self {
            title: None,
            kind: None,
            description: None,
            resource: None,
            tags: Vec::new(),
            trust: "unverified".to_string(),
            stale_after: None,
            status: None,
            generated_by: None,
            sources: Vec::new(),
            error: None,
            okf_version: None,
            concept: true,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OkfInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub trust: String,
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TypeCount {
    pub kind: String,
    pub count: usize,
}

pub struct Summary {
    pub stale: usize,
    pub missing_type: usize,
    pub frontmatter_invalid: usize,
    pub bundle: bool,
    pub types: Vec<TypeCount>,
}

pub fn reserved_path(path: &str) -> bool {
    matches!(path.rsplit(['/', '\\']).next(), Some("index.md" | "log.md"))
}

pub fn utc_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    format_unix(secs)
}

pub fn apply(record: &mut Record, value: &serde_yaml_ng::Value) {
    if !matches!(value, serde_yaml_ng::Value::Mapping(_)) {
        note(record, "frontmatterはキーと値で指定してください");
        return;
    }
    if let Some(kind) = value.get("type") {
        match kind.as_str().and_then(|text| nonempty(text, 80)) {
            Some(kind) => record.kind = Some(kind),
            None if kind.as_str().is_some() => {}
            None => note(record, "typeは文字列で指定してください"),
        }
    }
    record.title = value
        .get("title")
        .and_then(|item| item.as_str())
        .and_then(|text| nonempty(text, 200));
    record.description = value
        .get("description")
        .and_then(|item| item.as_str())
        .and_then(|text| nonempty(text, 500));
    record.resource = value
        .get("resource")
        .and_then(|item| item.as_str())
        .and_then(|text| nonempty(text, 300));
    record.status = value
        .get("status")
        .and_then(|item| item.as_str())
        .and_then(|text| nonempty(text, 40));
    if let Some(tags) = value.get("tags") {
        match tags {
            serde_yaml_ng::Value::Sequence(items) => {
                for item in items.iter().take(24) {
                    if let Some(tag) = item.as_str().and_then(|text| nonempty(text, 40)) {
                        record.tags.push(tag);
                    }
                }
            }
            serde_yaml_ng::Value::String(tag) => {
                if let Some(tag) = nonempty(tag, 40) {
                    record.tags.push(tag);
                }
            }
            _ => note(record, "tagsは文字列の配列で指定してください"),
        }
    }
    if let Some(generated) = value.get("generated") {
        match generated.get("by").and_then(|item| item.as_str()) {
            Some(actor) => record.generated_by = nonempty(actor, 120),
            None if generated.get("by").is_some() => {
                note(record, "generated.byは文字列で指定してください")
            }
            None => {}
        }
    }
    if let Some(verified) = value.get("verified") {
        let items: Vec<&serde_yaml_ng::Value> = match verified {
            serde_yaml_ng::Value::Sequence(items) => items.iter().collect(),
            serde_yaml_ng::Value::Mapping(_) => vec![verified],
            _ => {
                note(record, "verifiedの形式が不正です");
                Vec::new()
            }
        };
        let mut human = false;
        let mut any = false;
        for item in items {
            if let Some(actor) = item.get("by").and_then(|value| value.as_str()) {
                any = true;
                if actor.starts_with("human:") {
                    human = true;
                }
            }
        }
        record.trust = if human {
            "human-reviewed".to_string()
        } else if any {
            "machine-confirmed".to_string()
        } else {
            "unverified".to_string()
        };
    }
    if let Some(stale) = value.get("stale_after") {
        match stale.as_str().and_then(normalize_instant) {
            Some(instant) => record.stale_after = Some(instant),
            None => note(record, "stale_afterの日時を読み取れません"),
        }
    }
    if let Some(sources) = value.get("sources") {
        match sources {
            serde_yaml_ng::Value::Sequence(items) => {
                for item in items.iter().take(8) {
                    let label = item
                        .get("title")
                        .and_then(|value| value.as_str())
                        .or_else(|| item.get("resource").and_then(|value| value.as_str()))
                        .or_else(|| item.get("id").and_then(|value| value.as_str()));
                    if let Some(label) = label.and_then(|text| nonempty(text, 120)) {
                        record.sources.push(label);
                    }
                }
            }
            _ => note(record, "sourcesは配列で指定してください"),
        }
    }
    match value.get("okf_version") {
        Some(serde_yaml_ng::Value::String(version)) => {
            record.okf_version = nonempty(version, 20);
        }
        Some(serde_yaml_ng::Value::Number(version)) => {
            record.okf_version = nonempty(&version.to_string(), 20);
        }
        Some(_) => note(record, "okf_versionの形式が不正です"),
        None => {}
    }
}

pub fn table_exists(db: &Connection) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='okf')",
        [],
        |row| row.get(0),
    )?)
}

pub fn persist(tx: &rusqlite::Transaction<'_>, path: &str, record: &Record) -> Result<()> {
    let tags = json_list(&record.tags);
    let sources = json_list(&record.sources);
    tx.execute(
        "INSERT OR REPLACE INTO okf(path,concept,kind,description,resource,tags,trust,stale_after,status,generated_by,sources,frontmatter_error) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            path,
            i64::from(record.concept),
            record.kind,
            record.description,
            record.resource,
            tags,
            record.trust,
            record.stale_after,
            record.status,
            record.generated_by,
            sources,
            record.error,
        ],
    )?;
    Ok(())
}

pub fn remove(tx: &rusqlite::Transaction<'_>, path: &str) -> Result<()> {
    tx.execute("DELETE FROM okf WHERE path=?1", [path])?;
    Ok(())
}

pub fn set_bundle(tx: &rusqlite::Transaction<'_>, version: Option<&str>) -> Result<()> {
    if let Some(version) = version.filter(|value| !value.is_empty()) {
        tx.execute(
            "INSERT INTO state_text(key,value) VALUES('okf_version',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [version],
        )?;
    } else {
        tx.execute("DELETE FROM state_text WHERE key='okf_version'", [])?;
    }
    Ok(())
}

pub fn info(db: &Connection, path: &str) -> Result<Option<OkfInfo>> {
    let bundle = if path == "index.md" {
        bundle_version(db)?
    } else {
        None
    };
    if !table_exists(db)? {
        return Ok(bundle.and_then(|version| display(blank(Some(version)))));
    }
    let row = db
        .query_row(
            "SELECT kind,description,resource,tags,trust,stale_after,status,generated_by,sources,frontmatter_error FROM okf WHERE path=?1",
            [path],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            },
        )
        .optional()?;
    let Some((
        kind,
        description,
        resource,
        tags,
        trust,
        stale_after,
        status,
        generated_by,
        sources,
        error,
    )) = row
    else {
        return Ok(bundle.and_then(|version| display(blank(Some(version)))));
    };
    let now = utc_now();
    let stale = stale_after
        .as_ref()
        .is_some_and(|instant| instant.as_str() <= now.as_str());
    Ok(display(OkfInfo {
        kind,
        description,
        resource,
        tags: decode_list(tags),
        trust,
        stale,
        stale_after,
        status,
        generated_by,
        sources: decode_list(sources),
        error,
        bundle,
    }))
}

pub fn summary(db: &Connection) -> Result<Summary> {
    let bundle = bundle_version(db)?.is_some();
    if !table_exists(db)? {
        return Ok(Summary {
            stale: 0,
            missing_type: 0,
            frontmatter_invalid: 0,
            bundle,
            types: Vec::new(),
        });
    }
    let now = utc_now();
    let stale = count(
        db,
        "SELECT COUNT(*) FROM okf WHERE stale_after IS NOT NULL AND stale_after<=?1",
        [now.as_str()],
    )?;
    let missing_type = if bundle {
        count(
            db,
            "SELECT COUNT(*) FROM okf WHERE concept=1 AND (kind IS NULL OR kind='') AND (frontmatter_error IS NULL OR frontmatter_error='')",
            [],
        )?
    } else {
        0
    };
    let frontmatter_invalid = count(
        db,
        "SELECT COUNT(*) FROM okf WHERE frontmatter_error IS NOT NULL AND frontmatter_error<>''",
        [],
    )?;
    let mut statement = db.prepare(
        "SELECT kind, COUNT(*) FROM okf WHERE concept=1 AND kind IS NOT NULL AND kind<>'' GROUP BY kind ORDER BY COUNT(*) DESC, kind LIMIT 40",
    )?;
    let types = statement
        .query_map([], |row| {
            Ok(TypeCount {
                kind: row.get(0)?,
                count: row.get::<_, i64>(1)? as usize,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Summary {
        stale,
        missing_type,
        frontmatter_invalid,
        bundle,
        types,
    })
}

pub fn fill_descriptions(db: &Connection, hits: &mut [SearchHit]) -> Result<()> {
    if hits.is_empty() || !table_exists(db)? {
        return Ok(());
    }
    for chunk in hits.chunks_mut(100) {
        let sql = format!(
            "SELECT path, description FROM okf WHERE path IN ({})",
            vec!["?"; chunk.len()].join(",")
        );
        let mut statement = db.prepare(&sql)?;
        let mut found = HashMap::<String, String>::new();
        for row in statement.query_map(
            rusqlite::params_from_iter(chunk.iter().map(|hit| hit.path.as_str())),
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )? {
            let (path, description) = row?;
            if let Some(description) = description {
                found.insert(path, description);
            }
        }
        for hit in chunk.iter_mut() {
            if hit.snippet.is_none()
                && let Some(description) = found.get(&hit.path)
            {
                hit.snippet = Some(description.clone());
            }
        }
    }
    Ok(())
}

pub fn kind_labels(db: &Connection, paths: &[String]) -> Result<HashMap<String, String>> {
    let mut labels = HashMap::new();
    if paths.is_empty() || !table_exists(db)? {
        return Ok(labels);
    }
    for chunk in paths.chunks(100) {
        let sql = format!(
            "SELECT path, kind FROM okf WHERE path IN ({}) AND kind IS NOT NULL AND kind<>''",
            vec!["?"; chunk.len()].join(",")
        );
        let mut statement = db.prepare(&sql)?;
        for row in statement.query_map(rusqlite::params_from_iter(chunk), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })? {
            let (path, kind) = row?;
            labels.insert(path, kind);
        }
    }
    Ok(labels)
}

pub fn with_type_label(title: &str, kind: &str) -> String {
    if kind.is_empty() || title.contains('·') {
        title.to_string()
    } else {
        format!("{kind} · {title}")
    }
}

pub fn paths_of_kind(db: &Connection, kind: &str) -> Result<HashSet<String>> {
    if !table_exists(db)? {
        return Ok(HashSet::new());
    }
    let mut statement = db.prepare("SELECT path FROM okf WHERE kind=?1")?;
    Ok(statement
        .query_map([kind], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?)
}

fn blank(bundle: Option<String>) -> OkfInfo {
    OkfInfo {
        kind: None,
        description: None,
        resource: None,
        tags: Vec::new(),
        trust: "unverified".to_string(),
        stale: false,
        stale_after: None,
        status: None,
        generated_by: None,
        sources: Vec::new(),
        error: None,
        bundle,
    }
}

fn display(info: OkfInfo) -> Option<OkfInfo> {
    let visible = info.kind.is_some()
        || info.description.is_some()
        || info.resource.is_some()
        || !info.tags.is_empty()
        || info.stale
        || info.stale_after.is_some()
        || info.status.is_some()
        || info.generated_by.is_some()
        || !info.sources.is_empty()
        || info.error.is_some()
        || info.bundle.is_some()
        || info.trust != "unverified";
    visible.then_some(info)
}

fn bundle_version(db: &Connection) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT value FROM state_text WHERE key='okf_version'",
            [],
            |row| row.get(0),
        )
        .optional()?
        .filter(|value: &String| !value.is_empty()))
}

fn count(db: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<usize> {
    Ok(db.query_row(sql, params, |row| row.get::<_, i64>(0))? as usize)
}

fn json_list(values: &[String]) -> Option<String> {
    (!values.is_empty()).then(|| serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string()))
}

fn decode_list(value: Option<String>) -> Vec<String> {
    value
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn note(record: &mut Record, message: &str) {
    if record.error.is_none() {
        record.error = Some(message.to_string());
    }
}

fn nonempty(value: &str, max: usize) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.chars().take(max).collect())
}

pub fn normalize_instant(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.len() < 10 {
        return None;
    }
    let (year, month, day) = parse_ymd(&raw[..10])?;
    let date_only = format_unix(civil_to_unix(year, month, day, 0, 0, 0)?);
    if date_only[..10] != raw[..10] {
        return None;
    }
    if raw.len() == 10 {
        return Some(date_only);
    }
    if raw.as_bytes().get(10) != Some(&b'T') {
        return None;
    }
    let rest = &raw[11..];
    if rest.len() < 8
        || rest.as_bytes().get(2) != Some(&b':')
        || rest.as_bytes().get(5) != Some(&b':')
    {
        return None;
    }
    let hour: u32 = rest[0..2].parse().ok()?;
    let minute: u32 = rest[3..5].parse().ok()?;
    let second: u32 = rest[6..8].parse().ok()?;
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut index = 8;
    if rest.as_bytes().get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while rest
            .as_bytes()
            .get(index)
            .is_some_and(|byte| byte.is_ascii_digit())
        {
            index += 1;
        }
        if index == start {
            return None;
        }
    }
    let offset = match rest.as_bytes().get(index) {
        Some(b'Z') if index + 1 == rest.len() => 0,
        Some(b'+') | Some(b'-') => {
            let sign = if rest.as_bytes()[index] == b'+' {
                1
            } else {
                -1
            };
            let zone = &rest[index + 1..];
            if zone.len() != 5 || zone.as_bytes().get(2) != Some(&b':') {
                return None;
            }
            let zone_hour: i64 = zone[0..2].parse().ok()?;
            let zone_minute: i64 = zone[3..5].parse().ok()?;
            if zone_hour > 23 || zone_minute > 59 {
                return None;
            }
            sign * (zone_hour * 3600 + zone_minute * 60)
        }
        _ => return None,
    };
    Some(format_unix(
        civil_to_unix(year, month, day, hour, minute, second)? - offset,
    ))
}

fn parse_ymd(value: &str) -> Option<(i32, u32, u32)> {
    if value.len() != 10
        || value.as_bytes().get(4) != Some(&b'-')
        || value.as_bytes().get(7) != Some(&b'-')
    {
        return None;
    }
    Some((
        value[0..4].parse().ok()?,
        value[5..7].parse().ok()?,
        value[8..10].parse().ok()?,
    ))
}

fn civil_to_unix(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Option<i64> {
    if !(1..=12).contains(&month) || day == 0 || day > 31 {
        return None;
    }
    let mut year_i = year;
    if month <= 2 {
        year_i -= 1;
    }
    let era = if year_i >= 0 { year_i } else { year_i - 399 } / 400;
    let year_of_era = (year_i - era * 400) as u32;
    let shifted_month = month as i32 + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month as u32 + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era as i64 * 146_097 + i64::from(day_of_era) - 719_468;
    Some(days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second))
}

fn format_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let time = secs.rem_euclid(86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        (time % 3600) / 60,
        time % 60
    )
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = (z - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year as i32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch_and_zone_offsets_normalize() {
        assert_eq!(format_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            normalize_instant("2026-09-23").as_deref(),
            Some("2026-09-23T00:00:00Z")
        );
        assert_eq!(
            normalize_instant("2026-09-23T09:00:00+09:00").as_deref(),
            Some("2026-09-23T00:00:00Z")
        );
        assert_eq!(
            normalize_instant("2026-09-23T00:00:00.5Z").as_deref(),
            Some("2026-09-23T00:00:00Z")
        );
        assert!(normalize_instant("2023-02-29").is_none());
        assert!(normalize_instant("yesterday").is_none());
    }
}
