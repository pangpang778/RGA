use std::env;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    Mock,
    OpenAi,
    Anthropic,
}

#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub provider: ProviderKind,
    pub api_key: Option<String>,
    pub api_base: String,
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub reasoning_effort: Option<String>,
}

impl LlmConfig {
    pub fn from_env(cli_provider: Option<&str>) -> Self {
        let provider_name = cli_provider
            .map(ToOwned::to_owned)
            .or_else(|| env::var("RGA_PROVIDER").ok())
            .unwrap_or_else(infer_provider);
        match provider_name.to_ascii_lowercase().as_str() {
            "anthropic" | "claude" => Self::anthropic(),
            "openai" | "oai" | "openai-compatible" => Self::openai(),
            "mock" | "dry" | "none" => Self::mock(),
            _ => Self::mock(),
        }
    }

    pub fn openai() -> Self {
        Self {
            provider: ProviderKind::OpenAi,
            api_key: env::var("RGA_OPENAI_API_KEY")
                .ok()
                .or_else(|| env::var("OPENAI_API_KEY").ok()),
            api_base: env::var("RGA_OPENAI_BASE_URL")
                .or_else(|_| env::var("OPENAI_BASE_URL"))
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
            model: env::var("RGA_OPENAI_MODEL")
                .or_else(|_| env::var("OPENAI_MODEL"))
                .unwrap_or_else(|_| "gpt-5.4".to_string()),
            temperature: parse_env("RGA_TEMPERATURE"),
            max_tokens: parse_env("RGA_MAX_TOKENS"),
            reasoning_effort: env::var("RGA_REASONING_EFFORT").ok(),
        }
    }

    pub fn anthropic() -> Self {
        Self {
            provider: ProviderKind::Anthropic,
            api_key: env::var("RGA_ANTHROPIC_API_KEY")
                .ok()
                .or_else(|| env::var("ANTHROPIC_API_KEY").ok()),
            api_base: env::var("RGA_ANTHROPIC_BASE_URL")
                .or_else(|_| env::var("ANTHROPIC_BASE_URL"))
                .unwrap_or_else(|_| "https://api.anthropic.com".to_string()),
            model: env::var("RGA_ANTHROPIC_MODEL")
                .or_else(|_| env::var("ANTHROPIC_MODEL"))
                .unwrap_or_else(|_| "claude-sonnet-4-6".to_string()),
            temperature: parse_env("RGA_TEMPERATURE"),
            max_tokens: parse_env("RGA_MAX_TOKENS").or(Some(8192)),
            reasoning_effort: None,
        }
    }

    pub fn mock() -> Self {
        Self {
            provider: ProviderKind::Mock,
            api_key: None,
            api_base: String::new(),
            model: "mock".to_string(),
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
        }
    }

    pub fn openai_with(api_key: Option<String>, api_base: String, model: String) -> Self {
        Self {
            provider: ProviderKind::OpenAi,
            api_key,
            api_base,
            model,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
        }
    }

    pub fn anthropic_with(api_key: Option<String>, api_base: String, model: String) -> Self {
        Self {
            provider: ProviderKind::Anthropic,
            api_key,
            api_base,
            model,
            temperature: None,
            max_tokens: Some(8192),
            reasoning_effort: None,
        }
    }
}

fn infer_provider() -> String {
    if env::var("RGA_ANTHROPIC_API_KEY").is_ok() || env::var("ANTHROPIC_API_KEY").is_ok() {
        "anthropic".to_string()
    } else if env::var("RGA_OPENAI_API_KEY").is_ok() || env::var("OPENAI_API_KEY").is_ok() {
        "openai".to_string()
    } else {
        "mock".to_string()
    }
}

fn parse_env<T: std::str::FromStr>(name: &str) -> Option<T> {
    env::var(name).ok().and_then(|v| v.parse().ok())
}

pub fn api_url(base: &str, path: &str) -> String {
    let b = base.trim_end_matches('/');
    let p = path.trim_matches('/');
    if b.ends_with('$') {
        return b.trim_end_matches('$').trim_end_matches('/').to_string();
    }
    if b.ends_with(p) {
        b.to_string()
    } else if b.contains("/v1") || b.contains("/v2") {
        format!("{b}/{p}")
    } else {
        format!("{b}/v1/{p}")
    }
}

#[cfg(test)]
mod tests {
    use super::api_url;

    #[test]
    fn builds_urls_like_python_helper() {
        assert_eq!(
            api_url("https://api.openai.com/v1", "chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(api_url("https://host/api$", "messages"), "https://host/api");
        assert_eq!(
            api_url("https://host", "messages"),
            "https://host/v1/messages"
        );
    }
}
