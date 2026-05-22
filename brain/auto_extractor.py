from __future__ import annotations

import json
import logging
import re
from typing import TYPE_CHECKING, Any

from brain.long_memory import VALID_CATEGORIES, LongMemoryStore


if TYPE_CHECKING:
    from main import ChatClient


logger = logging.getLogger(__name__)

EXTRACTOR_SYSTEM_PROMPT = (
    'Ты помощник по извлечению долговременных фактов из диалога. '
    'Ты получаешь последние реплики между пользователем и ассистентом Гертой. '
    'Верни ТОЛЬКО JSON-массив без какого-либо текста до или после. '
    'Каждый элемент массива — объект {"category": ..., "content": ...}. '
    f'Допустимые категории: {sorted(VALID_CATEGORIES)}. '
    'Сохраняй ТОЛЬКО факты, которые: '
    '(1) стабильны и пригодятся в будущих сессиях; '
    '(2) ещё не очевидны из имени проекта или системного промпта; '
    '(3) сформулированы конкретно (одна мысль на факт, без воды). '
    'Игнорируй мелочи, временные эмоции, метакомментарии и шум. '
    'Если ничего стабильного нет — верни пустой массив [].'
)

JSON_ARRAY_RE = re.compile(r'\[[\s\S]*\]')


class AutoFactExtractor:
    def __init__(
        self,
        store: LongMemoryStore,
        chat_client: 'ChatClient',
        *,
        interval_turns: int,
        history_window: int = 12,
    ) -> None:
        self.store = store
        self.chat_client = chat_client
        self.interval_turns = max(1, interval_turns)
        self.history_window = max(2, history_window)
        self._turn_counter = 0

    def on_turn_complete(self, messages: list[dict[str, str]]) -> int:
        """Increment turn counter. Returns number of facts added if extraction ran, else 0."""
        if not self.store.enabled or self.interval_turns <= 0:
            return 0

        self._turn_counter += 1
        if self._turn_counter < self.interval_turns:
            return 0

        self._turn_counter = 0
        try:
            return self._extract_and_store(messages)
        except Exception as exc:
            logger.warning("Auto fact extraction failed: %s", exc)
            return 0

    def _extract_and_store(self, messages: list[dict[str, str]]) -> int:
        recent = [m for m in messages if m.get('role') in {'user', 'assistant'}][-self.history_window:]
        if len(recent) < 2:
            return 0

        transcript = '\n'.join(
            f"{'Пользователь' if m['role'] == 'user' else 'Герта'}: {m['content']}"
            for m in recent
        )
        request_messages = [
            {'role': 'system', 'content': EXTRACTOR_SYSTEM_PROMPT},
            {'role': 'user', 'content': f'Последние реплики:\n\n{transcript}\n\nВерни JSON-массив фактов.'},
        ]

        reply = self.chat_client.chat(request_messages)
        if not reply:
            return 0

        facts_payload = _parse_facts_json(reply)
        if not facts_payload:
            return 0

        added = 0
        for item in facts_payload:
            category = str(item.get('category', '')).strip().lower()
            content = str(item.get('content', '')).strip()
            if not content:
                continue
            if category not in VALID_CATEGORIES:
                category = 'notes'
            fact = self.store.add_fact(content, category, source='auto')
            if fact is not None and fact.source == 'auto':
                added += 1

        if added:
            logger.info("Auto-extractor stored %d new fact(s).", added)
        return added


def _parse_facts_json(raw: str) -> list[dict[str, Any]]:
    match = JSON_ARRAY_RE.search(raw)
    if match is None:
        return []
    try:
        data = json.loads(match.group(0))
    except json.JSONDecodeError:
        return []
    if not isinstance(data, list):
        return []
    return [item for item in data if isinstance(item, dict)]
