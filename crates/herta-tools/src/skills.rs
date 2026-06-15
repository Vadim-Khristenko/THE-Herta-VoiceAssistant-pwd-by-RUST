//! Навыки как инструменты агента (прогрессивное раскрытие).
//!
//! `list_skills` отдаёт каталог (имя/назначение/когда применять), `use_skill`
//! подгружает полное тело конкретного навыка. Так модель держит в контексте
//! только дешёвые карточки и разворачивает инструкции лишь при необходимости —
//! включая навык авто-сжатия контекста.

use crate::registry::Tool;
use async_trait::async_trait;
use herta_core::{ParamType, Skill, ToolCall, ToolParameter, ToolResult, ToolSpec};
use std::sync::Arc;

/// Каталог загруженных навыков (иммутабельный после загрузки).
#[derive(Debug, Clone)]
pub struct SkillLibrary {
    skills: Arc<Vec<Skill>>,
}

impl SkillLibrary {
    /// Загрузить навыки из каталога (`*.herta`).
    pub fn load(dir: impl AsRef<std::path::Path>) -> Self {
        Self {
            skills: Arc::new(herta_core::load_skills(dir)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn catalog(&self) -> String {
        if self.skills.is_empty() {
            return "Навыки не найдены.".to_string();
        }
        self.skills
            .iter()
            .map(Skill::summary)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn find(&self, name: &str) -> Option<&Skill> {
        self.skills
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name.trim()))
    }
}

/// `list_skills` — перечислить доступные навыки.
pub struct ListSkillsTool {
    lib: SkillLibrary,
}
impl ListSkillsTool {
    pub fn new(lib: SkillLibrary) -> Self {
        Self { lib }
    }
}

#[async_trait]
impl Tool for ListSkillsTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "list_skills",
            "Перечислить доступные навыки Герты с их назначением и условием применения. Без параметров. \
             Вызови первым делом, если задача похожа на специализированную (например, сжатие контекста), \
             чтобы узнать, какой навык загрузить через use_skill.",
            vec![],
        )
    }
    async fn call(&self, _call: &ToolCall) -> ToolResult {
        ToolResult::ok("list_skills", self.lib.catalog())
    }
}

/// `use_skill` — загрузить полное тело навыка по имени.
pub struct UseSkillTool {
    lib: SkillLibrary,
}
impl UseSkillTool {
    pub fn new(lib: SkillLibrary) -> Self {
        Self { lib }
    }
}

#[async_trait]
impl Tool for UseSkillTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec::new(
            "use_skill",
            "Загрузить полные инструкции конкретного навыка по его имени (см. list_skills). Возвращает \
             пошаговое руководство, которому нужно следовать. Используй, когда условие применения навыка \
             выполнено — например, навык context-compaction при приближении к лимиту контекста.",
            vec![ToolParameter::new("name", ParamType::String, "Имя навыка из каталога list_skills", true)],
        )
    }
    async fn call(&self, call: &ToolCall) -> ToolResult {
        let Some(name) = call.arg_str("name") else {
            return ToolResult::rejected("use_skill", "не передано `name`");
        };
        match self.lib.find(&name) {
            Some(skill) => ToolResult::ok(
                "use_skill",
                format!("Навык `{}`:\n{}", skill.name, skill.body),
            ),
            None => ToolResult::rejected(
                "use_skill",
                format!("навык `{name}` не найден; вызови list_skills"),
            ),
        }
    }
}
