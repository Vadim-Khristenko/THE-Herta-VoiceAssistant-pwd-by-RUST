from __future__ import annotations

import logging

from actions.tool_layer import CallableTool, ToolCall, ToolParameter, ToolResult, ToolSpec
from brain.long_memory import DEFAULT_CATEGORY, VALID_CATEGORIES, LongMemoryStore


logger = logging.getLogger(__name__)


class MemoryToolProvider:
    """Exposes long-term memory operations as CallableTool instances for the registry."""

    def __init__(self, store: LongMemoryStore) -> None:
        self.store = store

    def callable_tools(self) -> list[CallableTool]:
        if not self.store.enabled:
            return []

        return [
            CallableTool(
                ToolSpec(
                    name='remember',
                    description=(
                        'Persist a useful fact about the user, the project, their preferences, '
                        'or a free-form note across sessions. Call this when the user explicitly '
                        'asks you to remember something ("запомни", "сохрани", "запиши") OR when '
                        'you observe a stable, non-trivial fact worth keeping.'
                    ),
                    parameters=(
                        ToolParameter('content', 'string', 'The fact to remember, rephrased concisely.'),
                        ToolParameter(
                            'category',
                            'string',
                            f'One of: {", ".join(sorted(VALID_CATEGORIES))}. Default: {DEFAULT_CATEGORY}.',
                            required=False,
                        ),
                    ),
                ),
                lambda call: self._remember(
                    str(call.arguments.get('content') or '').strip(),
                    str(call.arguments.get('category') or DEFAULT_CATEGORY).strip().lower(),
                ),
            ),
            CallableTool(
                ToolSpec(
                    name='recall',
                    description=(
                        'List facts the assistant remembers about the user/project. '
                        'Optionally filter by category. Use when the user asks what you remember.'
                    ),
                    parameters=(
                        ToolParameter(
                            'category',
                            'string',
                            f'Optional filter: {", ".join(sorted(VALID_CATEGORIES))}. Omit to list everything.',
                            required=False,
                        ),
                    ),
                ),
                lambda call: self._recall(str(call.arguments.get('category') or '').strip().lower()),
            ),
            CallableTool(
                ToolSpec(
                    name='forget',
                    description=(
                        'Remove a previously remembered fact by substring match. '
                        'Use when the user asks to forget or correct something.'
                    ),
                    parameters=(
                        ToolParameter('content_match', 'string', 'Substring of the fact to forget.'),
                    ),
                ),
                lambda call: self._forget(str(call.arguments.get('content_match') or '').strip()),
            ),
        ]

    def _remember(self, content: str, category: str) -> ToolResult:
        if not content:
            return ToolResult(
                action_name='remember',
                message='Нечего запоминать: пустой текст.',
                executed=False,
            )

        fact = self.store.add_fact(content, category, source='explicit')
        if fact is None:
            return ToolResult(
                action_name='remember',
                message='Не удалось сохранить факт.',
                executed=False,
            )

        return ToolResult(
            action_name='remember',
            message=f'Запомнила в категории «{fact.category}»: {fact.content}',
            executed=True,
            data={'fact_id': fact.fact_id, 'category': fact.category, 'content': fact.content},
        )

    def _recall(self, category: str) -> ToolResult:
        if category and category not in VALID_CATEGORIES:
            return ToolResult(
                action_name='recall',
                message=f'Неизвестная категория: {category!r}.',
                executed=False,
            )

        facts = self.store.by_category(category) if category else self.store.all_facts()
        if not facts:
            scope = f'в категории «{category}»' if category else 'между сессиями'
            return ToolResult(
                action_name='recall',
                message=f'Пока ничего не помню {scope}.',
                executed=True,
                data={'facts': []},
            )

        lines = [f'- [{fact.category}] {fact.content}' for fact in facts]
        return ToolResult(
            action_name='recall',
            message='Помню следующее:\n' + '\n'.join(lines),
            executed=True,
            data={'facts': [fact.to_dict() for fact in facts]},
        )

    def _forget(self, content_match: str) -> ToolResult:
        if not content_match:
            return ToolResult(
                action_name='forget',
                message='Нужно указать, что забыть.',
                executed=False,
            )

        removed = self.store.remove_by_content(content_match)
        if removed == 0:
            return ToolResult(
                action_name='forget',
                message=f'Ничего не нашла, совпадающего с {content_match!r}.',
                executed=True,
                data={'removed': 0},
            )
        return ToolResult(
            action_name='forget',
            message=f'Забыла {removed} факт(ов), совпадающих с {content_match!r}.',
            executed=True,
            data={'removed': removed},
        )
