use anyhow::{Context, Result};
use chrono::Local;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub assets: PathBuf,
    pub memory: PathBuf,
    pub temp: PathBuf,
}

impl RuntimePaths {
    pub fn discover() -> Result<Self> {
        let exe = std::env::current_exe().ok();
        let mut candidates = Vec::new();
        candidates.push(std::env::current_dir().context("read current directory")?);
        if let Some(exe) = exe.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf) {
            candidates.push(exe.clone());
            if let Some(parent) = exe.parent() {
                candidates.push(parent.to_path_buf());
            }
        }
        if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
            candidates.push(PathBuf::from(manifest));
        }
        let root = candidates
            .into_iter()
            .find(|p| p.join("assets").join("tools_schema.json").is_file())
            .unwrap_or(std::env::current_dir()?);
        let paths = Self {
            assets: root.join("assets"),
            memory: root.join("memory"),
            temp: root.join("temp"),
            root,
        };
        fs::create_dir_all(&paths.temp).context("create temp directory")?;
        fs::create_dir_all(&paths.memory).context("create memory directory")?;
        paths.ensure_memory_files()?;
        Ok(paths)
    }

    fn ensure_memory_files(&self) -> Result<()> {
        let global_mem = self.memory.join("global_mem.txt");
        if !global_mem.exists() {
            fs::write(&global_mem, "# [Global Memory - L2]\n")?;
        }
        let insight = self.memory.join("global_mem_insight.txt");
        if !insight.exists() {
            let suffix = if is_english() { "_en" } else { "" };
            let tpl = self
                .assets
                .join(format!("global_mem_insight_template{suffix}.txt"));
            let content = fs::read_to_string(tpl).unwrap_or_default();
            fs::write(insight, content)?;
        }
        Ok(())
    }

    pub fn task_dir(&self, task: Option<&str>) -> Result<PathBuf> {
        let dir = match task {
            Some(t) if !t.trim().is_empty() => self.temp.join(sanitize_name(t)),
            _ => self.temp.clone(),
        };
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn load_tools_schema(&self) -> Result<serde_json::Value> {
        let suffix = if is_english() { "" } else { "_cn" };
        let preferred = self.assets.join(format!("tools_schema{suffix}.json"));
        let fallback = self.assets.join("tools_schema.json");
        let path = if preferred.exists() {
            preferred
        } else {
            fallback
        };
        let data = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&data).with_context(|| format!("parse {}", path.display()))
    }

    pub fn system_prompt(&self) -> Result<String> {
        let suffix = if is_english() { "_en" } else { "" };
        let prompt_path = self.assets.join(format!("sys_prompt{suffix}.txt"));
        let mut prompt = fs::read_to_string(&prompt_path)
            .with_context(|| format!("read {}", prompt_path.display()))?;
        prompt.push_str(&format!(
            "\nToday: {}\n",
            Local::now().format("%Y-%m-%d %a")
        ));
        prompt.push_str(&self.global_memory());
        Ok(prompt)
    }

    pub fn global_memory(&self) -> String {
        let suffix = if is_english() { "_en" } else { "" };
        let mut out = String::from("\n");
        let structure = fs::read_to_string(
            self.assets
                .join(format!("insight_fixed_structure{suffix}.txt")),
        )
        .unwrap_or_default();
        let insight =
            fs::read_to_string(self.memory.join("global_mem_insight.txt")).unwrap_or_default();
        out.push_str(&format!("cwd = {} (./)\n", self.temp.display()));
        out.push_str("\n[Memory] (../memory)\n");
        out.push_str(&structure);
        out.push_str("\n../memory/global_mem_insight.txt:\n");
        out.push_str(&insight);
        out.push('\n');
        out
    }
}

pub fn is_english() -> bool {
    std::env::var("GA_LANG")
        .map(|v| v.eq_ignore_ascii_case("en"))
        .unwrap_or_else(|_| {
            std::env::var("LANG")
                .map(|v| !v.to_ascii_lowercase().starts_with("zh"))
                .unwrap_or(true)
        })
}

pub fn sanitize_name(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars().take(80) {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('_');
        }
    }
    if out.is_empty() {
        "task".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_name;

    #[test]
    fn sanitizes_task_names() {
        assert_eq!(sanitize_name("hello world/.."), "hello_world..");
        assert_eq!(sanitize_name("涓枃"), "task");
    }
}
