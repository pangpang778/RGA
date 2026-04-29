use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn find_free_port(lo: u16, hi: u16) -> Option<u16> {
    (lo..=hi).find(|port| TcpListener::bind(("127.0.0.1", *port)).is_ok())
}

pub fn inject(root: &Path, text: &str) -> std::io::Result<()> {
    let dir = root.join("temp").join("launcher_inject");
    std::fs::create_dir_all(&dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::fs::write(dir.join(format!("input_{stamp}.txt")), text)
}

pub fn start_web_runtime(exe: &Path, root: &Path, port: u16) -> std::io::Result<Child> {
    Command::new(exe)
        .args(["--frontend", "streamlit", "--port", &port.to_string()])
        .current_dir(root)
        .stdin(Stdio::null())
        .spawn()
}

pub fn last_reply_time(root: &Path) -> Option<std::time::SystemTime> {
    let temp = root.join("temp");
    std::fs::read_dir(temp)
        .ok()?
        .flatten()
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_port() {
        assert!(find_free_port(18501, 18599).is_some());
    }
}
