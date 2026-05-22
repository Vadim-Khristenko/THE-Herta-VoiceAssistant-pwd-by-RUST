from __future__ import annotations

import logging
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path



from actions.tool_layer import CallableTool, ToolCall, ToolParameter, ToolResult, ToolSpec
from config import CodeToolsConfig


logger = logging.getLogger(__name__)

MAX_OUTPUT_CHARS = 4000


@dataclass(frozen=True, slots=True)
class CheckOutcome:
    tool: str
    returncode: int
    output: str
    timed_out: bool


class CodeToolProvider:
    """Exposes mypy/ruff as safe, read-only CallableTool instances."""

    def __init__(self, config: CodeToolsConfig) -> None:
        self.config = config
        self.project_root = Path(config.project_root).resolve()

    def callable_tools(self) -> list[CallableTool]:
        if not self.config.enabled:
            return []

        return [
            CallableTool(
                ToolSpec(
                    name='type_check',
                    description=(
                        'Run mypy static type checker on a Python file or module path '
                        'inside the project. Read-only; no files are modified. '
                        'Use when the user asks to check types, verify annotations, '
                        'or hunt typing issues in code that already exists on disk.'
                    ),
                    parameters=(
                        ToolParameter('target', 'string', 'Path relative to project root (file or directory).'),
                    ),
                ),
                lambda call: self._run_mypy_on_path(str(call.arguments.get('target') or '').strip()),
            ),
            CallableTool(
                ToolSpec(
                    name='lint_code',
                    description=(
                        'Run ruff linter on a Python file or directory inside the project. '
                        'Read-only; reports style/quality issues without changing files.'
                    ),
                    parameters=(
                        ToolParameter('target', 'string', 'Path relative to project root (file or directory).'),
                    ),
                ),
                lambda call: self._run_ruff_on_path(str(call.arguments.get('target') or '').strip()),
            ),
        ]

    def _safe_target(self, raw_target: str) -> Path | None:
        if not raw_target:
            return None
        candidate = (self.project_root / raw_target).resolve()
        try:
            candidate.relative_to(self.project_root)
        except ValueError:
            return None
        return candidate

    def _run_mypy_on_path(self, target: str) -> ToolResult:
        path = self._safe_target(target)
        if path is None:
            return ToolResult(
                action_name='type_check',
                message=f'Цель {target!r} вне корня проекта или пустая.',
                executed=False,
            )
        if not path.exists():
            return ToolResult(
                action_name='type_check',
                message=f'Файл/каталог не найден: {target!r}.',
                executed=False,
            )

        outcome = self.run_mypy(str(path))
        if outcome.timed_out:
            return ToolResult(
                action_name='type_check',
                message=f'mypy не завершился за {self.config.timeout_seconds}s на {target!r}.',
                executed=False,
            )

        verdict = 'Чисто, претензий нет.' if outcome.returncode == 0 else 'mypy нашёл проблемы.'
        message = f'{verdict}\n\n{outcome.output.strip() or "(вывод пуст)"}'
        return ToolResult(
            action_name='type_check',
            message=_truncate(message, MAX_OUTPUT_CHARS),
            executed=True,
            data={'returncode': outcome.returncode, 'tool': 'mypy', 'target': target},
        )

    def _run_ruff_on_path(self, target: str) -> ToolResult:
        path = self._safe_target(target)
        if path is None:
            return ToolResult(
                action_name='lint_code',
                message=f'Цель {target!r} вне корня проекта или пустая.',
                executed=False,
            )
        if not path.exists():
            return ToolResult(
                action_name='lint_code',
                message=f'Файл/каталог не найден: {target!r}.',
                executed=False,
            )

        outcome = self.run_ruff(str(path))
        if outcome.timed_out:
            return ToolResult(
                action_name='lint_code',
                message=f'ruff не завершился за {self.config.timeout_seconds}s на {target!r}.',
                executed=False,
            )

        verdict = 'Без замечаний.' if outcome.returncode == 0 else 'ruff нашёл замечания.'
        message = f'{verdict}\n\n{outcome.output.strip() or "(вывод пуст)"}'
        return ToolResult(
            action_name='lint_code',
            message=_truncate(message, MAX_OUTPUT_CHARS),
            executed=True,
            data={'returncode': outcome.returncode, 'tool': 'ruff', 'target': target},
        )

    def run_mypy(self, target: str) -> CheckOutcome:
        args = [sys.executable, '-m', 'mypy', *self.config.mypy_args, target]
        return _run_subprocess(args, self.config.timeout_seconds, tool='mypy')

    def run_ruff(self, target: str) -> CheckOutcome:
        args = [sys.executable, '-m', 'ruff', *self.config.ruff_args, target]
        return _run_subprocess(args, self.config.timeout_seconds, tool='ruff')

    def check_snippet(self, source: str) -> list[CheckOutcome]:
        """Write the snippet to a temp .py file and run mypy + ruff on it."""
        snippet = source.strip()
        if not snippet:
            return []

        results: list[CheckOutcome] = []
        with tempfile.NamedTemporaryFile(
            mode='w',
            suffix='.py',
            prefix='herta_snippet_',
            delete=False,
            encoding='utf-8',
        ) as tmp:
            tmp.write(snippet)
            if not snippet.endswith('\n'):
                tmp.write('\n')
            tmp_path = Path(tmp.name)

        try:
            results.append(self.run_mypy(str(tmp_path)))
            results.append(self.run_ruff(str(tmp_path)))
        finally:
            try:
                tmp_path.unlink()
            except OSError:
                pass

        return results


def _run_subprocess(args: list[str], timeout_seconds: int, *, tool: str) -> CheckOutcome:
    try:
        proc = subprocess.run(
            args,
            capture_output=True,
            text=True,
            encoding='utf-8',
            errors='replace',
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return CheckOutcome(tool=tool, returncode=-1, output='', timed_out=True)
    except FileNotFoundError as exc:
        return CheckOutcome(
            tool=tool,
            returncode=-1,
            output=f'{tool} не запустился: {exc}',
            timed_out=False,
        )

    output_parts = []
    if proc.stdout:
        output_parts.append(proc.stdout)
    if proc.stderr:
        output_parts.append(proc.stderr)
    combined = '\n'.join(part.strip() for part in output_parts if part.strip())
    return CheckOutcome(tool=tool, returncode=proc.returncode, output=combined, timed_out=False)


def _truncate(text: str, limit: int) -> str:
    if len(text) <= limit:
        return text
    return text[:limit].rstrip() + f'\n\n…(обрезано до {limit} символов)'
