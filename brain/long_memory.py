from __future__ import annotations

import json
import logging
import secrets
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Final

from config import LongMemoryConfig


logger = logging.getLogger(__name__)

VALID_CATEGORIES: Final[frozenset[str]] = frozenset({'user', 'project', 'preferences', 'notes'})
DEFAULT_CATEGORY: Final[str] = 'notes'
SCHEMA_VERSION: Final[int] = 1


@dataclass(slots=True)
class Fact:
    fact_id: str
    category: str
    content: str
    created_at: str
    source: str = 'explicit'

    def to_dict(self) -> dict[str, str]:
        return {
            'fact_id': self.fact_id,
            'category': self.category,
            'content': self.content,
            'created_at': self.created_at,
            'source': self.source,
        }

    @classmethod
    def from_dict(cls, payload: dict[str, Any]) -> Fact | None:
        category = payload.get('category')
        content = payload.get('content')
        fact_id = payload.get('fact_id')
        created_at = payload.get('created_at')
        if not isinstance(category, str) or category not in VALID_CATEGORIES:
            return None
        if not isinstance(content, str) or not content.strip():
            return None
        if not isinstance(fact_id, str) or not fact_id:
            return None
        if not isinstance(created_at, str) or not created_at:
            created_at = _now_iso()
        source = payload.get('source')
        if not isinstance(source, str) or source not in {'explicit', 'auto'}:
            source = 'explicit'
        return cls(
            fact_id=fact_id,
            category=category,
            content=content.strip(),
            created_at=created_at,
            source=source,
        )


class LongMemoryStore:
    def __init__(self, config: LongMemoryConfig) -> None:
        self.config = config
        self.path = self._resolve_path(config.path)
        self._facts: list[Fact] = []
        self._loaded = False

    def _resolve_path(self, raw_path: str) -> Path:
        candidate = Path(raw_path)
        if candidate.is_absolute():
            return candidate
        return Path.cwd() / candidate

    def _ensure_loaded(self) -> None:
        if self._loaded:
            return
        self._facts = self._read_facts()
        self._loaded = True

    def _read_facts(self) -> list[Fact]:
        if not self.path.exists():
            return []
        try:
            with self.path.open('r', encoding='utf-8') as file:
                payload = json.load(file)
        except Exception as exc:
            logger.warning("Failed to read long-term memory from %s: %s", self.path, exc)
            return []

        if not isinstance(payload, dict):
            return []

        raw_facts = payload.get('facts')
        if not isinstance(raw_facts, list):
            return []

        facts: list[Fact] = []
        for item in raw_facts:
            if not isinstance(item, dict):
                continue
            fact = Fact.from_dict(item)
            if fact is not None:
                facts.append(fact)
        return facts

    def _write_facts(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        payload = {
            'version': SCHEMA_VERSION,
            'facts': [fact.to_dict() for fact in self._facts[-self.config.max_facts:]],
        }
        with self.path.open('w', encoding='utf-8') as file:
            json.dump(payload, file, ensure_ascii=False, indent=2)
            file.write('\n')

    @property
    def enabled(self) -> bool:
        return self.config.enabled

    def all_facts(self) -> list[Fact]:
        self._ensure_loaded()
        return list(self._facts)

    def by_category(self, category: str) -> list[Fact]:
        normalized = category.strip().lower()
        return [fact for fact in self.all_facts() if fact.category == normalized]

    def add_fact(self, content: str, category: str = DEFAULT_CATEGORY, *, source: str = 'explicit') -> Fact | None:
        normalized_content = content.strip()
        if not normalized_content:
            return None

        normalized_category = category.strip().lower() or DEFAULT_CATEGORY
        if normalized_category not in VALID_CATEGORIES:
            normalized_category = DEFAULT_CATEGORY

        self._ensure_loaded()

        for existing in self._facts:
            if existing.category == normalized_category and _normalize(existing.content) == _normalize(normalized_content):
                return existing

        fact = Fact(
            fact_id=secrets.token_hex(6),
            category=normalized_category,
            content=normalized_content,
            created_at=_now_iso(),
            source=source if source in {'explicit', 'auto'} else 'explicit',
        )
        self._facts.append(fact)
        self._write_facts()
        return fact

    def remove_fact(self, fact_id: str) -> bool:
        self._ensure_loaded()
        before = len(self._facts)
        self._facts = [fact for fact in self._facts if fact.fact_id != fact_id]
        if len(self._facts) != before:
            self._write_facts()
            return True
        return False

    def remove_by_content(self, content: str) -> int:
        self._ensure_loaded()
        normalized = _normalize(content)
        if not normalized:
            return 0
        before = len(self._facts)
        self._facts = [fact for fact in self._facts if normalized not in _normalize(fact.content)]
        removed = before - len(self._facts)
        if removed > 0:
            self._write_facts()
        return removed

    def clear(self) -> int:
        self._ensure_loaded()
        before = len(self._facts)
        self._facts = []
        if before > 0:
            self._write_facts()
        return before

    def format_for_prompt(self) -> str:
        facts = self.all_facts()
        if not facts:
            return ''

        grouped: dict[str, list[Fact]] = {key: [] for key in VALID_CATEGORIES}
        for fact in facts:
            grouped.setdefault(fact.category, []).append(fact)

        lines: list[str] = ['Что ты помнишь между сессиями (долговременная память):']
        labels = {
            'user': 'О пользователе',
            'project': 'О текущих проектах',
            'preferences': 'Предпочтения и стиль работы',
            'notes': 'Заметки',
        }
        for category, label in labels.items():
            category_facts = grouped.get(category) or []
            if not category_facts:
                continue
            lines.append(f'- {label}:')
            for fact in category_facts:
                lines.append(f'  * {fact.content}')
        return '\n'.join(lines)


def _now_iso() -> str:
    return datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')


def _normalize(text: str) -> str:
    return ' '.join(text.lower().split())
