//! Долговременная память: персистентные факты между сессиями.
//! Порт `brain/long_memory.py`. Дедупликация по нормализованному содержимому.

use crate::error::{HertaError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Категория факта. Неизвестные строки маппятся в `Notes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FactCategory {
    User,
    Project,
    Preferences,
    Notes,
}

impl FactCategory {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "user" => Self::User,
            "project" => Self::Project,
            "preferences" => Self::Preferences,
            _ => Self::Notes,
        }
    }

    pub fn label_ru(self) -> &'static str {
        match self {
            Self::User => "О пользователе",
            Self::Project => "О проекте",
            Self::Preferences => "Предпочтения",
            Self::Notes => "Заметки",
        }
    }

    fn all() -> [FactCategory; 4] {
        [Self::User, Self::Project, Self::Preferences, Self::Notes]
    }
}

/// Источник факта.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FactSource {
    Explicit,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub fact_id: String,
    pub category: FactCategory,
    pub content: String,
    pub created_at: String,
    pub source: FactSource,
}

#[derive(Debug, Serialize, Deserialize)]
struct LongMemoryFile {
    version: u32,
    facts: Vec<Fact>,
}

/// Хранилище долговременных фактов.
#[derive(Debug)]
pub struct LongMemoryStore {
    path: PathBuf,
    max_facts: usize,
    enabled: bool,
    facts: Vec<Fact>,
}

fn normalize_content(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

impl LongMemoryStore {
    pub fn load(path: impl Into<PathBuf>, max_facts: usize, enabled: bool) -> Self {
        let path = path.into();
        let facts = if enabled {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str::<LongMemoryFile>(&raw).ok())
                .map(|f| f.facts)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Self { path, max_facts: max_facts.max(1), enabled, facts }
    }

    pub fn all_facts(&self) -> &[Fact] {
        &self.facts
    }

    pub fn by_category(&self, category: FactCategory) -> Vec<&Fact> {
        self.facts.iter().filter(|f| f.category == category).collect()
    }

    /// Добавить факт. Возвращает `None`, если дубликат по нормализованному тексту.
    pub fn add_fact(&mut self, content: &str, category: FactCategory, source: FactSource) -> Result<Option<Fact>> {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let norm = normalize_content(trimmed);
        if self.facts.iter().any(|f| normalize_content(&f.content) == norm) {
            return Ok(None);
        }
        let fact = Fact {
            fact_id: uuid::Uuid::new_v4().to_string(),
            category,
            content: trimmed.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            source,
        };
        self.facts.push(fact.clone());
        if self.facts.len() > self.max_facts {
            let overflow = self.facts.len() - self.max_facts;
            self.facts.drain(0..overflow);
        }
        self.persist()?;
        Ok(Some(fact))
    }

    pub fn remove_by_content(&mut self, substring: &str) -> Result<usize> {
        let needle = normalize_content(substring);
        if needle.is_empty() {
            return Ok(0);
        }
        let before = self.facts.len();
        self.facts.retain(|f| !normalize_content(&f.content).contains(&needle));
        let removed = before - self.facts.len();
        if removed > 0 {
            self.persist()?;
        }
        Ok(removed)
    }

    pub fn clear(&mut self) -> Result<usize> {
        let n = self.facts.len();
        self.facts.clear();
        self.persist()?;
        Ok(n)
    }

    /// Блок для инъекции в системный промпт. Пусто - возвращает `None`.
    pub fn format_for_prompt(&self) -> Option<String> {
        if self.facts.is_empty() {
            return None;
        }
        let mut out = String::from("Известные факты (долговременная память):");
        for category in FactCategory::all() {
            let items = self.by_category(category);
            if items.is_empty() {
                continue;
            }
            out.push_str(&format!("\n{}:", category.label_ru()));
            for fact in items {
                out.push_str(&format!("\n- {}", fact.content));
            }
        }
        Some(out)
    }

    fn persist(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = LongMemoryFile { version: 1, facts: self.facts.clone() };
        std::fs::write(&self.path, serde_json::to_string_pretty(&file)?).map_err(HertaError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_and_format() {
        let dir = std::env::temp_dir().join(format!("herta-lm-{}", uuid::Uuid::new_v4()));
        let path = dir.join("lm.json");
        let mut store = LongMemoryStore::load(&path, 100, true);
        assert!(store.add_fact("Пользователь любит Rust", FactCategory::User, FactSource::Explicit).unwrap().is_some());
        assert!(store.add_fact("пользователь  любит   rust", FactCategory::User, FactSource::Auto).unwrap().is_none());
        let block = store.format_for_prompt().unwrap();
        assert!(block.contains("О пользователе"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
