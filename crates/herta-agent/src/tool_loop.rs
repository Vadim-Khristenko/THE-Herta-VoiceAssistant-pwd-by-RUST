//! Нативный цикл вызова инструментов (tool-loop).
//!
//! Реализация провайдер-агностична: вместо реконструкции native `tool_result`
//! блоков (формат которых различается у Anthropic/OpenAI/Ollama) результаты
//! инструментов подаются обратно как обычная реплика пользователя. Это работает
//! одинаково на всех бэкендах и не ломается на оркестрации tool_use↔tool_result.
//!
//! Цикл ограничен `max_iterations`, чтобы исключить бесконечную рекурсию вызовов.

use herta_core::{Message, ToolResult};
use herta_llm::ChatClient;
use herta_tools::ToolRegistry;

/// Итог работы цикла: финальный текст и протокол выполненных инструментов.
#[derive(Debug, Clone)]
pub struct ToolLoopOutcome {
    pub text: String,
    pub tool_results: Vec<ToolResult>,
    /// Достигнут ли предел итераций (ответ может быть неполным).
    pub hit_limit: bool,
}

/// Прогнать диалог через модель с доступом к инструментам реестра.
///
/// `messages` — полный контекст (персона + история + текущий запрос).
/// Возвращает финальный текстовый ответ после всех вызовов инструментов.
pub async fn run(
    client: &dyn ChatClient,
    registry: &ToolRegistry,
    messages: &[Message],
    max_iterations: usize,
) -> herta_core::Result<ToolLoopOutcome> {
    let specs = registry.specs();
    // Инструментов нет — обычный чат без накладных расходов.
    if specs.is_empty() {
        return Ok(ToolLoopOutcome {
            text: client.chat(messages).await?,
            tool_results: Vec::new(),
            hit_limit: false,
        });
    }

    let mut conversation = messages.to_vec();
    let mut tool_results: Vec<ToolResult> = Vec::new();
    let max_iterations = max_iterations.max(1);

    for iteration in 0..max_iterations {
        let response = client.chat_with_tools(&conversation, &specs).await?;

        if !response.wants_tools() {
            return Ok(ToolLoopOutcome {
                text: response.text,
                tool_results,
                hit_limit: false,
            });
        }

        // Сохраняем текст-рассуждение ассистента (если был) для связности.
        if !response.text.trim().is_empty() {
            conversation.push(Message::assistant(response.text.clone()));
        }

        // Выполняем все запрошенные вызовы и собираем результаты в одну реплику.
        let mut block = String::from("[Результаты инструментов]");
        for call in &response.tool_calls {
            let result = registry.dispatch(call).await;
            block.push_str(&format!(
                "\n- {} ({}): {}",
                result.tool_name,
                if result.executed {
                    "ok"
                } else {
                    "отклонено"
                },
                result.message
            ));
            tool_results.push(result);
        }

        let last = iteration + 1 == max_iterations;
        if last {
            block.push_str("\n\nДостигнут предел вызовов инструментов. Дай финальный ответ по имеющимся данным.");
        } else {
            block.push_str("\n\nУчти результаты и продолжи ответ. Вызывай инструменты снова только при необходимости.");
        }
        conversation.push(Message::user(block));
    }

    // Предел исчерпан — финальный чистый запрос без инструментов.
    let text = client.chat(&conversation).await?;
    Ok(ToolLoopOutcome {
        text,
        tool_results,
        hit_limit: true,
    })
}
