use std::path::{Path, PathBuf};

pub const HELP_COMMANDS: &[(&str, &str)] = &[
    ("/help", "show help"),
    ("/new", "start a new conversation"),
    ("/continue", "list recoverable sessions"),
    ("/continue N", "restore/select a session"),
    ("/llm", "list or select model/provider"),
    ("/abort", "abort current task"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrontendKind {
    Streamlit,
    Qt,
    Telegram,
    Feishu,
    WeChat,
    WeCom,
    DingTalk,
    Qq,
    DesktopPet,
}

impl FrontendKind {
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "streamlit" | "stapp" | "stapp2" | "web" => Some(Self::Streamlit),
            "qt" | "qtapp" => Some(Self::Qt),
            "telegram" | "tg" | "tgapp" => Some(Self::Telegram),
            "feishu" | "lark" | "fsapp" => Some(Self::Feishu),
            "wechat" | "wx" => Some(Self::WeChat),
            "wecom" => Some(Self::WeCom),
            "dingtalk" => Some(Self::DingTalk),
            "qq" => Some(Self::Qq),
            "pet" | "desktop_pet" => Some(Self::DesktopPet),
            _ => None,
        }
    }
}

pub fn build_help_text() -> String {
    let mut out = String::from("RGA commands:\n");
    for (cmd, desc) in HELP_COMMANDS {
        out.push_str(&format!("  {cmd:<14} {desc}\n"));
    }
    out
}

pub fn clean_reply(text: &str) -> String {
    let mut out = text.to_string();
    for tag in ["thinking", "summary", "tool_use", "file_content"] {
        out = strip_tag_blocks(&out, tag);
    }
    out.trim().to_string()
}

fn strip_tag_blocks(text: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut rest = text;
    let mut out = String::new();
    while let Some(s) = rest.find(&open) {
        out.push_str(&rest[..s]);
        let after = &rest[s + open.len()..];
        if let Some(e) = after.find(&close) {
            rest = &after[e + close.len()..];
        } else {
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}

pub fn extract_files(text: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut rest = text;
    while let Some(s) = rest.find("[FILE:") {
        let after = &rest[s + 6..];
        if let Some(e) = after.find(']') {
            files.push(PathBuf::from(after[..e].trim()));
            rest = &after[e + 1..];
        } else {
            break;
        }
    }
    files
}

pub fn strip_files(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(s) = rest.find("[FILE:") {
        out.push_str(&rest[..s]);
        let after = &rest[s + 6..];
        if let Some(e) = after.find(']') {
            rest = &after[e + 1..];
        } else {
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}

pub fn split_text(text: &str, limit: usize) -> Vec<String> {
    if limit == 0 {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if cur.chars().count() >= limit {
            chunks.push(std::mem::take(&mut cur));
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

pub fn list_skins(root: &Path) -> Vec<String> {
    let skins = root.join("frontends_assets").join("skins");
    std::fs::read_dir(skins)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleans_and_splits_frontend_text() {
        assert_eq!(clean_reply("a<thinking>x</thinking>b"), "ab");
        assert_eq!(extract_files("see [FILE:a.txt]")[0], PathBuf::from("a.txt"));
        assert_eq!(split_text("abcd", 2), vec!["ab", "cd"]);
    }
}
