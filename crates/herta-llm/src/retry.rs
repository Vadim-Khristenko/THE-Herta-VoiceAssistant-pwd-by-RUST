//! Повторы с экспоненциальной задержкой для временных сетевых сбоев.

use std::future::Future;
use std::time::Duration;

/// HTTP-статусы, которые имеет смысл повторять.
pub fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 409 | 429 | 500 | 502 | 503 | 504)
}

/// Выполнить async-операцию с повторами. `attempts` — максимум попыток (>=1).
/// `op` возвращает `Ok(T)` при успехе или `Err((retryable, error))`.
pub async fn with_backoff<T, E, F, Fut>(attempts: u32, mut op: F) -> std::result::Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = std::result::Result<T, (bool, E)>>,
{
    let attempts = attempts.max(1);
    let mut last_err: Option<E> = None;
    for attempt in 0..attempts {
        match op().await {
            Ok(value) => return Ok(value),
            Err((retryable, err)) => {
                last_err = Some(err);
                if !retryable || attempt + 1 == attempts {
                    break;
                }
                // 0.5s, 1s, 2s, 4s, ... с потолком 16s.
                let backoff_ms = (500u64 << attempt.min(5)).min(16_000);
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            }
        }
    }
    Err(last_err.expect("цикл повторов гарантирует наличие ошибки"))
}
