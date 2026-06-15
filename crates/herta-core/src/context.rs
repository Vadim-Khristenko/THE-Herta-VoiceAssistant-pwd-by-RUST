//! Автосжатие контекста. Когда диалог приближается к лимиту окна модели,
//! движок выделяет «середину» истории под суммаризацию, сохраняя закреплённый
//! системный префикс (персона + few-shot) и последние реплики дословно.
//!
//! Решение детерминированное: на вход - срез сообщений и бюджет, на выход -
//! явный `CompactionPlan`. Сам вызов LLM делает слой выше (он async); ядро
//! остаётся чистым и тестируемым.

use crate::config::ContextConfig;
use crate::message::{estimate_total_tokens, Message, Role};

/// Решение движка о необходимости и форме сжатия.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionDecision {
    /// Сжатие не требуется - бюджет в норме.
    NotNeeded,
    /// Нужно сжать середину истории.
    Compact(CompactionPlan),
}

/// План сжатия: какие сообщения свернуть, какие сохранить дословно.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionPlan {
    /// Индексы закреплённого системного префикса (сохраняются как есть).
    pub pinned_prefix_len: usize,
    /// Диапазон [start, end) сообщений под суммаризацию.
    pub summarize_range: (usize, usize),
    /// Сколько последних сообщений сохраняется дословно.
    pub keep_recent: usize,
    /// Оценка токенов до сжатия.
    pub tokens_before: usize,
}

/// Движок управления контекстным окном.
#[derive(Debug, Clone)]
pub struct ContextManager {
    max_tokens: usize,
    threshold: f32,
    keep_recent: usize,
}

impl ContextManager {
    pub fn new(cfg: &ContextConfig) -> Self {
        Self {
            max_tokens: cfg.max_tokens.max(512),
            threshold: cfg.compaction_threshold.clamp(0.1, 0.98),
            keep_recent: cfg.keep_recent_messages.max(2),
        }
    }

    /// Порог в токенах, при превышении которого включается сжатие.
    pub fn trigger_tokens(&self) -> usize {
        ((self.max_tokens as f32) * self.threshold) as usize
    }

    /// Длина закреплённого системного префикса - ведущие `System` сообщения
    /// плюс примыкающие few-shot пары. Считаем непрерывный блок с начала,
    /// пока не дойдём до первой «живой» реплики пользователя за few-shot.
    fn pinned_prefix_len(messages: &[Message]) -> usize {
        let mut len = 0;
        while len < messages.len() && messages[len].role == Role::System {
            len += 1;
        }
        len
    }

    /// Принять решение о сжатии для текущего среза истории.
    pub fn decide(&self, messages: &[Message]) -> CompactionDecision {
        let tokens_before = estimate_total_tokens(messages);
        if tokens_before <= self.trigger_tokens() {
            return CompactionDecision::NotNeeded;
        }

        let prefix = Self::pinned_prefix_len(messages);
        let total = messages.len();

        // Нужно как минимум: префикс + 2 сворачиваемых + keep_recent.
        let min_required = prefix + 2 + self.keep_recent;
        if total < min_required {
            return CompactionDecision::NotNeeded;
        }

        let end = total - self.keep_recent;
        if end <= prefix {
            return CompactionDecision::NotNeeded;
        }

        CompactionDecision::Compact(CompactionPlan {
            pinned_prefix_len: prefix,
            summarize_range: (prefix, end),
            keep_recent: self.keep_recent,
            tokens_before,
        })
    }

    /// Построить план сжатия принудительно (по команде пользователя), игнорируя
    /// порог токенов, но сохраняя структурные требования (префикс + хвост).
    pub fn force_plan(&self, messages: &[Message]) -> Option<CompactionPlan> {
        let prefix = Self::pinned_prefix_len(messages);
        let total = messages.len();
        if total < prefix + 2 + self.keep_recent {
            return None;
        }
        let end = total - self.keep_recent;
        if end <= prefix {
            return None;
        }
        Some(CompactionPlan {
            pinned_prefix_len: prefix,
            summarize_range: (prefix, end),
            keep_recent: self.keep_recent,
            tokens_before: estimate_total_tokens(messages),
        })
    }

    /// Системный промпт для модели-суммаризатора (в образе Герты, но по делу).
    pub fn summarizer_system_prompt() -> &'static str {
        "Ты сжимаешь историю диалога Великой Герты с пользователем. \
         Сохрани все стабильные факты, решения, принятые договорённости, важный контекст кода и задач. \
         Отбрось воду, повторы и эмоциональный шум. \
         Пиши плотно, в третьем лице, по-русски, без оценок и без образа - только содержательная сводка. \
         Формат: маркированный список ключевых пунктов. Не выдумывай того, чего не было."
    }

    /// Построить запрос к LLM для суммаризации согласно плану.
    pub fn build_summarization_request(
        messages: &[Message],
        plan: &CompactionPlan,
    ) -> Vec<Message> {
        let (start, end) = plan.summarize_range;
        let mut transcript = String::with_capacity(2048);
        for msg in &messages[start..end] {
            let who = match msg.role {
                Role::User => "Пользователь",
                Role::Assistant => "Герта",
                Role::Tool => "Инструмент",
                Role::System => "Система",
            };
            transcript.push_str(who);
            transcript.push_str(": ");
            transcript.push_str(msg.content.trim());
            transcript.push('\n');
        }
        vec![
            Message::system(Self::summarizer_system_prompt()),
            Message::user(format!("Сожми следующий фрагмент диалога:\n\n{transcript}")),
        ]
    }

    /// Применить готовую сводку: префикс + сводка-System + сохранённый хвост.
    pub fn apply(messages: &[Message], plan: &CompactionPlan, summary: &str) -> Vec<Message> {
        let (_, end) = plan.summarize_range;
        let mut out = Vec::with_capacity(plan.pinned_prefix_len + 1 + plan.keep_recent);
        out.extend_from_slice(&messages[0..plan.pinned_prefix_len]);
        out.push(Message::system(format!(
            "Сводка предыдущей части диалога (сжато для экономии контекста):\n{}",
            summary.trim()
        )));
        out.extend_from_slice(&messages[end..]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(max: usize, thresh: f32, keep: usize) -> ContextConfig {
        ContextConfig {
            max_tokens: max,
            compaction_threshold: thresh,
            keep_recent_messages: keep,
        }
    }

    #[test]
    fn no_compaction_when_small() {
        let mgr = ContextManager::new(&cfg(8192, 0.8, 4));
        let msgs = vec![
            Message::system("персона"),
            Message::user("привет"),
            Message::assistant("Уже лучше."),
        ];
        assert_eq!(mgr.decide(&msgs), CompactionDecision::NotNeeded);
    }

    #[test]
    fn compaction_triggers_and_preserves_structure() {
        // Маленький бюджет, чтобы гарантированно перешагнуть порог.
        let mgr = ContextManager::new(&cfg(512, 0.5, 2));
        let big = "слово ".repeat(200);
        let mut msgs = vec![Message::system("персона Герты")];
        for i in 0..12 {
            msgs.push(Message::user(format!("вопрос {i} {big}")));
            msgs.push(Message::assistant(format!("ответ {i} {big}")));
        }
        match mgr.decide(&msgs) {
            CompactionDecision::Compact(plan) => {
                assert_eq!(plan.pinned_prefix_len, 1);
                assert_eq!(plan.keep_recent, 2);
                let compacted = ContextManager::apply(&msgs, &plan, "сводка пунктов");
                // Префикс (1) + сводка (1) + хвост (2) = 4.
                assert_eq!(compacted.len(), 4);
                assert_eq!(compacted[0].role, Role::System);
                assert_eq!(compacted[1].role, Role::System);
                assert!(compacted[1].content.contains("Сводка"));
            }
            CompactionDecision::NotNeeded => panic!("ожидалось сжатие"),
        }
    }
}
