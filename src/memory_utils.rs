use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretStr(String);
impl SecretStr {
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn masked(&self) -> String {
        if self.0.len() <= 8 {
            "****".to_string()
        } else {
            format!("{}****{}", &self.0[..4], &self.0[self.0.len() - 4..])
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Keychain {
    values: HashMap<String, String>,
}

impl Keychain {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.is_file() {
            return Ok(Self::default());
        }
        let bytes = fs::read(path)?;
        let data = xor(&bytes);
        Ok(serde_json::from_slice(&data)?)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, xor(&serde_json::to_vec(self)?))?;
        Ok(())
    }
    pub fn set(&mut self, key: &str, value: &str) {
        self.values.insert(key.to_string(), value.to_string());
    }
    pub fn get(&self, key: &str) -> Option<SecretStr> {
        self.values.get(key).cloned().map(SecretStr)
    }
}

fn xor(data: &[u8]) -> Vec<u8> {
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "rga".into());
    let mask = user.as_bytes();
    data.iter()
        .enumerate()
        .map(|(i, b)| b ^ mask[i % mask.len()])
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvironmentInfo {
    pub os: String,
    pub shell: String,
    pub rust: Option<String>,
    pub python: Option<String>,
    pub tools: Vec<String>,
}

pub fn detect_environment() -> EnvironmentInfo {
    EnvironmentInfo {
        os: std::env::consts::OS.to_string(),
        shell: std::env::var("SHELL")
            .or_else(|_| std::env::var("ComSpec"))
            .unwrap_or_default(),
        rust: run_version("rustc", &["--version"]),
        python: run_version("python", &["--version"]),
        tools: ["git", "cargo", "python", "node", "uv"]
            .into_iter()
            .filter(|t| run_version(t, &["--version"]).is_some())
            .map(str::to_string)
            .collect(),
    }
}

fn run_version(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    let s = if out.stdout.is_empty() {
        out.stderr
    } else {
        out.stdout
    };
    Some(String::from_utf8_lossy(&s).trim().to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub path: PathBuf,
    pub score: usize,
}

pub fn skill_search(root: &Path, query: &str, top_k: usize) -> Vec<SearchResult> {
    let q = query.to_ascii_lowercase();
    let mut out = Vec::new();
    visit_files(root, &mut |p| {
        if p.extension()
            .and_then(|s| s.to_str())
            .is_some_and(|e| matches!(e, "md" | "txt" | "json" | "rs"))
            && let Ok(text) = fs::read_to_string(p)
        {
            let low = text.to_ascii_lowercase();
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let score = low.matches(&q).count() + name.matches(&q).count() * 5;
            if score > 0 {
                out.push(SearchResult {
                    title: p
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string(),
                    path: p.to_path_buf(),
                    score,
                });
            }
        }
    });
    out.sort_by_key(|r| std::cmp::Reverse(r.score));
    out.truncate(top_k);
    out
}

fn visit_files(root: &Path, f: &mut impl FnMut(&Path)) {
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| matches!(n, "target" | ".git" | "temp"))
            {
                continue;
            }
            if p.is_dir() {
                visit_files(&p, f);
            } else {
                f(&p);
            }
        }
    }
}

pub fn ocr_image(_image_path: &Path) -> Result<String> {
    Err(anyhow!(
        "native OCR backend is not bundled; configure an external OCR tool"
    ))
}
pub fn detect_ui_elements(_image_path: &Path) -> Result<Value> {
    Ok(json!({"detections":[],"note":"native YOLO model not bundled"}))
}
pub fn scan_process_memory(_pid: u32, _pattern: &str) -> Result<Value> {
    Err(anyhow!(
        "process memory scanning is intentionally not enabled in safe RGA builds"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keychain_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("k.enc");
        let mut k = Keychain::default();
        k.set("a", "secret");
        k.save(&p).unwrap();
        assert_eq!(
            Keychain::load(&p).unwrap().get("a").unwrap().expose(),
            "secret"
        );
    }
}
