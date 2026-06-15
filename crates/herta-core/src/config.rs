//! Конфигурация ассистента. Порт `config.py`: типобезопасные структуры,
//! заполняемые из переменных окружения с разумными значениями по умолчанию.
//!
//! Каждая секция - отдельная структура с `Default`. `AppConfig::from_env`
//! читает `.env` (через `dotenvy`) и переменные процесса.

use serde::{Deserialize, Serialize};

// --- помощники чтения окружения ---

fn env_str(key: &str, default: &str) -> String {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn env_bool(key: &str, default: bool) -> bool {
    match env_opt(key) {
        None => default,
        Some(v) => matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"),
    }
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    env_opt(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn env_opt_parse<T: std::str::FromStr>(key: &str) -> Option<T> {
    env_opt(key).and_then(|v| v.parse().ok())
}

fn env_csv(key: &str, default: &[&str]) -> Vec<String> {
    match env_opt(key) {
        Some(v) => v.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect(),
        None => default.iter().map(|s| s.to_string()).collect(),
    }
}

/// Активный провайдер LLM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    Ollama,
    Cerebras,
    DeepSeek,
    GoogleAi,
}

impl LlmProvider {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "cerebras" => Self::Cerebras,
            "deepseek" => Self::DeepSeek,
            "google_ai" | "google" | "gemini" => Self::GoogleAi,
            _ => Self::Ollama,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::Cerebras => "cerebras",
            Self::DeepSeek => "deepseek",
            Self::GoogleAi => "google_ai",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    pub host: String,
    pub model: String,
    pub timeout_seconds: f64,
    pub keep_alive: String,
    pub think: bool,
    pub temperature: f32,
    pub num_ctx: u32,
    pub num_gpu: Option<u32>,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            host: "http://127.0.0.1:11434".into(),
            model: "qwen3:4b".into(),
            timeout_seconds: 300.0,
            keep_alive: "10m".into(),
            think: false,
            temperature: 0.55,
            num_ctx: 2048,
            num_gpu: None,
        }
    }
}

/// Общий конфиг OpenAI-совместимого провайдера (Cerebras, DeepSeek).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiCompatConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub timeout_seconds: f64,
    pub temperature: f32,
    pub max_tokens: u32,
    pub retry_attempts: u32,
    pub rate_limit_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleAiConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub fallback_model: Option<String>,
    pub timeout_seconds: f64,
    pub temperature: f32,
    pub max_tokens: u32,
    pub live_model: String,
    pub live_voice_name: Option<String>,
}

impl Default for GoogleAiConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            model: "gemma-3-27b-it".into(),
            fallback_model: None,
            timeout_seconds: 45.0,
            temperature: 0.55,
            max_tokens: 700,
            live_model: "gemini-3.1-flash-live-preview".into(),
            live_voice_name: Some("Kore".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub enabled: bool,
    pub path: String,
    pub max_messages: usize,
    pub context_messages: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self { enabled: true, path: "data/dialogue_memory.json".into(), max_messages: 80, context_messages: 12 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LongMemoryConfig {
    pub enabled: bool,
    pub path: String,
    pub max_facts: usize,
    pub auto_extract_enabled: bool,
    pub auto_extract_every_turns: u32,
}

impl Default for LongMemoryConfig {
    fn default() -> Self {
        Self { enabled: true, path: "data/long_memory.json".into(), max_facts: 200, auto_extract_enabled: true, auto_extract_every_turns: 6 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchConfig {
    pub enabled: bool,
    pub provider: String,
    pub api_key: Option<String>,
    pub max_results: usize,
    pub timeout_seconds: f64,
    pub search_depth: String,
    pub followup_in_character: bool,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self { enabled: false, provider: "tavily".into(), api_key: None, max_results: 5, timeout_seconds: 15.0, search_depth: "basic".into(), followup_in_character: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeToolsConfig {
    pub enabled: bool,
    pub project_root: String,
    pub timeout_seconds: u64,
    pub self_check_enabled: bool,
}

impl Default for CodeToolsConfig {
    fn default() -> Self {
        Self { enabled: false, project_root: ".".into(), timeout_seconds: 30, self_check_enabled: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemActionsConfig {
    pub enabled: bool,
    pub document_dir: String,
    pub registry_path: String,
    pub browser_home_url: String,
    pub vscode_command: String,
    pub vscode_open_workspace: bool,
}

impl Default for SystemActionsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            document_dir: "desktop".into(),
            registry_path: "data/system_actions_registry.json".into(),
            browser_home_url: "https://www.google.com".into(),
            vscode_command: "code".into(),
            vscode_open_workspace: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeWordConfig {
    pub enabled: bool,
    pub mode: String,
    pub phrases: Vec<String>,
    pub follow_up_seconds: f64,
}

impl Default for WakeWordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: "text".into(),
            phrases: ["герта", "великая герта", "эй герта", "слушай герта", "herta"].iter().map(|s| s.to_string()).collect(),
            follow_up_seconds: 10.0,
        }
    }
}

/// Лимиты контекстного окна и порог автосжатия.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Полный бюджет токенов модели.
    pub max_tokens: usize,
    /// Доля бюджета, при достижении которой запускается автосжатие (0.0..1.0).
    pub compaction_threshold: f32,
    /// Сколько недавних реплик сохранять дословно при сжатии.
    pub keep_recent_messages: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self { max_tokens: 8192, compaction_threshold: 0.8, keep_recent_messages: 6 }
    }
}

/// Конфиг оркестрации саб-агентов.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub enabled: bool,
    /// Максимум одновременно работающих саб-агентов.
    pub max_concurrent: usize,
    /// Таймаут на одного саб-агента.
    pub timeout_seconds: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self { enabled: true, max_concurrent: 4, timeout_seconds: 180 }
    }
}

/// Корневая конфигурация приложения.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub log_level: String,
    pub llm_provider: LlmProvider,
    pub stt_provider: String,
    pub max_history_messages: usize,
    pub persona_rewrite_enabled: bool,
    pub ollama: OllamaConfig,
    pub cerebras: OpenAiCompatConfig,
    pub deepseek: OpenAiCompatConfig,
    pub google_ai: GoogleAiConfig,
    pub memory: MemoryConfig,
    pub long_memory: LongMemoryConfig,
    pub web_search: WebSearchConfig,
    pub code_tools: CodeToolsConfig,
    pub system_actions: SystemActionsConfig,
    pub wakeword: WakeWordConfig,
    pub context: ContextConfig,
    pub agent: AgentConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            log_level: "INFO".into(),
            llm_provider: LlmProvider::Ollama,
            stt_provider: "whisper".into(),
            max_history_messages: 8,
            persona_rewrite_enabled: false,
            ollama: OllamaConfig::default(),
            cerebras: OpenAiCompatConfig {
                api_key: None,
                base_url: "https://api.cerebras.ai/v1".into(),
                model: "gpt-oss-120b".into(),
                timeout_seconds: 60.0,
                temperature: 0.55,
                max_tokens: 700,
                retry_attempts: 4,
                rate_limit_retries: 2,
            },
            deepseek: OpenAiCompatConfig {
                api_key: None,
                base_url: "https://api.deepseek.com".into(),
                model: "deepseek-v4-flash".into(),
                timeout_seconds: 120.0,
                temperature: 0.55,
                max_tokens: 700,
                retry_attempts: 4,
                rate_limit_retries: 2,
            },
            google_ai: GoogleAiConfig::default(),
            memory: MemoryConfig::default(),
            long_memory: LongMemoryConfig::default(),
            web_search: WebSearchConfig::default(),
            code_tools: CodeToolsConfig::default(),
            system_actions: SystemActionsConfig::default(),
            wakeword: WakeWordConfig::default(),
            context: ContextConfig::default(),
            agent: AgentConfig::default(),
        }
    }
}

impl AppConfig {
    /// Загрузка из `.env` + переменных окружения. Отсутствующий `.env` - не ошибка.
    #[allow(clippy::field_reassign_with_default)] // секции читаются последовательно для читаемости
    pub fn from_env() -> Self {
        let _ = dotenvy::dotenv();
        let mut cfg = AppConfig::default();

        cfg.log_level = env_str("LOG_LEVEL", "INFO").to_uppercase();
        cfg.llm_provider = LlmProvider::parse(&env_str("LLM_PROVIDER", "ollama"));
        cfg.stt_provider = env_str("STT_PROVIDER", "whisper").to_lowercase();
        cfg.max_history_messages = env_parse("MAX_HISTORY_MESSAGES", 8);
        cfg.persona_rewrite_enabled = env_bool("PERSONA_REWRITE_ENABLED", false);

        cfg.ollama = OllamaConfig {
            host: env_str("OLLAMA_HOST", "http://127.0.0.1:11434"),
            model: env_str("OLLAMA_MODEL", "qwen3:4b"),
            timeout_seconds: env_parse("OLLAMA_TIMEOUT_SECONDS", 300.0),
            keep_alive: env_str("OLLAMA_KEEP_ALIVE", "10m"),
            think: env_bool("OLLAMA_THINK", false),
            temperature: env_parse("OLLAMA_TEMPERATURE", 0.55),
            num_ctx: env_parse("OLLAMA_NUM_CTX", 2048),
            num_gpu: env_opt_parse("OLLAMA_NUM_GPU"),
        };

        cfg.cerebras = OpenAiCompatConfig {
            api_key: env_opt("CEREBRAS_API_KEY"),
            base_url: env_str("CEREBRAS_BASE_URL", "https://api.cerebras.ai/v1"),
            model: env_str("CEREBRAS_MODEL", "gpt-oss-120b"),
            timeout_seconds: env_parse("CEREBRAS_TIMEOUT_SECONDS", 60.0),
            temperature: env_parse("CEREBRAS_TEMPERATURE", 0.55),
            max_tokens: env_parse("CEREBRAS_MAX_TOKENS", 700),
            retry_attempts: env_parse("CEREBRAS_RETRY_ATTEMPTS", 4),
            rate_limit_retries: env_parse("CEREBRAS_RATE_LIMIT_RETRIES", 2),
        };

        cfg.deepseek = OpenAiCompatConfig {
            api_key: env_opt("DEEPSEEK_API_KEY"),
            base_url: env_str("DEEPSEEK_BASE_URL", "https://api.deepseek.com"),
            model: env_str("DEEPSEEK_MODEL", "deepseek-v4-flash"),
            timeout_seconds: env_parse("DEEPSEEK_TIMEOUT_SECONDS", 120.0),
            temperature: env_parse("DEEPSEEK_TEMPERATURE", 0.55),
            max_tokens: env_parse("DEEPSEEK_MAX_TOKENS", 700),
            retry_attempts: env_parse("DEEPSEEK_RETRY_ATTEMPTS", 4),
            rate_limit_retries: env_parse("DEEPSEEK_RATE_LIMIT_RETRIES", 2),
        };

        cfg.google_ai = GoogleAiConfig {
            api_key: env_opt("GOOGLE_AI_API_KEY").or_else(|| env_opt("GEMINI_API_KEY")),
            base_url: env_str("GOOGLE_AI_BASE_URL", "https://generativelanguage.googleapis.com/v1beta"),
            model: env_str("GOOGLE_AI_MODEL", "gemma-3-27b-it"),
            fallback_model: env_opt("GOOGLE_AI_FALLBACK_MODEL"),
            timeout_seconds: env_parse("GOOGLE_AI_TIMEOUT_SECONDS", 45.0),
            temperature: env_parse("GOOGLE_AI_TEMPERATURE", 0.55),
            max_tokens: env_parse("GOOGLE_AI_MAX_TOKENS", 700),
            live_model: env_str("GOOGLE_AI_LIVE_MODEL", "gemini-3.1-flash-live-preview"),
            live_voice_name: env_opt("GOOGLE_AI_LIVE_VOICE").or_else(|| Some("Kore".into())),
        };

        cfg.memory = MemoryConfig {
            enabled: env_bool("MEMORY_ENABLED", true),
            path: env_str("MEMORY_PATH", "data/dialogue_memory.json"),
            max_messages: env_parse("MEMORY_MAX_MESSAGES", 80),
            context_messages: env_parse("MEMORY_CONTEXT_MESSAGES", 12),
        };

        cfg.long_memory = LongMemoryConfig {
            enabled: env_bool("LONG_MEMORY_ENABLED", true),
            path: env_str("LONG_MEMORY_PATH", "data/long_memory.json"),
            max_facts: env_parse("LONG_MEMORY_MAX_FACTS", 200),
            auto_extract_enabled: env_bool("LONG_MEMORY_AUTO_EXTRACT", true),
            auto_extract_every_turns: env_parse("LONG_MEMORY_AUTO_EXTRACT_EVERY_TURNS", 6),
        };

        cfg.web_search = WebSearchConfig {
            enabled: env_bool("WEB_SEARCH_ENABLED", false),
            provider: env_str("WEB_SEARCH_PROVIDER", "tavily").to_lowercase(),
            api_key: env_opt("TAVILY_API_KEY").or_else(|| env_opt("WEB_SEARCH_API_KEY")),
            max_results: env_parse("WEB_SEARCH_MAX_RESULTS", 5),
            timeout_seconds: env_parse("WEB_SEARCH_TIMEOUT_SECONDS", 15.0),
            search_depth: env_str("WEB_SEARCH_DEPTH", "basic").to_lowercase(),
            followup_in_character: env_bool("WEB_SEARCH_FOLLOWUP_IN_CHARACTER", true),
        };

        cfg.code_tools = CodeToolsConfig {
            enabled: env_bool("CODE_TOOLS_ENABLED", false),
            project_root: env_str("CODE_TOOLS_PROJECT_ROOT", "."),
            timeout_seconds: env_parse("CODE_TOOLS_TIMEOUT_SECONDS", 30),
            self_check_enabled: env_bool("CODE_TOOLS_SELF_CHECK", false),
        };

        cfg.system_actions = SystemActionsConfig {
            enabled: env_bool("SYSTEM_ACTIONS_ENABLED", false),
            document_dir: env_str("SYSTEM_ACTIONS_DOCUMENT_DIR", "desktop"),
            registry_path: env_str("SYSTEM_ACTIONS_REGISTRY_PATH", "data/system_actions_registry.json"),
            browser_home_url: env_str("SYSTEM_ACTIONS_BROWSER_HOME_URL", "https://www.google.com"),
            vscode_command: env_str("SYSTEM_ACTIONS_VSCODE_COMMAND", "code"),
            vscode_open_workspace: env_bool("SYSTEM_ACTIONS_VSCODE_OPEN_WORKSPACE", true),
        };

        cfg.wakeword = WakeWordConfig {
            enabled: env_bool("WAKEWORD_ENABLED", false),
            mode: env_str("WAKEWORD_MODE", "text").to_lowercase(),
            phrases: env_csv("WAKEWORD_PHRASES", &["герта", "великая герта", "эй герта", "слушай герта", "herta"]),
            follow_up_seconds: env_parse("WAKEWORD_FOLLOW_UP_SECONDS", 10.0),
        };

        cfg.context = ContextConfig {
            max_tokens: env_parse("CONTEXT_MAX_TOKENS", 8192),
            compaction_threshold: env_parse("CONTEXT_COMPACTION_THRESHOLD", 0.8),
            keep_recent_messages: env_parse("CONTEXT_KEEP_RECENT_MESSAGES", 6),
        };

        cfg.agent = AgentConfig {
            enabled: env_bool("AGENT_ENABLED", true),
            max_concurrent: env_parse("AGENT_MAX_CONCURRENT", 4),
            timeout_seconds: env_parse("AGENT_TIMEOUT_SECONDS", 180),
        };

        cfg
    }

    /// Имя активной модели у выбранного провайдера.
    pub fn active_model(&self) -> &str {
        match self.llm_provider {
            LlmProvider::Ollama => &self.ollama.model,
            LlmProvider::Cerebras => &self.cerebras.model,
            LlmProvider::DeepSeek => &self.deepseek.model,
            LlmProvider::GoogleAi => &self.google_ai.model,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_parsing() {
        assert_eq!(LlmProvider::parse("cerebras"), LlmProvider::Cerebras);
        assert_eq!(LlmProvider::parse("GOOGLE"), LlmProvider::GoogleAi);
        assert_eq!(LlmProvider::parse("неизвестно"), LlmProvider::Ollama);
    }

    #[test]
    fn defaults_are_sane() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.active_model(), "qwen3:4b");
        assert!(cfg.context.compaction_threshold > 0.0 && cfg.context.compaction_threshold <= 1.0);
    }
}
