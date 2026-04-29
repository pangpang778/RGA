use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

#[derive(Clone, Debug)]
pub struct ServiceInfo {
    pub name: String,
    pub port: u16,
    pub url: String,
    pub alive: bool,
}

pub fn acquire_singleton(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

pub fn discover_services() -> Vec<ServiceInfo> {
    let candidates = [
        ("hub", 19735u16),
        ("streamlit", 18501),
        ("wecom", 19531),
        ("browser-ws", 18765),
        ("browser-http", 18766),
    ];
    candidates
        .into_iter()
        .map(|(name, port)| ServiceInfo {
            name: name.to_string(),
            port,
            url: format!("http://127.0.0.1:{port}"),
            alive: std::net::TcpStream::connect(("127.0.0.1", port)).is_ok(),
        })
        .collect()
}

#[derive(Default)]
pub struct ServiceManager {
    children: Vec<(String, Child)>,
}

impl ServiceManager {
    pub fn start_rga_service(
        &mut self,
        name: &str,
        exe: &Path,
        args: &[&str],
        cwd: &Path,
    ) -> std::io::Result<()> {
        let child = Command::new(exe)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        self.children.push((name.to_string(), child));
        Ok(())
    }

    pub fn stop_all(&mut self) {
        for (_, child) in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.children.clear();
    }

    pub fn running_names(&self) -> Vec<String> {
        self.children.iter().map(|(n, _)| n.clone()).collect()
    }
}

impl Drop for ServiceManager {
    fn drop(&mut self) {
        self.stop_all();
    }
}

pub fn service_command_for_frontend(frontend: &str) -> Option<Vec<String>> {
    match frontend.to_ascii_lowercase().as_str() {
        "streamlit" | "web" | "stapp" | "stapp2" => {
            Some(vec!["--frontend".into(), "streamlit".into()])
        }
        "telegram" | "tg" => Some(vec!["--frontend".into(), "telegram".into()]),
        "feishu" | "lark" => Some(vec!["--frontend".into(), "feishu".into()]),
        "wechat" | "wecom" | "dingtalk" | "qq" | "qt" | "pet" => {
            Some(vec!["--frontend".into(), frontend.into()])
        }
        _ => None,
    }
}

pub fn hub_state_path(root: &Path) -> PathBuf {
    root.join("temp").join("hub_state.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovers_known_services() {
        assert!(discover_services().iter().any(|s| s.name == "hub"));
        assert!(service_command_for_frontend("telegram").is_some());
    }
}
