use anyhow::{Context, Result};
use chrono::{Datelike, Local, NaiveTime, Timelike};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct ScheduledTask {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_repeat")]
    pub repeat: String,
    #[serde(default = "default_schedule")]
    pub schedule: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default = "default_max_delay")]
    pub max_delay_hours: u64,
}

fn default_repeat() -> String {
    "daily".to_string()
}
fn default_schedule() -> String {
    "00:00".to_string()
}
fn default_max_delay() -> u64 {
    6
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriggeredTask {
    pub id: String,
    pub report_path: PathBuf,
    pub prompt: String,
}

pub fn check_scheduled_tasks(root: &Path) -> Result<Option<TriggeredTask>> {
    let tasks_dir = root.join("sche_tasks");
    let done_dir = tasks_dir.join("done");
    if !tasks_dir.is_dir() {
        return Ok(None);
    }
    fs::create_dir_all(&done_dir)?;
    let done_files: Vec<String> = fs::read_dir(&done_dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    let now = Local::now();
    let mut files: Vec<_> = fs::read_dir(&tasks_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    files.sort();
    for path in files {
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("task")
            .to_string();
        let data = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let task: ScheduledTask =
            serde_json::from_str(&data).with_context(|| format!("parse {}", path.display()))?;
        if !task.enabled {
            continue;
        }
        if task.repeat == "weekday" && now.weekday().number_from_monday() >= 6 {
            continue;
        }
        let sched = match NaiveTime::parse_from_str(&task.schedule, "%H:%M") {
            Ok(t) => t,
            Err(_) => continue,
        };
        let now_minutes = now.hour() * 60 + now.minute();
        let sched_minutes = sched.hour() * 60 + sched.minute();
        if now_minutes < sched_minutes {
            continue;
        }
        if now_minutes - sched_minutes > (task.max_delay_hours as u32) * 60 {
            continue;
        }
        if let Some(last) = last_run(&id, &done_files)
            && now
                .signed_duration_since(last)
                .to_std()
                .unwrap_or(Duration::ZERO)
                < cooldown(&task.repeat)
        {
            continue;
        }
        let report_path = done_dir.join(format!("{}_{id}.md", now.format("%Y-%m-%d_%H%M")));
        let prompt = format!(
            "[定时任务] {id}\n[报告路径] {}\n\n先读 scheduled_task_sop 了解执行流程，然后执行以下任务：\n\n{}\n\n完成后将执行报告写入 {}。",
            report_path.display(),
            task.prompt,
            report_path.display()
        );
        return Ok(Some(TriggeredTask {
            id,
            report_path,
            prompt,
        }));
    }
    Ok(None)
}

fn cooldown(repeat: &str) -> Duration {
    match repeat {
        "once" => Duration::from_secs(999_999 * 24 * 3600),
        "daily" | "weekday" => Duration::from_secs(20 * 3600),
        "weekly" => Duration::from_secs(6 * 24 * 3600),
        "monthly" => Duration::from_secs(27 * 24 * 3600),
        r if r.starts_with("every_") => parse_every(r).unwrap_or(Duration::from_secs(20 * 3600)),
        _ => Duration::from_secs(20 * 3600),
    }
}

fn parse_every(repeat: &str) -> Option<Duration> {
    let nunit = repeat.strip_prefix("every_")?;
    let unit = nunit.chars().last()?;
    let n: u64 = nunit[..nunit.len().saturating_sub(1)].parse().ok()?;
    match unit {
        'h' => Some(Duration::from_secs(n * 3600)),
        'm' => Some(Duration::from_secs(n * 60)),
        'd' => Some(Duration::from_secs(n * 24 * 3600)),
        _ => None,
    }
}

fn last_run(id: &str, done_files: &[String]) -> Option<chrono::DateTime<Local>> {
    let suffix = format!("_{id}.md");
    done_files
        .iter()
        .filter(|f| f.ends_with(&suffix) && f.len() >= 15)
        .filter_map(|f| chrono::NaiveDateTime::parse_from_str(&f[..15], "%Y-%m-%d_%H%M").ok())
        .filter_map(|n| n.and_local_timezone(Local).single())
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_repeat() {
        assert_eq!(parse_every("every_2h"), Some(Duration::from_secs(7200)));
        assert_eq!(parse_every("every_30m"), Some(Duration::from_secs(1800)));
    }
}
