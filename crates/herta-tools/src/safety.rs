//! Проверки безопасности для системных действий. Без внешних regex-зависимостей:
//! простой матчинг по нормализованной строке достаточно надёжен и быстр.

/// Маркеры деструктивных намерений (рус./англ.).
const DESTRUCTIVE_MARKERS: &[&str] = &[
    "удали", "удалить", "сотри", "стереть", "снеси", "формат", "перезапиши", "перезаписать",
    "delete", "remove", "erase", "format", "overwrite", "rm -rf", "drop table",
];

/// Текст содержит признак деструктивного действия?
pub fn looks_destructive(text: &str) -> bool {
    let n = text.to_lowercase();
    DESTRUCTIVE_MARKERS.iter().any(|m| n.contains(m))
}

/// Проверка пути на выход за пределы корня (защита от path traversal).
/// Возвращает `true`, если путь безопасен (внутри `root`).
pub fn path_within_root(root: &std::path::Path, candidate: &std::path::Path) -> bool {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    match candidate.canonicalize() {
        Ok(abs) => abs.starts_with(&root),
        // Несуществующий файл: проверяем по строковому префиксу его родителя.
        Err(_) => candidate
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .map(|p| p.starts_with(&root))
            .unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_destructive() {
        assert!(looks_destructive("удали все файлы"));
        assert!(looks_destructive("please delete everything"));
        assert!(!looks_destructive("создай заметку"));
    }
}
