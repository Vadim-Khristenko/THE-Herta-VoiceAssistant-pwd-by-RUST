from __future__ import annotations

import logging
import re
from dataclasses import dataclass
from typing import TYPE_CHECKING

from actions.code_tools import CodeToolProvider
from config import CodeToolsConfig


if TYPE_CHECKING:
    from main import ChatClient


logger = logging.getLogger(__name__)

CODE_BLOCK_RE = re.compile(
    r'```(?P<lang>[a-zA-Z0-9_+\-]*)\s*\n(?P<body>.*?)```',
    re.DOTALL,
)
PYTHON_LANGS = frozenset({'python', 'py', 'python3', ''})

REPAIR_TEMPLATE = (
    'Я прогнала твой код через mypy и ruff. Нашлись замечания. '
    'Перепиши ответ так, чтобы код прошёл обе проверки. '
    'Сохрани персону, краткость и структуру; не добавляй извинений. '
    'Замечания:\n\n{issues}\n\nЕсли часть замечаний несущественна или ложная — обоснуй в одну строку.'
)


@dataclass(frozen=True, slots=True)
class SnippetIssue:
    snippet: str
    report: str


def extract_python_blocks(text: str) -> list[str]:
    blocks: list[str] = []
    for match in CODE_BLOCK_RE.finditer(text):
        lang = match.group('lang').strip().lower()
        if lang not in PYTHON_LANGS:
            continue
        body = match.group('body').strip()
        if body:
            blocks.append(body)
    return blocks


def collect_snippet_issues(
    blocks: list[str],
    provider: CodeToolProvider,
) -> list[SnippetIssue]:
    issues: list[SnippetIssue] = []
    for snippet in blocks:
        outcomes = provider.check_snippet(snippet)
        reports: list[str] = []
        for outcome in outcomes:
            if outcome.returncode == 0:
                continue
            cleaned = outcome.output.strip()
            if not cleaned:
                continue
            reports.append(f'### {outcome.tool}\n{cleaned}')
        if reports:
            issues.append(SnippetIssue(snippet=snippet, report='\n\n'.join(reports)))
    return issues


def maybe_self_check_and_repair(
    *,
    reply: str,
    messages: list[dict[str, str]],
    chat_client: 'ChatClient',
    config: CodeToolsConfig,
    provider: CodeToolProvider | None,
) -> str:
    if provider is None or not config.self_check_enabled:
        return reply

    blocks = extract_python_blocks(reply)
    if not blocks:
        return reply

    eligible = [block for block in blocks if len(block.splitlines()) >= config.self_check_min_lines]
    if not eligible:
        return reply

    eligible = eligible[: config.self_check_max_snippets]

    issues = collect_snippet_issues(eligible, provider)
    if not issues:
        logger.info('Self-check passed: %d snippet(s) clean.', len(eligible))
        return reply

    feedback = _build_repair_feedback(issues)
    logger.info('Self-check found issues in %d/%d snippet(s); requesting repair.', len(issues), len(eligible))

    repair_messages = list(messages) + [
        {'role': 'assistant', 'content': reply},
        {'role': 'user', 'content': REPAIR_TEMPLATE.format(issues=feedback)},
    ]

    try:
        repaired = chat_client.chat(repair_messages)
    except Exception as exc:
        logger.warning('Self-check repair LLM call failed: %s', exc)
        return reply

    if not repaired:
        return reply

    return repaired


def _build_repair_feedback(issues: list[SnippetIssue]) -> str:
    parts: list[str] = []
    for index, issue in enumerate(issues, start=1):
        parts.append(f'## Сниппет {index}\n```python\n{issue.snippet}\n```\n{issue.report}')
    return '\n\n'.join(parts)
