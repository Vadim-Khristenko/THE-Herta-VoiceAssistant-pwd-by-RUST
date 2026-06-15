//! `herta` — точка входа. По умолчанию запускает TUI; поддерживает одноразовый
//! текстовый режим и самодиагностику.

mod doctor;

use std::sync::Arc;

use clap::{Parser, Subcommand};
use herta_core::{AppConfig, ContextManager, DialogueMemory, LongMemoryStore, Message};
use herta_core::persona;
use herta_agent::Supervisor;
use herta_llm::ChatClient;
use herta_tui::App;

#[derive(Parser, Debug)]
#[command(name = "herta", version, about = "Великая Герта — голосовой ассистент и TUI на Rust")]
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
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level.to_lowercase()));
    // В TUI логи в stdout ломают рендер, поэтому пишем в stderr.
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).try_init();
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

/// Блок долговременной памяти для инъекции в системный промпт.
fn long_memory_block(config: &AppConfig) -> Option<String> {
    if !config.long_memory.enabled {
        return None;
    }
    let store = LongMemoryStore::load(&config.long_memory.path, config.long_memory.max_facts, true);
    store.format_for_prompt()
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

    let supervisor = Supervisor::new(Arc::clone(&client), &config.agent, persona::build_system_prompt(Some(client.model_name())));
    let ctx_manager = ContextManager::new(&config.context);
    let mem_block = long_memory_block(&config);

    let app = App::new(client, supervisor, ctx_manager, config.context.max_tokens, mem_block);
    app.run().await?;
    Ok(())
}

async fn run_oneshot(config: &AppConfig, prompt: &str) -> anyhow::Result<()> {
    let client = herta_llm::build_client(config)?;
    client.warm_up().await?;

    // Быстрый ответ об идентичности без модели.
    if persona::is_identity_query(prompt) {
        println!("{}", persona::build_identity_reply(prompt));
        return Ok(());
    }

    let mut messages = persona::build_bootstrap_messages(Some(client.model_name()), long_memory_block(config).as_deref());
    // Кратковременная история для связности.
    let memory = DialogueMemory::new(
        &config.memory.path,
        config.memory.max_messages,
        config.memory.context_messages,
        config.memory.enabled,
    );
    messages.extend(memory.load_context_messages());
    messages.push(Message::user(prompt.to_string()));

    let reply = client.chat(&messages).await?;
    let reply = if reply.trim().is_empty() { "Пустой ответ модели.".to_string() } else { reply };
    println!("{reply}");
    memory.append_turn(prompt, &reply)?;
    Ok(())
}
