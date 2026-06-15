//! Кратковременная память диалога: персистентная история сообщений в JSON.
//! Порт `brain/memory.py`. Хранит только роли user/assistant с непустым текстом.

use crate::error::{HertaError, Result};
use crate::message::{Message, Role};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct MemoryFile {
    version: u32,
    messages: Vec<StoredMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredMessage {
    role: String,
    content: String,
}

impl StoredMessage {
    fn into_message(self) -> Option<Message> {
        let content = self.content.trim().to_string();
        if content.is_empty() {
            return None;
        }
        match self.role.as_str() {
            "user" => Some(Message::user(content)),
            "assistant" => Some(Message::assistant(content)),
            _ => None,
        }
    }

    fn from_message(msg: &Message) -> Option<Self> {
        let content = msg.content.trim();
        if content.is_empty() {
            return None;
        }
        let role = match msg.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            _ => return None,
        };
        Some(Self { role: role.to_string(), content: content.to_string() })
    }
}

/// Хранилище кратковременной истории диалога.
#[derive(Debug)]
pub struct DialogueMemory {
    path: PathBuf,
    max_messages: usize,
    context_messages: usize,
    enabled: bool,
}

impl DialogueMemory {
    pub fn new(path: impl Into<PathBuf>, max_messages: usize, context_messages: usize, enabled: bool) -> Self {
        Self { path: path.into(), max_messages: max_messages.max(2), context_messages, enabled }
    }

    fn read_all(&self) -> Vec<StoredMessage> {
        if !self.enabled {
            return Vec::new();
        }
        let Ok(raw) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        serde_json::from_str::<MemoryFile>(&raw).map(|f| f.messages).unwrap_or_default()
    }

    /// Последние `context_messages` сообщений. Гарантирует, что срез начинается
    /// с реплики пользователя - иначе провайдеры отвергают историю.
    pub fn load_context_messages(&self) -> Vec<Message> {
        if !self.enabled {
            return Vec::new();
        }
        let stored = self.read_all();
        let mut messages: Vec<Message> = stored.into_iter().filter_map(StoredMessage::into_message).collect();

        if messages.len() > self.context_messages {
            messages.drain(0..messages.len() - self.context_messages);
        }
        while messages.first().map(|m| m.role) == Some(Role::Assistant) {
            messages.remove(0);
        }
        messages
    }

    /// Добавить пару user+assistant и сохранить, обрезав до `max_messages`.
    pub fn append_turn(&self, user_text: &str, assistant_text: &str) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let mut all = self.read_all();
        if let Some(sm) = StoredMessage::from_message(&Message::user(user_text)) {
            all.push(sm);
        }
        if let Some(sm) = StoredMessage::from_message(&Message::assistant(assistant_text)) {
            all.push(sm);
        }
        if all.len() > self.max_messages {
            all.drain(0..all.len() - self.max_messages);
        }
        self.write(&all)
    }

    fn write(&self, messages: &[StoredMessage]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = MemoryFile { version: 1, messages: messages.to_vec() };
        let raw = serde_json::to_string_pretty(&file)?;
        std::fs::write(&self.path, raw).map_err(HertaError::Io)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_user_first() {
        let dir = std::env::temp_dir().join(format!("herta-mem-{}", uuid::Uuid::new_v4()));
        let path = dir.join("dlg.json");
        let mem = DialogueMemory::new(&path, 10, 4, true);
        mem.append_turn("привет", "Уже лучше.").unwrap();
        mem.append_turn("как дела", "Сносно.").unwrap();
        let ctx = mem.load_context_messages();
        assert_eq!(ctx.first().unwrap().role, Role::User);
        std::fs::remove_dir_all(&dir).ok();
    }
}
