//! Сборка реестра инструментов из конфигурации и режима работы.
//!
//! Регистрируются все инструменты; видимость и исполнение фильтрует политика
//! режима ([`whatcode_core::Policy`]) внутри реестра. Базовый набор (git, файлы,
//! время, fetch_url, навыки, cargo, uv, toolchain) доступен всегда; память,
//! веб-поиск, анализ кода и системные действия — по флагам конфигурации.

use crate::build_tools::{ProjectInfoTool, VerifyBuildTool};
use crate::code_tools::{LintTool, TypeCheckTool};
use crate::fs_tools::{AppendFileTool, ListDirTool, ReadFileTool, WriteFileTool};
use crate::git::advanced::*;
use crate::git::read::*;
use crate::git::write::*;
use crate::http_tool::FetchUrlTool;
use crate::memory_tools::{ForgetTool, RecallTool, RememberTool};
use crate::proc_tool::ProcessTool;
use crate::registry::ToolRegistry;
use crate::skills::{ListSkillsTool, SkillLibrary, UseSkillTool};
use crate::system_actions::{CreateNoteTool, OpenUrlTool};
use crate::time_tool::CurrentTimeTool;
use crate::toolchain::{CheckToolchainTool, InstallToolchainTool};
use crate::web_search::WebSearchTool;
use whatcode_core::config::AppConfig;
use whatcode_core::{AgentMode, LongMemoryStore, ToolRisk};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Каталог навыков из окружения `WHATCODE_SKILLS_DIR` (по умолчанию `skills`).
fn skills_dir() -> String {
    std::env::var("WHATCODE_SKILLS_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "skills".to_string())
}

/// Описание одного процесс-инструмента для bulk-регистрации.
struct ProcessToolDef {
    name: &'static str,
    description: &'static str,
    program: &'static str,
    base_args: &'static [&'static str],
    timeout_secs: u64,
}

/// Зарегистрировать несколько `ProcessTool` с заданным уровнем риска.
fn register_process_tools(reg: &mut ToolRegistry, tools: &[ProcessToolDef], risk: ToolRisk) {
    for t in tools {
        reg.register(Arc::new(ProcessTool::new(
            t.name,
            t.description,
            t.program,
            t.base_args.to_vec(),
            risk,
            t.timeout_secs,
        )));
    }
}

/// Зарегистрировать инструменты автономной разработки на Rust (cargo).
fn register_cargo(reg: &mut ToolRegistry) {
    register_process_tools(
        reg,
        &[
            ProcessToolDef { name: "cargo_check", description: "Быстрая проверка компиляции Rust-проекта без сборки артефактов (`cargo check`). Используй часто во время правок.", program: "cargo", base_args: &["check"], timeout_secs: 300 },
            ProcessToolDef { name: "cargo_build", description: "Собрать Rust-проект (`cargo build`). Доп. аргументы: например `--release`.", program: "cargo", base_args: &["build"], timeout_secs: 600 },
            ProcessToolDef { name: "cargo_test", description: "Запустить тесты Rust-проекта (`cargo test`). Можно указать имя теста в args.", program: "cargo", base_args: &["test"], timeout_secs: 600 },
            ProcessToolDef { name: "cargo_clippy", description: "Линтер Rust (`cargo clippy --all-targets`). Показывает предупреждения и ошибки качества.", program: "cargo", base_args: &["clippy", "--all-targets"], timeout_secs: 600 },
            ProcessToolDef { name: "cargo_fmt", description: "Отформатировать код Rust (`cargo fmt`). Изменяет файлы. Для проверки используй args `--check`.", program: "cargo", base_args: &["fmt"], timeout_secs: 120 },
            ProcessToolDef { name: "cargo_add", description: "Добавить зависимость в Cargo.toml (`cargo add <crate>`). Имя крейта передай в args.", program: "cargo", base_args: &["add"], timeout_secs: 120 },
            ProcessToolDef { name: "cargo_run", description: "Запустить бинарь проекта (`cargo run`). Аргументы программы — после `--` в args.", program: "cargo", base_args: &["run"], timeout_secs: 600 },
        ],
        ToolRisk::Write,
    );
}

/// Зарегистрировать инструменты Python через UV-менеджер.
fn register_uv(reg: &mut ToolRegistry) {
    register_process_tools(
        reg,
        &[
            ProcessToolDef { name: "uv_run", description: "Запустить Python-команду/скрипт в окружении проекта через UV (`uv run`). Команда — в args.", program: "uv", base_args: &["run"], timeout_secs: 600 },
            ProcessToolDef { name: "uv_add", description: "Добавить Python-зависимость через UV (`uv add <pkg>`). Имя пакета — в args.", program: "uv", base_args: &["add"], timeout_secs: 300 },
            ProcessToolDef { name: "uv_sync", description: "Синхронизировать окружение Python по lock-файлу (`uv sync`).", program: "uv", base_args: &["sync"], timeout_secs: 600 },
            ProcessToolDef { name: "uv_pip", description: "Управление пакетами в стиле pip через UV (`uv pip ...`). Подкоманду и аргументы передай в args, напр. `install requests`.", program: "uv", base_args: &["pip"], timeout_secs: 600 },
        ],
        ToolRisk::Write,
    );
}

/// Зарегистрировать инструменты TypeScript/JavaScript через Bun.
fn register_bun(reg: &mut ToolRegistry) {
    register_process_tools(
        reg,
        &[
            ProcessToolDef { name: "bun_run", description: "Запустить скрипт через Bun (`bun run`). Имя скрипта/команды — в args.", program: "bun", base_args: &["run"], timeout_secs: 600 },
            ProcessToolDef { name: "bun_test", description: "Запустить тесты через Bun (`bun test`). Фильтр — в args.", program: "bun", base_args: &["test"], timeout_secs: 600 },
            ProcessToolDef { name: "bun_build", description: "Собрать проект через Bun (`bun build`). Аргументы — в args.", program: "bun", base_args: &["build"], timeout_secs: 600 },
            ProcessToolDef { name: "bun_add", description: "Добавить npm-зависимость через Bun (`bun add <pkg>`). Имя пакета — в args.", program: "bun", base_args: &["add"], timeout_secs: 300 },
            ProcessToolDef { name: "bun_install", description: "Установить зависимости через Bun (`bun install`).", program: "bun", base_args: &["install"], timeout_secs: 600 },
            ProcessToolDef { name: "bun_lint", description: "Проверить код через Bun-линтер (`bun lint` или `bun x eslint`). Команда — в args.", program: "bun", base_args: &["lint"], timeout_secs: 300 },
        ],
        ToolRisk::Write,
    );
}

/// Построить полный реестр инструментов для агента в заданном режиме.
pub fn build_registry(
    config: &AppConfig,
    long_memory: Arc<Mutex<LongMemoryStore>>,
    mode: AgentMode,
) -> ToolRegistry {
    let mut reg = ToolRegistry::with_mode(mode);

    // --- разведка и чтение (Git + файлы + сеть) ---
    reg.register(Arc::new(GitStatusTool::default()))
        .register(Arc::new(GitLogTool::default()))
        .register(Arc::new(GitDiffTool::default()))
        .register(Arc::new(GitDiffStagedTool::default()))
        .register(Arc::new(GitBranchTool::default()))
        .register(Arc::new(GitBranchRemoteTool::default()))
        .register(Arc::new(GitShowTool::default()))
        .register(Arc::new(GitGrepTool::default()))
        .register(Arc::new(GitRemoteTool::default()))
        .register(Arc::new(ReadFileTool))
        .register(Arc::new(ListDirTool))
        .register(Arc::new(FetchUrlTool::default()))
        .register(Arc::new(CurrentTimeTool))
        .register(Arc::new(CheckToolchainTool));

    // --- запись и git-мутации ---
    reg.register(Arc::new(WriteFileTool))
        .register(Arc::new(AppendFileTool))
        .register(Arc::new(GitAddTool::default()))
        .register(Arc::new(GitResetHeadTool::default()))
        .register(Arc::new(GitCommitTool::default()))
        .register(Arc::new(GitPushTool::default()))
        .register(Arc::new(GitPullTool::default()))
        .register(Arc::new(GitCheckoutTool::default()))
        .register(Arc::new(GitStashTool::default()))
        .register(Arc::new(GitResetTool::default()))
        .register(Arc::new(GitRevertTool::default()))
        .register(Arc::new(GitRebaseTool::default()))
        .register(Arc::new(GitCherryPickTool::default()))
        .register(Arc::new(GitCleanTool::default()))
        .register(Arc::new(GitMergeTool::default()));

    // --- полуавтоматические git-workflows ---
    reg.register(Arc::new(GitRollbackCommitTool::default()))
        .register(Arc::new(GitSyncBranchTool::default()))
        .register(Arc::new(GitUnstageAllTool::default()))
        .register(Arc::new(GitDiscardChangesTool::default()))
        .register(Arc::new(GitCommitAllTool::default()))
        .register(Arc::new(GitSavepointTool::default()));

    // --- автономная разработка ---
    register_cargo(&mut reg);
    register_uv(&mut reg);
    register_bun(&mut reg);
    reg.register(Arc::new(VerifyBuildTool))
        .register(Arc::new(ProjectInfoTool));

    // --- установка инструментария (опасное) ---
    reg.register(Arc::new(InstallToolchainTool));

    // --- навыки (прогрессивное раскрытие) ---
    let library = SkillLibrary::load(skills_dir());
    if !library.is_empty() {
        reg.register(Arc::new(ListSkillsTool::new(library.clone())))
            .register(Arc::new(UseSkillTool::new(library)));
    }

    // --- долговременная память ---
    if config.long_memory.enabled {
        reg.register(Arc::new(RememberTool::new(Arc::clone(&long_memory))))
            .register(Arc::new(RecallTool::new(Arc::clone(&long_memory))))
            .register(Arc::new(ForgetTool::new(Arc::clone(&long_memory))));
    }

    // --- веб-поиск ---
    if config.web_search.enabled {
        reg.register(Arc::new(WebSearchTool::new(config.web_search.clone())));
    }

    // --- анализ кода (Python) ---
    if config.code_tools.enabled {
        reg.register(Arc::new(TypeCheckTool::new(config.code_tools.clone())))
            .register(Arc::new(LintTool::new(config.code_tools.clone())));
    }

    // --- системные действия ---
    if config.system_actions.enabled {
        reg.register(Arc::new(OpenUrlTool::new(config.system_actions.clone())))
            .register(Arc::new(CreateNoteTool::new(config.system_actions.clone())));
    }

    reg
}
