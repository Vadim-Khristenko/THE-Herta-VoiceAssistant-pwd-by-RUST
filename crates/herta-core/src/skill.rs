//! Собственный компактный формат навыков `.herta` и его загрузчик.
//!
//! Навык — это переиспользуемый блок инструкций с прогрессивным раскрытием:
//! в контексте модели по умолчанию находятся только `name`/`when`/`desc`
//! (дёшево), а тяжёлое `body` подгружается по требованию инструментом.
//!
//! Формат файла (оптимизирован под минимум токенов и однозначный парсинг):
//! ```text
//! @skill имя-навыка
//! @when когда применять (одна строка-триггер)
//! @desc краткое назначение (одна строка)
//! ---
//! Тело навыка: пошаговые инструкции в свободном виде.
//! ```
//! Строки до разделителя `---` — заголовок (директивы `@`), после — тело.

use std::path::Path;

/// Разобранный навык.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub when: String,
    pub desc: String,
    pub body: String,
}

/// Ошибка разбора навыка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillParseError {
    MissingName,
    MissingSeparator,
}

impl std::fmt::Display for SkillParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillParseError::MissingName => write!(f, "нет директивы @skill <имя>"),
            SkillParseError::MissingSeparator => {
                write!(f, "нет разделителя `---` между заголовком и телом")
            }
        }
    }
}

impl Skill {
    /// Разобрать навык из строки в формате `.herta`.
    pub fn parse(text: &str) -> Result<Skill, SkillParseError> {
        let (header, body) = text
            .split_once("\n---")
            .ok_or(SkillParseError::MissingSeparator)?;

        let mut name = None;
        let mut when = String::new();
        let mut desc = String::new();

        for line in header.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("@skill") {
                name = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("@when") {
                when = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("@desc") {
                desc = rest.trim().to_string();
            }
        }

        let name = name
            .filter(|n| !n.is_empty())
            .ok_or(SkillParseError::MissingName)?;
        Ok(Skill {
            name,
            when,
            desc,
            body: body.trim_start_matches(['-', '\n']).trim().to_string(),
        })
    }

    /// Краткая карточка для каталога (без тела).
    pub fn summary(&self) -> String {
        format!("- {} — {} [когда: {}]", self.name, self.desc, self.when)
    }
}

/// Загрузить все `*.herta` из каталога. Несуществующий каталог — пустой список,
/// битые файлы пропускаются (логируются вызывающим при желании).
pub fn load_dir(dir: impl AsRef<Path>) -> Vec<Skill> {
    let dir = dir.as_ref();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("herta") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(skill) = Skill::parse(&text) {
                skills.push(skill);
            }
        }
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_skill() {
        let text = "@skill context-compaction\n@when контекст близок к лимиту\n@desc сжать историю\n---\nШаг 1. Сохрани факты.\nШаг 2. Сожми середину.";
        let skill = Skill::parse(text).unwrap();
        assert_eq!(skill.name, "context-compaction");
        assert_eq!(skill.when, "контекст близок к лимиту");
        assert_eq!(skill.desc, "сжать историю");
        assert!(skill.body.starts_with("Шаг 1"));
        assert!(skill.body.contains("Шаг 2"));
    }

    #[test]
    fn rejects_missing_name() {
        let text = "@when всегда\n---\nтело";
        assert_eq!(Skill::parse(text), Err(SkillParseError::MissingName));
    }

    #[test]
    fn rejects_missing_separator() {
        let text = "@skill x\nтело без разделителя";
        assert_eq!(Skill::parse(text), Err(SkillParseError::MissingSeparator));
    }
}
