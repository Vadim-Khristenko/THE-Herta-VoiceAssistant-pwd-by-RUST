//! `herta` — точка входа. По умолчанию запускает TUI; поддерживает одноразовый
//! текстовый режим и самодиагностику.

mod doctor;

use std::sync::Arc;

use clap::{Parser, Subcommand};
use herta_agent::{run_tool_loop, Supervisor};
use herta_core::persona;
use herta_core::{AppConfig, ContextManager, DialogueMemory, LongMemoryStore, Message};
use herta_llm::ChatClient;
use herta_tools::ToolRegistry;
use herta_tui::App;
use tokio::sync::Mutex;

#[derive(Parser, Debug)]
#[command(
    name = "herta",
    version,
    about = "Великая Герта — голосовой ассистент и TUI на Rust"
)]
struct Cli {
    /// Одноразовый запрос: вывести ответ и выйти.
    #[arg(short, long, value_name = "ТЕКСТ")]
    text: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Запустить интерактивный TUI (поведение по умолчанию).
    Tui,
    /// Самодиагностика конфигурации и окружения.
    Doctor,
}

fn init_tracing(level: &str) {
    use tracing_subscriber::EnvFilter;
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level.to_lowercase()));
    // В TUI логи в stdout ломают рендер, поэтому пишем в stderr.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::from_env();
    init_tracing(&config.log_level);

    match cli.command {
        Some(Command::Doctor) => {
            let code = doctor::run(&config).await;
            std::process::exit(code);
        }
        Some(Command::Tui) | None => {
            if let Some(prompt) = cli.text {
                run_oneshot(&config, &prompt).await
            } else {
                run_tui(config).await
            }
        }
    }
}

/// Загрузить долговременную память, вернуть общий стор и блок для промпта.
fn load_long_memory(config: &AppConfig) -> (Arc<Mutex<LongMemoryStore>>, Option<String>) {
    let store = LongMemoryStore::load(
        &config.long_memory.path,
        config.long_memory.max_facts,
        config.long_memory.enabled,
    );
    let block = if config.long_memory.enabled {
        store.format_for_prompt()
    } else {
        None
    };
    (Arc::new(Mutex::new(store)), block)
}

async fn run_tui(config: AppConfig) -> anyhow::Result<()> {
    let client: Arc<dyn ChatClient> = Arc::from(herta_llm::build_client(&config)?);
    // Прогрев в фоне: не блокируем запуск интерфейса.
    {
        let warm = Arc::clone(&client);
        tokio::spawn(async move {
            if let Err(e) = warm.warm_up().await {
                tracing::warn!(error = %e, "прогрев модели не удался");
            }
        });
    }

    let (long_memory, mem_block) = load_long_memory(&config);
    let registry = Arc::new(herta_tools::build_registry(
        &config,
        Arc::clone(&long_memory),
    ));

    let supervisor = Supervisor::new(
        Arc::clone(&client),
        &config.agent,
        persona::build_system_prompt(Some(client.model_name())),
    );
    let ctx_manager = ContextManager::new(&config.context);
    let voice = herta_voice::Voice::from_config(&config.voice);

    let app = App::new(
        client,
        registry,
        supervisor,
        ctx_manager,
        voice,
        config.context.max_tokens,
        config.agent.tool_loop_iterations,
        mem_block,
    );
    app.run().await?;
    Ok(())
}

async fn run_oneshot(config: &AppConfig, prompt: &str) -> anyhow::Result<()> {
    // Быстрый ответ об идентичности без модели и без сети.
    if persona::is_identity_query(prompt) {
        println!("{}", persona::build_identity_reply(prompt));
        return Ok(());
    }

    let client = herta_llm::build_client(config)?;
    client.warm_up().await?;

    let (long_memory, mem_block) = load_long_memory(config);
    let registry: ToolRegistry = herta_tools::build_registry(config, long_memory);

    let mut messages =
        persona::build_bootstrap_messages(Some(client.model_name()), mem_block.as_deref());
    // Кратковременная история для связности.
    let memory = DialogueMemory::new(
        &config.memory.path,
        config.memory.max_messages,
        config.memory.context_messages,
        config.memory.enabled,
    );
    messages.extend(memory.load_context_messages());
    messages.push(Message::user(prompt.to_string()));

    // Одноразовый режим тоже проходит через нативный tool-loop.
    let outcome = run_tool_loop(
        client.as_ref(),
        &registry,
        &messages,
        config.agent.tool_loop_iterations,
    )
    .await?;
    let reply = if outcome.text.trim().is_empty() {
        "Пустой ответ модели.".to_string()
    } else {
        outcome.text
    };
    println!("{reply}");
    memory.append_turn(prompt, &reply)?;
    Ok(())
}
