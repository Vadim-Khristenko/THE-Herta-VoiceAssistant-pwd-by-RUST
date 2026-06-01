from __future__ import annotations

import json
import logging
import os
import re
import shutil
import subprocess
import webbrowser
from datetime import datetime
from pathlib import Path
from urllib.parse import quote_plus

from actions.tool_layer import CallableTool, ToolCall, ToolParameter, ToolRegistry, ToolResult, ToolSpec
from config import SystemActionsConfig


URL_RE = re.compile(r'\b(?:https?://[^\s]+|www\.[^\s]+|[a-z0-9-]+\.[a-z]{2,}(?:/[^\s]*)?)', re.IGNORECASE)
INVALID_PATH_CHARS_RE = re.compile(r'[<>:"/\\|?*\x00-\x1f]')
DESTRUCTIVE_RE = re.compile(
    r'\b('
    r'удали|удалить|удаляй|сотри|стереть|стирай|очисти|снеси|уничтожь|'
    r'перемести|замени|перезапиши|форматируй|'
    r'delete|remove|erase|rm|rmdir|del|format|move|overwrite'
    r')\b',
    re.IGNORECASE,
)

OPEN_WORDS = ('открой', 'открыть', 'запусти', 'запустить')
CREATE_WORDS = ('создай', 'создать', 'сделай', 'заведи')
WRITE_WORDS = ('запиши', 'допиши', 'добавь', 'внеси', 'заполни', 'напиши')
RENAME_WORDS = ('переименуй', 'переименовать')
SEARCH_MARKERS = ('загугли', 'найди в интернете', 'найди в браузере', 'поиск в браузере')
REMEMBER_TRIGGERS = ('запомни', 'запомнить', 'сохрани в памяти', 'сохрани факт', 'отложи в память')
RECALL_TRIGGERS = (
    'что ты помнишь',
    'что ты обо мне помнишь',
    'что обо мне помнишь',
    'что ты знаешь обо мне',
    'что ты обо мне знаешь',
    'что у тебя в памяти',
    'покажи память',
    'перечисли факты',
    'покажи факты',
)
FORGET_TRIGGERS = ('забудь', 'удали из памяти', 'сотри из памяти', 'выкинь из памяти')
WEB_SEARCH_EXPLICIT_TRIGGERS = (
    'найди в интернете',
    'поищи в интернете',
    'погугли мне',
    'погугли',
    'найди мне',
    'поищи мне',
    'найди',
    'поищи',
)
WEB_SEARCH_NEWS_KEYWORDS = (
    'новости',
    'новость',
    'свежие новости',
    'последние новости',
    'что нового',
)
WEB_SEARCH_FACT_TRIGGERS = (
    'что такое',
    'кто такой',
    'кто такая',
    'кто такие',
    'когда выходит',
    'когда вышел',
    'когда вышла',
)
WEB_SEARCH_WEATHER_TRIGGERS = (
    'какая погода',
    'какая сейчас погода',
    'погода в',
    'погода на',
)
TYPE_CHECK_TRIGGERS = (
    'проверь типы',
    'проверь типизацию',
    'проверь файл',
    'проверь код',
    'mypy',
    'типы в',
)
LINT_TRIGGERS = (
    'линтуй',
    'линт',
    'ruff',
    'проверь стиль',
    'проверь линтером',
)
CODE_TARGET_PREPOSITIONS = ('в файле ', 'в модуле ', 'файл ', 'файла ', 'на файл ', 'на ', 'в ', 'для ')

SITE_ALIASES: dict[str, str] = {
    'ютуб': 'https://www.youtube.com',
    'youtube': 'https://www.youtube.com',
    'гугл': 'https://www.google.com',
    'google': 'https://www.google.com',
    'яндекс': 'https://www.yandex.ru',
    'yandex': 'https://www.yandex.ru',
    'вконтакте': 'https://vk.com',
    'вк': 'https://vk.com',
    'vk': 'https://vk.com',
    'гитхаб': 'https://github.com',
    'github': 'https://github.com',
    'телеграм': 'https://web.telegram.org',
    'telegram': 'https://web.telegram.org',
    'твиттер': 'https://twitter.com',
    'twitter': 'https://twitter.com',
    'почту': 'https://mail.google.com',
    'почта': 'https://mail.google.com',
    'gmail': 'https://mail.google.com',
    'reddit': 'https://www.reddit.com',
    'реддит': 'https://www.reddit.com',
    'stackoverflow': 'https://stackoverflow.com',
    'wikipedia': 'https://www.wikipedia.org',
    'вики': 'https://ru.wikipedia.org',
    'википедию': 'https://ru.wikipedia.org',
}
VAGUE_NAME_PATTERNS = (
    'как нибудь',
    'как-нибудь',
    'что нибудь',
    'что-нибудь',
    'нибудь',
    'как хочешь',
    'хочешь',
    'как угодно',
    'угодно',
    'любое',
    'любую',
    'любой',
    'сама придумай',
    'сам придумай',
    'придумай',
    'на свое усмотрение',
    'на своё усмотрение',
)


SystemActionResult = ToolResult


class SystemActionRunner:
    def __init__(
        self,
        config: SystemActionsConfig,
        logger: logging.Logger | None = None,
        *,
        extra_tools: list[CallableTool] | None = None,
    ) -> None:
        self.config = config
        self.logger = logger or logging.getLogger(__name__)
        self.root_dir = _resolve_managed_root(config.document_dir)
        self.registry_path = _resolve_registry_path(config.registry_path)
        self._extra_tools: list[CallableTool] = list(extra_tools or [])
        # Names of independent tools (web search, long-term memory, code tools). They have
        # their own enable flags and must not be gated by SYSTEM_ACTIONS_ENABLED.
        self._extra_tool_names: frozenset[str] = frozenset(tool.spec.name for tool in self._extra_tools)
        self.tools = self._build_tool_registry()

    def handle(self, user_text: str) -> SystemActionResult | None:
        normalized = _normalize(user_text)
        if not normalized:
            return None

        blocked_result = self.block_if_unsafe(user_text)
        if blocked_result is not None:
            return blocked_result

        tool_call = self._detect_tool_call(user_text, normalized)
        if tool_call is None:
            return None

        return self.execute_tool_call(tool_call)

    def block_if_unsafe(self, user_text: str) -> SystemActionResult | None:
        normalized = _normalize(user_text)
        if not normalized or not _is_destructive_request(normalized):
            return None

        return SystemActionResult(
            action_name='blocked_destructive_request',
            message='Нет. Удалять, перезаписывать, перемещать, очищать или форматировать файлы мне запрещено.',
            executed=False,
        )

    def execute_tool_call(self, tool_call: ToolCall) -> SystemActionResult:
        if tool_call.name not in {spec.name for spec in self.tool_specs()}:
            return SystemActionResult(
                action_name='unknown_tool',
                message=f"Неизвестный инструмент: {tool_call.name}",
                executed=False,
            )

        # Only native filesystem/system actions are gated by SYSTEM_ACTIONS_ENABLED.
        # Independent tools (web search, memory, code tools) run on their own flags.
        if not self.config.enabled and tool_call.name not in self._extra_tool_names:
            return SystemActionResult(
                action_name='system_actions_disabled',
                message="Системные действия отключены. Включи SYSTEM_ACTIONS_ENABLED='true' и запусти Герту заново.",
                executed=False,
            )

        try:
            return self.tools.run(tool_call)
        except Exception as exc:
            self.logger.warning("System action failed: %s", exc)
            return SystemActionResult(
                action_name='system_action_failed',
                message=f'Не вышло выполнить системное действие: {exc}',
                executed=False,
            )

    def tool_specs(self) -> list[ToolSpec]:
        return self.tools.specs

    def _build_tool_registry(self) -> ToolRegistry:
        return ToolRegistry(
            [
                CallableTool(
                    ToolSpec(
                        name='open_url',
                        description='Open the default browser with a safe HTTP/HTTPS URL.',
                        parameters=(
                            ToolParameter('url', 'string', 'HTTP or HTTPS URL to open.'),
                            ToolParameter('action_name', 'string', 'Result action name.', required=False),
                        ),
                    ),
                    lambda call: self._open_url(
                        str(call.arguments['url']),
                        str(call.arguments.get('action_name') or 'open_browser'),
                    ),
                ),
                CallableTool(
                    ToolSpec(
                        name='search_web',
                        description='Open the default browser with a Google search query.',
                        parameters=(ToolParameter('query', 'string', 'Search query to open in the browser.'),),
                    ),
                    lambda call: self._open_url(
                        f"https://www.google.com/search?q={quote_plus(str(call.arguments['query']))}",
                        'open_search',
                    ),
                ),
                CallableTool(
                    ToolSpec(
                        name='open_vscode',
                        description='Open VS Code through the configured command without shell access.',
                    ),
                    lambda _call: self._open_vscode(),
                ),
                CallableTool(
                    ToolSpec(
                        name='create_folder',
                        description=(
                            'Create a folder inside the managed system-actions root. '
                            'If the user asks you to choose a name, provide a short meaningful Russian folder_name.'
                        ),
                        parameters=(
                            ToolParameter('folder_name', 'string', 'Folder name. Omit only if no name is needed.', required=False),
                        ),
                    ),
                    lambda call: self._create_folder_fields(_optional_str(call.arguments.get('folder_name'))),
                ),
                CallableTool(
                    ToolSpec(
                        name='create_folder_with_document',
                        description=(
                            'Create a managed folder and a .txt document inside it. '
                            'Use this when the user asks for a folder plus a note/document/file in the same request.'
                        ),
                        parameters=(
                            ToolParameter('folder_name', 'string', 'Folder name. Choose a meaningful name if the user asks you to decide.', required=False),
                            ToolParameter('document_title', 'string', 'Text document title without path. Choose a useful title if needed.', required=False),
                            ToolParameter('content', 'string', 'Initial document text.', required=False),
                        ),
                    ),
                    lambda call: self._create_folder_with_document_fields(
                        _optional_str(call.arguments.get('folder_name')),
                        _optional_str(call.arguments.get('document_title')),
                        _optional_str(call.arguments.get('content')) or '',
                    ),
                ),
                CallableTool(
                    ToolSpec(
                        name='create_text_document',
                        description='Create a new .txt document in the managed folder without overwriting existing files.',
                        parameters=(
                            ToolParameter('document_title', 'string', 'Text document title without path. Choose a useful title if needed.', required=False),
                            ToolParameter('content', 'string', 'Initial document text.', required=False),
                            ToolParameter('folder_name', 'string', 'Existing or new managed folder name for the document.', required=False),
                        ),
                    ),
                    lambda call: self._create_text_document_fields(
                        _optional_str(call.arguments.get('document_title')),
                        _optional_str(call.arguments.get('content')) or '',
                        _optional_str(call.arguments.get('folder_name')),
                    ),
                ),
                CallableTool(
                    ToolSpec(
                        name='append_text_document',
                        description='Append text to a managed .txt document, creating it if needed.',
                        parameters=(
                            ToolParameter('content', 'string', 'Text to append.'),
                            ToolParameter('document_title', 'string', 'Target .txt document title without path.', required=False),
                            ToolParameter('folder_name', 'string', 'Managed folder name containing the document.', required=False),
                        ),
                    ),
                    lambda call: self._append_text_document_fields(
                        _optional_str(call.arguments.get('document_title')),
                        _optional_str(call.arguments.get('content')) or '',
                        _optional_str(call.arguments.get('folder_name')),
                    ),
                ),
                CallableTool(
                    ToolSpec(
                        name='rename_created_item',
                        description=(
                            'Rename only files or folders previously created by Herta and recorded in the registry. '
                            "Use kind='folder' for folders and kind='file' for text documents."
                        ),
                        parameters=(
                            ToolParameter('kind', 'string', "Either 'folder' or 'file'."),
                            ToolParameter('old_name', 'string', 'Current item name without path.'),
                            ToolParameter('new_name', 'string', 'New item name without path. Choose a meaningful name if the user asks you to decide.', required=False),
                        ),
                    ),
                    lambda call: self._rename_created_item_fields(
                        str(call.arguments['kind']),
                        str(call.arguments['old_name']),
                        _optional_str(call.arguments.get('new_name')),
                    ),
                ),
                *self._extra_tools,
            ]
        )

    def _detect_tool_call(self, original_text: str, normalized: str) -> ToolCall | None:
        memory_call = _detect_memory_call(original_text, normalized)
        if memory_call is not None:
            return memory_call

        code_call = _detect_code_check_call(original_text, normalized)
        if code_call is not None:
            return code_call

        search_call = _detect_web_search_call(original_text, normalized)
        if search_call is not None:
            return search_call

        if _has_any(normalized, OPEN_WORDS):
            url = _extract_url(original_text)
            if url is not None:
                return ToolCall('open_url', {'url': url})

            if 'браузер' in normalized:
                return ToolCall('open_url', {'url': self.config.browser_home_url})

            if _mentions_vscode(normalized):
                return ToolCall('open_vscode')

            shortcut_url = _site_alias_url(normalized)
            if shortcut_url is not None:
                return ToolCall('open_url', {'url': shortcut_url})

            search_target = _strip_leading_trigger(original_text, normalized, OPEN_WORDS)
            if search_target:
                cleaned = search_target.strip(' ,.;:!?\'"`').strip()
                if cleaned and not _mentions_folder(normalized) and not _mentions_text_document(normalized):
                    return ToolCall('search_web', {'query': cleaned})

        search_query = _extract_search_query(original_text, normalized)
        if search_query is not None:
            return ToolCall('search_web', {'query': search_query})

        if _has_any(normalized, RENAME_WORDS) and (_mentions_text_document(normalized) or _mentions_folder(normalized)):
            parsed = _extract_rename_request(original_text)
            if parsed is None:
                return ToolCall('rename_created_item', {'kind': 'file', 'old_name': '', 'new_name': ''})
            kind, old_name, new_name = parsed
            return ToolCall('rename_created_item', {'kind': kind, 'old_name': old_name, 'new_name': new_name})

        if _has_any(normalized, WRITE_WORDS) and (_mentions_text_document(normalized) or _mentions_folder(normalized)):
            return ToolCall(
                'append_text_document',
                {
                    'document_title': _extract_write_document_title(original_text) or _extract_document_title(original_text),
                    'content': _extract_write_document_content(original_text) or _extract_document_content(original_text),
                    'folder_name': _extract_folder_name(original_text),
                },
            )

        if _has_any(normalized, CREATE_WORDS) and _mentions_folder(normalized) and _mentions_text_document(normalized):
            return ToolCall(
                'create_folder_with_document',
                {
                    'folder_name': _extract_folder_name(original_text),
                    'document_title': _extract_document_title(original_text),
                    'content': _extract_document_content(original_text),
                },
            )

        if _has_any(normalized, CREATE_WORDS) and _mentions_folder(normalized):
            return ToolCall('create_folder', {'folder_name': _extract_folder_name(original_text)})

        if _has_any(normalized, CREATE_WORDS) and _mentions_text_document(normalized):
            return ToolCall(
                'create_text_document',
                {
                    'document_title': _extract_document_title(original_text),
                    'content': _extract_document_content(original_text),
                    'folder_name': _extract_folder_name(original_text),
                },
            )

        return None

    def _open_url(self, url: str, action_name: str = 'open_browser') -> SystemActionResult:
        safe_url = _normalize_url(url)
        webbrowser.open(safe_url, new=2)
        return SystemActionResult(
            action_name=action_name,
            message=f'Открываю: {safe_url}',
            executed=True,
        )

    def _open_vscode(self) -> SystemActionResult:
        command = _resolve_vscode_command(self.config.vscode_command)
        if command is None:
            return SystemActionResult(
                action_name='open_vscode',
                message="Не нашла команду VS Code. В VS Code включи команду 'code' в PATH или задай SYSTEM_ACTIONS_VSCODE_COMMAND.",
                executed=False,
            )

        # Always request a fresh window so the action is visible even when Herta is
        # launched from inside VS Code; otherwise `code <dir>` silently reuses the
        # already-open window and nothing appears.
        args = [command, '--new-window']
        if self.config.vscode_open_workspace:
            args.append(str(Path.cwd()))
        subprocess.Popen(args, shell=False)
        return SystemActionResult(
            action_name='open_vscode',
            message='Открываю VS Code.',
            executed=True,
        )

    def _create_folder_fields(self, folder_name: str | None) -> SystemActionResult:
        self.root_dir.mkdir(parents=True, exist_ok=True)
        target_path = _safe_folder_path(self.root_dir, folder_name or _generated_folder_name())

        existing = target_path.exists()
        if existing and not self._is_registered(target_path, 'folder'):
            return SystemActionResult(
                action_name='create_folder',
                message=f'Папка уже существует, но она не создана Гертой, поэтому я не буду ее трогать: {target_path}',
                executed=False,
            )

        target_path.mkdir(parents=True, exist_ok=True)
        self._register(target_path, 'folder')
        return SystemActionResult(
            action_name='create_folder',
            message=f"{'Папка уже была создана' if existing else 'Создана папка'}: {target_path}",
            executed=not existing,
            data={'path': str(target_path), 'name': target_path.name},
        )

    def _create_folder(self, user_text: str) -> SystemActionResult:
        return self._create_folder_fields(_extract_folder_name(user_text))

    def _create_folder_with_document_fields(
        self,
        folder_name: str | None,
        document_title: str | None,
        content: str,
    ) -> SystemActionResult:
        folder_result = self._create_folder_fields(folder_name)
        folder_path_raw = folder_result.data.get('path')
        if not folder_path_raw:
            return folder_result

        return self._create_text_document_fields(
            document_title,
            content,
            folder_name=None,
            parent_dir=Path(str(folder_path_raw)),
        )

    def _create_folder_with_document(self, user_text: str) -> SystemActionResult:
        folder_result = self._create_folder(user_text)
        folder_path_raw = folder_result.data.get('path')
        if not folder_path_raw:
            return folder_result

        return self._create_text_document(user_text, parent_dir=Path(str(folder_path_raw)))

    def _create_text_document_fields(
        self,
        document_title: str | None,
        content: str,
        folder_name: str | None = None,
        parent_dir: Path | None = None,
    ) -> SystemActionResult:
        documents_dir = parent_dir or self._resolve_registered_folder(folder_name)
        documents_dir.mkdir(parents=True, exist_ok=True)

        title = document_title or _generated_document_name()
        filename = _text_filename(title)

        target_path = _unique_child_path(documents_dir, filename)
        with target_path.open('x', encoding='utf-8') as file:
            if content:
                file.write(content)
                if not content.endswith('\n'):
                    file.write('\n')

        self._register(target_path, 'file')
        return SystemActionResult(
            action_name='create_text_document',
            message=f'Создан текстовый документ: {target_path}',
            executed=True,
            data={'path': str(target_path), 'name': target_path.name},
        )

    def _create_text_document(self, user_text: str, parent_dir: Path | None = None) -> SystemActionResult:
        return self._create_text_document_fields(
            _extract_document_title(user_text),
            _extract_document_content(user_text),
            _extract_folder_name(user_text),
            parent_dir=parent_dir,
        )

    def _append_text_document_fields(
        self,
        document_title: str | None,
        content: str,
        folder_name: str | None = None,
    ) -> SystemActionResult:
        documents_dir = self._resolve_registered_folder(folder_name)
        documents_dir.mkdir(parents=True, exist_ok=True)

        title = document_title or 'herta_note'
        filename = _text_filename(title)
        target_path = _safe_text_path(documents_dir, filename)
        if not content:
            return SystemActionResult(
                action_name='append_text_document',
                message='Не вижу текст, который нужно записать. Скажи: "допиши в документ план текст купить чай".',
                executed=False,
            )

        if target_path.exists() and not self._is_registered(target_path, 'file'):
            return SystemActionResult(
                action_name='append_text_document',
                message=f'Файл уже существует, но он не создан Гертой, поэтому я не буду его менять: {target_path}',
                executed=False,
            )

        created = not target_path.exists()
        with target_path.open('a', encoding='utf-8') as file:
            if not created and target_path.stat().st_size > 0:
                file.write('\n')
            file.write(content)
            if not content.endswith('\n'):
                file.write('\n')

        self._register(target_path, 'file')
        return SystemActionResult(
            action_name='append_text_document',
            message=f"{'Создан и заполнен' if created else 'Записано в'} текстовый документ: {target_path}",
            executed=True,
            data={'path': str(target_path), 'name': target_path.name, 'created': created},
        )

    def _append_text_document(self, user_text: str) -> SystemActionResult:
        return self._append_text_document_fields(
            _extract_write_document_title(user_text) or _extract_document_title(user_text),
            _extract_write_document_content(user_text) or _extract_document_content(user_text),
            _extract_folder_name(user_text),
        )

    def _rename_created_item_fields(
        self,
        kind: str,
        old_name: str,
        new_name: str | None,
    ) -> SystemActionResult:
        normalized_kind = kind.strip().lower()
        if normalized_kind not in {'folder', 'file'}:
            return SystemActionResult(
                action_name='rename_created_item',
                message="Тип для переименования должен быть 'folder' или 'file'.",
                executed=False,
            )

        meaningful_old_name = _meaningful_name_or_none(old_name)
        if meaningful_old_name is None:
            return SystemActionResult(
                action_name='rename_created_item',
                message='Не поняла, что именно переименовать.',
                executed=False,
            )

        meaningful_new_name = _meaningful_name_or_none(new_name or '')
        if meaningful_new_name is None:
            meaningful_new_name = _generated_folder_name() if normalized_kind == 'folder' else _generated_document_name()

        old_path = self._find_registered_by_name(meaningful_old_name, normalized_kind)
        if old_path is None:
            return SystemActionResult(
                action_name='rename_created_item',
                message='Я могу переименовывать только файлы и папки, которые создала сама.',
                executed=False,
            )

        new_path = (
            _safe_folder_path(old_path.parent, meaningful_new_name)
            if normalized_kind == 'folder'
            else _safe_text_path(old_path.parent, _text_filename(meaningful_new_name))
        )
        if new_path.exists():
            return SystemActionResult(
                action_name='rename_created_item',
                message=f'Новое имя уже занято, поэтому не переименовываю: {new_path}',
                executed=False,
            )

        old_path.rename(new_path)
        self._replace_registered_path(old_path, new_path, normalized_kind)
        return SystemActionResult(
            action_name='rename_created_item',
            message=f'Переименовано: {old_path.name} -> {new_path.name}',
            executed=True,
            data={'old_path': str(old_path), 'path': str(new_path), 'name': new_path.name},
        )

    def _rename_created_item(self, user_text: str) -> SystemActionResult:
        parsed = _extract_rename_request(user_text)
        if parsed is None:
            return SystemActionResult(
                action_name='rename_created_item',
                message='Не поняла, что во что переименовать. Пример: "переименуй документ план в задачи".',
                executed=False,
            )

        kind, old_name, new_name = parsed
        return self._rename_created_item_fields(kind, old_name, new_name)

    def _resolve_target_folder(self, user_text: str) -> Path:
        folder_name = _extract_folder_name(user_text)
        if folder_name is None:
            self.root_dir.mkdir(parents=True, exist_ok=True)
            return self.root_dir
        return self._resolve_registered_folder(folder_name)

    def _resolve_registered_folder(self, folder_name: str | None) -> Path:
        if folder_name is None:
            self.root_dir.mkdir(parents=True, exist_ok=True)
            return self.root_dir

        folder_path = _safe_folder_path(self.root_dir, folder_name)
        if folder_path.exists() and not self._is_registered(folder_path, 'folder'):
            raise RuntimeError(f'Папка существует, но она не создана Гертой: {folder_path}')

        folder_path.mkdir(parents=True, exist_ok=True)
        self._register(folder_path, 'folder')
        return folder_path

    def _registry_entries(self) -> list[dict[str, str]]:
        if not self.registry_path.exists():
            return []
        try:
            raw_data = json.loads(self.registry_path.read_text(encoding='utf-8'))
        except Exception as exc:
            self.logger.warning("Failed to read system actions registry: %s", exc)
            return []
        if not isinstance(raw_data, list):
            return []
        return [entry for entry in raw_data if isinstance(entry, dict)]

    def _save_registry_entries(self, entries: list[dict[str, str]]) -> None:
        self.registry_path.parent.mkdir(parents=True, exist_ok=True)
        self.registry_path.write_text(
            json.dumps(entries, ensure_ascii=False, indent=2),
            encoding='utf-8',
        )

    def _register(self, path: Path, kind: str) -> None:
        resolved = str(path.resolve())
        entries = [entry for entry in self._registry_entries() if entry.get('path') != resolved]
        entries.append(
            {
                'path': resolved,
                'kind': kind,
                'created_at': datetime.now().isoformat(timespec='seconds'),
            }
        )
        self._save_registry_entries(entries)

    def _replace_registered_path(self, old_path: Path, new_path: Path, kind: str) -> None:
        old_resolved = str(old_path.resolve())
        entries = self._registry_entries()
        replaced = False
        for entry in entries:
            if entry.get('path') == old_resolved and entry.get('kind') == kind:
                entry['path'] = str(new_path.resolve())
                entry['renamed_at'] = datetime.now().isoformat(timespec='seconds')
                replaced = True
                break
        if not replaced:
            entries.append(
                {
                    'path': str(new_path.resolve()),
                    'kind': kind,
                    'created_at': datetime.now().isoformat(timespec='seconds'),
                }
            )
        self._save_registry_entries(entries)

    def _is_registered(self, path: Path, kind: str | None = None) -> bool:
        resolved = str(path.resolve())
        return any(
            entry.get('path') == resolved and (kind is None or entry.get('kind') == kind)
            for entry in self._registry_entries()
        )

    def _find_registered_by_name(self, name: str, kind: str) -> Path | None:
        expected_name = _sanitize_folder_name(name) if kind == 'folder' else _text_filename(name)
        matches: list[Path] = []
        for entry in self._registry_entries():
            if entry.get('kind') != kind:
                continue
            path = Path(str(entry.get('path', ''))).resolve()
            if not path.exists():
                continue
            if path.name.lower() == expected_name.lower() or path.stem.lower() == Path(expected_name).stem.lower():
                matches.append(path)

        if len(matches) > 1:
            raise RuntimeError(f'Нашла несколько созданных объектов с именем {name!r}. Уточни имя.')
        return matches[0] if matches else None


def build_system_actions_instruction(*, structured_tools_available: bool = True) -> str:
    base = (
        'The local assistant has a limited safe OS action runner. It can open the default browser, open HTTP/HTTPS URLs, '
        'open web searches, open VS Code, create folders, create new .txt files, append text to .txt files, and rename only files/folders '
        'that it created itself. It cannot delete, move, overwrite files, format drives, or run arbitrary shell commands. '
        'For destructive requests, refuse briefly.'
    )
    if structured_tools_available:
        return base + (
            ' When structured tools are available, use them for supported OS action requests instead of only promising the action. '
            'Never write tool-call JSON inline in your reply; that is not how tools are invoked here.'
        )
    return base + (
        ' Structured tool calling is NOT available in this session. Do NOT output tool-call JSON, function-call wrappers, '
        'or pseudo-XML. The user\'s Russian phrasing ("открой ютуб", "создай папку Х", "запомни Y", "проверь типы в Z") is '
        'parsed locally and executed automatically. Just answer in character; the parser handles the action.'
    )


def _normalize(text: str) -> str:
    return ' '.join(text.strip().lower().split())


def _optional_str(value: object) -> str | None:
    if value is None:
        return None
    stripped = str(value).strip()
    return stripped or None


def _has_any(text: str, words: tuple[str, ...]) -> bool:
    return any(word in text for word in words)


def _is_destructive_request(normalized: str) -> bool:
    return DESTRUCTIVE_RE.search(normalized) is not None


VSCODE_MARKERS = (
    'vscode',
    'vs code',
    'visual studio code',
    'вскод',
    'вс код',
    'вс-код',
    'вэскод',
    'вэс код',
    'вискод',
    'вис код',
    'vs код',
    'визуал студио',
    'визуальная студия',
)


def _mentions_vscode(normalized: str) -> bool:
    return any(marker in normalized for marker in VSCODE_MARKERS)


TEXT_DOCUMENT_RE = re.compile(
    r'\b(?:'
    r'текстовый\s+документ'
    r'|документ(?:а|у|ом|е|ах|ам|ы|ов|и)?'
    r'|файл(?:а|у|ом|е|ах|ам|ы|ов|ик)?'
    r'|txt'
    r'|заметок'
    r'|заметк(?:а|у|и|е|ах|ам|ами|ой|ою)?'
    r')\b',
    re.IGNORECASE | re.UNICODE,
)

FOLDER_RE = re.compile(
    r'\b(?:'
    r'папок'
    r'|папк(?:а|у|е|и|ой|ою|ам|ах|ами)?'
    r'|каталог(?:а|у|ом|е|ов|и|ах|ам)?'
    r'|директори(?:я|ю|и|ей|ях|ям|ями)?'
    r')\b',
    re.IGNORECASE | re.UNICODE,
)


def _mentions_text_document(normalized: str) -> bool:
    return TEXT_DOCUMENT_RE.search(normalized) is not None


def _mentions_folder(normalized: str) -> bool:
    return FOLDER_RE.search(normalized) is not None


def _extract_url(text: str) -> str | None:
    match = URL_RE.search(text)
    if match is None:
        return None
    return match.group(0).rstrip('.,;:!?)]}')


def _normalize_url(url: str) -> str:
    stripped = url.strip()
    if stripped.lower().startswith(('http://', 'https://')):
        return stripped
    return f'https://{stripped}'


def _extract_search_query(original_text: str, normalized: str) -> str | None:
    for marker in SEARCH_MARKERS:
        marker_index = normalized.find(marker)
        if marker_index < 0:
            continue
        query_start = marker_index + len(marker)
        query = original_text[query_start:].strip(' .,:;-')
        return query or None
    return None


def _extract_folder_name(text: str) -> str | None:
    explicit_name = _extract_named_as(text)
    if explicit_name is not None:
        return explicit_name

    patterns = (
        r'(?:папк[ауе]|каталог|директори[юи])\s+(.+)$',
        r'(?:в|к)\s+(?:папк[еу]|каталог|директори[юи])\s+(.+)$',
    )
    for pattern in patterns:
        match = re.search(pattern, text, re.IGNORECASE)
        if match is None:
            continue
        name = _meaningful_name_or_none(_trim_folder_name_tail(match.group(1)))
        if name is not None:
            return name
    return None


def _trim_folder_name_tail(raw_name: str) -> str:
    name = raw_name.strip(' "\'.,:;-')
    name = re.sub(
        r'^(?:и\s+)?(?:назови|назвать)\s+(?:ее|её|его|их)?\s*(?:как|в|на)?\s+',
        '',
        name,
        flags=re.IGNORECASE,
    )
    name = re.split(
        r'\s+(?:'
        r'и\s+(?:создай\s+)?(?:документ|файл|заметк[ауи]?|текстовый\s+документ)|'
        r'с\s+(?:документом|файлом|заметк[оа]й|текстом)|'
        r'со\s+(?:документом|файлом|текстом)|'
        r'внутри|туда\s+(?:документ|файл|заметк[ауи]?|текст)'
        r')\b',
        name,
        maxsplit=1,
        flags=re.IGNORECASE,
    )[0]
    return name.strip(' "\'.,:;-')


def _extract_document_title(text: str) -> str | None:
    patterns = (
        r'(?:с названием|под названием|назови)\s+(.+?)(?:\s+с текстом|\s+и текстом|\s+текстом|\s+в папк|\s+в каталог|$)',
        r'(?:документ|файл|заметку)\s+(.+?)(?:\s+с текстом|\s+и текстом|\s+текстом|\s+текст|\s+в папк|\s+в каталог|$)',
    )
    for pattern in patterns:
        match = re.search(pattern, text, re.IGNORECASE)
        if match is None:
            continue
        title = _meaningful_name_or_none(match.group(1))
        if title and title.lower() not in {'текстовый', 'документ', 'файл', 'заметку'}:
            return title
    return None


def _extract_document_content(text: str) -> str:
    patterns = (
        r':\s*(.+)$',
        r'(?:с текстом|и текстом|текстом|текст|напиши туда)\s+(.+)$',
    )
    for pattern in patterns:
        match = re.search(pattern, text, re.IGNORECASE)
        if match is None:
            continue
        content = match.group(1).strip()
        if content:
            return content
    return ''


def _extract_write_document_title(text: str) -> str | None:
    patterns = (
        r'(?:в|к)\s+(?:текстовый\s+)?(?:документ|файл|заметку)\s+(.+?)(?:\s+текст|\s+с текстом|\s+текстом|\s*:|$)',
        r'([^\s"<>:/\\|?*]+\.txt)\b',
    )
    for pattern in patterns:
        match = re.search(pattern, text, re.IGNORECASE)
        if match is None:
            continue
        title = _meaningful_name_or_none(match.group(1))
        if title and title.lower() not in {'текстовый документ', 'документ', 'файл', 'заметку'}:
            return title
    return None


def _extract_write_document_content(text: str) -> str:
    patterns = (
        r':\s*(.+)$',
        r'(?:текст|с текстом|текстом|напиши туда)\s+(.+)$',
        r'(?:запиши|допиши|добавь|внеси|заполни|напиши)\s+(.+?)\s+(?:в|к)\s+(?:текстовый\s+)?(?:документ|файл|заметку)\b.*$',
        r'(?:запиши|допиши|добавь|внеси|заполни|напиши)\s+(.+)$',
    )
    for pattern in patterns:
        match = re.search(pattern, text, re.IGNORECASE)
        if match is None:
            continue
        content = match.group(1).strip(' "\'.,:;-')
        if content:
            return content
    return ''


def _extract_rename_request(text: str) -> tuple[str, str, str] | None:
    match = re.search(
        r'переименуй\s+(?:(папку|каталог|директорию|документ|файл|заметку)\s+)?(.+?)\s+(?:в|на)\s+(.+)$',
        text,
        re.IGNORECASE,
    )
    if match is None:
        return None

    raw_kind = (match.group(1) or '').lower()
    old_name = match.group(2).strip(' "\'.,:;-')
    new_name = match.group(3).strip(' "\'.,:;-')
    meaningful_old_name = _meaningful_name_or_none(old_name)
    if not meaningful_old_name:
        return None

    kind = 'folder' if raw_kind in {'папку', 'каталог', 'директорию'} else 'file'
    meaningful_new_name = _meaningful_name_or_none(new_name)
    if meaningful_new_name is None:
        meaningful_new_name = _generated_folder_name() if kind == 'folder' else _generated_document_name()
    return kind, meaningful_old_name, meaningful_new_name


def _text_filename(title: str) -> str:
    filename = _sanitize_folder_name(title)
    if not filename.lower().endswith('.txt'):
        filename = f'{filename}.txt'
    return filename


def _extract_named_as(text: str) -> str | None:
    match = re.search(
        r'(?:назови|назвать|имя|название)\s+(?:ее|её|его|их|папку|файл|документ|заметку)?\s*(?:как|в|на)?\s+(.+?)(?:\s+(?:с текстом|и текстом|текстом|документ|файл|папк|туда)|$)',
        text,
        re.IGNORECASE,
    )
    if match is None:
        return None
    return _meaningful_name_or_none(match.group(1))


def _meaningful_name_or_none(raw_name: str) -> str | None:
    name = raw_name.strip(' "\'.,:;-')
    normalized = _normalize(name.replace('-', ' '))
    if not normalized:
        return None
    if normalized in {'и', 'ее', 'её', 'его', 'их', 'назови', 'назвать', 'папку', 'файл', 'документ'}:
        return None
    if any(pattern in normalized for pattern in VAGUE_NAME_PATTERNS):
        return None
    name = re.sub(r'^(?:и\s+)?(?:назови|назвать)\s+(?:ее|её|его|их)?\s*(?:как|в|на)?\s+', '', name, flags=re.IGNORECASE)
    name = name.strip(' "\'.,:;-')
    return name or None


def _generated_folder_name() -> str:
    names = (
        'Рабочие материалы Герты',
        'Заметки для наблюдений',
        'Архив идей Герты',
        'Черновики Герты',
        'Полезные материалы',
    )
    now = datetime.now()
    return f'{names[now.second % len(names)]} {now:%Y-%m-%d %H-%M}'


def _generated_document_name() -> str:
    return datetime.now().strftime('Заметка Герты %Y-%m-%d %H-%M')


def _sanitize_folder_name(name: str) -> str:
    cleaned = INVALID_PATH_CHARS_RE.sub('_', name).strip(' ._')
    if not cleaned:
        return datetime.now().strftime('herta_item_%Y%m%d_%H%M%S')
    return cleaned[:80]


def _resolve_managed_root(document_dir: str) -> Path:
    if document_dir.strip().lower() in {'desktop', 'рабочий стол'}:
        return _resolve_desktop_dir()

    path = Path(document_dir).expanduser()
    if not path.is_absolute():
        path = Path.cwd() / path
    return path.resolve()


def _resolve_registry_path(registry_path: str) -> Path:
    path = Path(registry_path).expanduser()
    if not path.is_absolute():
        path = Path.cwd() / path
    return path.resolve()


def _resolve_desktop_dir() -> Path:
    candidates = []
    user_profile = os.getenv('USERPROFILE')
    if user_profile:
        candidates.append(Path(user_profile) / 'Desktop')

    # On Linux the desktop folder may be localized (e.g. "Рабочий стол"); ask the
    # XDG helper before falling back to the conventional ~/Desktop name.
    xdg_desktop = _xdg_desktop_dir()
    if xdg_desktop is not None:
        candidates.append(xdg_desktop)

    candidates.append(Path.home() / 'Desktop')

    for candidate in candidates:
        if candidate.exists():
            return candidate.resolve()
    return candidates[0].resolve()


def _xdg_desktop_dir() -> Path | None:
    helper = shutil.which('xdg-user-dir')
    if helper is None:
        return None
    try:
        completed = subprocess.run(
            [helper, 'DESKTOP'],
            check=True,
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (subprocess.SubprocessError, OSError):
        return None
    path_text = completed.stdout.strip()
    if not path_text:
        return None
    return Path(path_text)


def _unique_child_path(parent: Path, filename: str) -> Path:
    base_path = _safe_text_path(parent, filename)
    if not base_path.exists():
        return base_path

    stem = base_path.stem
    suffix = base_path.suffix
    for index in range(2, 1000):
        candidate = _safe_text_path(parent, f'{stem}_{index}{suffix}')
        if not candidate.exists():
            return candidate
    raise RuntimeError('Could not create a unique text document name.')


def _safe_text_path(parent: Path, filename: str) -> Path:
    parent_path = parent.resolve()
    child_path = (parent_path / filename).resolve()
    if child_path.parent != parent_path:
        raise ValueError('Unsafe document path.')
    if child_path.suffix.lower() != '.txt':
        raise ValueError('Only .txt documents are allowed.')
    return child_path


def _safe_folder_path(parent: Path, folder_name: str) -> Path:
    parent_path = parent.resolve()
    child_path = (parent_path / _sanitize_folder_name(folder_name)).resolve()
    if child_path.parent != parent_path:
        raise ValueError('Unsafe folder path.')
    return child_path


def _resolve_vscode_command(command: str) -> str | None:
    configured = command.strip()
    if configured:
        configured_path = Path(configured)
        if configured_path.is_absolute() and configured_path.exists():
            return str(configured_path)

        found = shutil.which(configured)
        if found is not None:
            return found

    return shutil.which('code') or shutil.which('code.cmd')


def _detect_memory_call(original_text: str, normalized: str) -> ToolCall | None:
    if _has_any(normalized, RECALL_TRIGGERS):
        return ToolCall('recall', {})

    forget_payload = _strip_leading_trigger(original_text, normalized, FORGET_TRIGGERS)
    if forget_payload is not None:
        cleaned = _strip_memory_modifiers(forget_payload).strip(' ,.;:!?-"\'')
        if cleaned:
            return ToolCall('forget', {'content_match': cleaned})
        return ToolCall('forget', {'content_match': ''})

    remember_payload = _strip_leading_trigger(original_text, normalized, REMEMBER_TRIGGERS)
    if remember_payload is not None:
        cleaned = _strip_memory_modifiers(remember_payload).strip(' ,.;:!?-"\'')
        if cleaned:
            return ToolCall(
                'remember',
                {'content': cleaned, 'category': _guess_memory_category(cleaned)},
            )

    return None


def _strip_leading_trigger(original_text: str, normalized: str, triggers: tuple[str, ...]) -> str | None:
    if not _has_any(normalized, triggers):
        return None
    for trigger in sorted(triggers, key=len, reverse=True):
        pattern = re.compile(r'\b' + re.escape(trigger) + r'\b', re.IGNORECASE)
        match = pattern.search(original_text)
        if match is not None:
            return original_text[match.end():].lstrip()
    return ''


def _strip_trigger_anywhere(original_text: str, normalized: str, triggers: tuple[str, ...]) -> str | None:
    # Like _strip_leading_trigger, but the trigger may stand anywhere in the phrase.
    # The (longest) matched trigger is cut out and the surrounding words become the
    # query, so both "погугли X" and "X, погугли" yield query "X". Returns None when
    # no trigger is present, or '' when the phrase is only the trigger with no query.
    if not _has_any(normalized, triggers):
        return None
    for trigger in sorted(triggers, key=len, reverse=True):
        pattern = re.compile(r'\b' + re.escape(trigger) + r'\b', re.IGNORECASE)
        if pattern.search(original_text) is not None:
            remainder = pattern.sub(' ', original_text)
            return re.sub(r'\s+', ' ', remainder).strip()
    return ''


def _strip_memory_modifiers(text: str) -> str:
    lowered = text.lower().strip()
    for prefix in (
        'что ',
        'про то что ',
        'про то, что ',
        'о том что ',
        'о том, что ',
        ', что ',
        ': ',
        '- ',
    ):
        if lowered.startswith(prefix):
            return text[len(prefix):].strip()
    return text


def _detect_web_search_call(original_text: str, normalized: str) -> ToolCall | None:
    # Реплика, которая явно про локальные файлы или папки — не наш кейс.
    has_filesystem_context = _mentions_text_document(normalized) or _mentions_folder(normalized)

    explicit_payload = _strip_trigger_anywhere(original_text, normalized, WEB_SEARCH_EXPLICIT_TRIGGERS)
    if explicit_payload is not None and not has_filesystem_context:
        cleaned = explicit_payload.strip(' ,.;:!?\'"`').strip()
        if cleaned:
            return ToolCall('web_search', {'query': cleaned})

    weather_query = _build_search_query(original_text, normalized, WEB_SEARCH_WEATHER_TRIGGERS, prefix='погода')
    if weather_query is not None:
        return ToolCall('web_search', {'query': weather_query})

    if _has_any(normalized, WEB_SEARCH_NEWS_KEYWORDS) and not has_filesystem_context:
        query = original_text.strip(' ,.;:!?\'"`').strip()
        if query:
            return ToolCall('web_search', {'query': query})

    if _has_any(normalized, WEB_SEARCH_FACT_TRIGGERS):
        cleaned = original_text.strip(' ,.;:!?\'"`').strip()
        if cleaned:
            return ToolCall('web_search', {'query': cleaned})

    return None


def _build_search_query(
    original_text: str,
    normalized: str,
    triggers: tuple[str, ...],
    *,
    prefix: str | None,
) -> str | None:
    payload = _strip_trigger_anywhere(original_text, normalized, triggers)
    if payload is None:
        return None

    cleaned = payload.strip(' ,.;:!?\'"`').strip()
    if not cleaned:
        # 'какая погода' без уточнения локации - оставляем как есть, поиск выдаст по геолокации
        return prefix
    if prefix and prefix not in cleaned.lower():
        return f'{prefix} {cleaned}'
    return cleaned


def _site_alias_url(normalized: str) -> str | None:
    tokens = re.findall(r'[\w-]+', normalized, flags=re.UNICODE)
    for token in tokens:
        url = SITE_ALIASES.get(token.lower())
        if url is not None:
            return url
    return None


def _detect_code_check_call(original_text: str, normalized: str) -> ToolCall | None:
    if _has_any(normalized, LINT_TRIGGERS):
        target = _extract_code_target(original_text, normalized, LINT_TRIGGERS)
        if target:
            return ToolCall('lint_code', {'target': target})

    if _has_any(normalized, TYPE_CHECK_TRIGGERS):
        target = _extract_code_target(original_text, normalized, TYPE_CHECK_TRIGGERS)
        if target:
            return ToolCall('type_check', {'target': target})

    return None


def _extract_code_target(original_text: str, normalized: str, triggers: tuple[str, ...]) -> str | None:
    payload = _strip_leading_trigger(original_text, normalized, triggers)
    if payload is None:
        return None

    cleaned = payload.strip(' ,.;:!?\'"`')
    if not cleaned:
        return None

    lowered = cleaned.lower()
    for preposition in CODE_TARGET_PREPOSITIONS:
        if lowered.startswith(preposition):
            cleaned = cleaned[len(preposition):].strip()
            break

    candidate = cleaned.split()[0] if cleaned else ''
    candidate = candidate.strip(' ,.;:!?\'"`')
    return candidate or None


def _guess_memory_category(content: str) -> str:
    lowered = content.lower()
    if any(marker in lowered for marker in ('меня зовут', 'мне ', 'я живу', 'я работаю', 'мой возраст', 'я родил', 'я учу')):
        return 'user'
    if any(marker in lowered for marker in ('проект', 'репозитор', 'кодовая база', 'модуль', 'фича', 'релиз')):
        return 'project'
    if any(marker in lowered for marker in ('предпочит', 'люблю', 'не люблю', 'стиль', 'привычка', 'формат')):
        return 'preferences'
    return 'notes'
