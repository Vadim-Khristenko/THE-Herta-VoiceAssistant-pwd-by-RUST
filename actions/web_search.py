from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Any

import httpx

from actions.tool_layer import CallableTool, ToolCall, ToolParameter, ToolResult, ToolSpec
from config import WebSearchConfig


logger = logging.getLogger(__name__)

TAVILY_ENDPOINT = 'https://api.tavily.com/search'
MAX_QUERY_CHARS = 400
MAX_SNIPPET_CHARS = 600


@dataclass(frozen=True, slots=True)
class WebSearchResult:
    title: str
    url: str
    snippet: str
    score: float


@dataclass(frozen=True, slots=True)
class WebSearchResponse:
    query: str
    answer: str
    results: tuple[WebSearchResult, ...]

    def format_for_prompt(self, limit: int = 5) -> str:
        lines: list[str] = []
        if self.answer:
            lines.append(f'Сводка: {self.answer.strip()}')
            lines.append('')
        for index, result in enumerate(self.results[:limit], start=1):
            lines.append(f'[{index}] {result.title}')
            lines.append(f'    {result.url}')
            if result.snippet:
                lines.append(f'    {result.snippet}')
        return '\n'.join(lines).strip()

    def format_short(self, limit: int = 3) -> str:
        if self.answer:
            return self.answer.strip()
        parts: list[str] = []
        for result in self.results[:limit]:
            parts.append(f'{result.title} — {result.url}')
        return '\n'.join(parts)


class TavilyClient:
    def __init__(self, config: WebSearchConfig) -> None:
        self.config = config
        self._client = httpx.Client(timeout=config.timeout_seconds)

    def search(self, query: str) -> WebSearchResponse:
        if not self.config.api_key:
            raise RuntimeError('TAVILY_API_KEY is not configured.')

        trimmed_query = query.strip()[:MAX_QUERY_CHARS]
        if not trimmed_query:
            raise ValueError('Empty search query.')

        payload: dict[str, Any] = {
            'api_key': self.config.api_key,
            'query': trimmed_query,
            'search_depth': self.config.search_depth,
            'max_results': self.config.max_results,
            'include_answer': True,
            'include_raw_content': False,
            'include_images': False,
        }

        response = self._client.post(TAVILY_ENDPOINT, json=payload)
        response.raise_for_status()
        data = response.json()

        results: list[WebSearchResult] = []
        for item in data.get('results', []):
            if not isinstance(item, dict):
                continue
            snippet = (item.get('content') or '').strip()
            if len(snippet) > MAX_SNIPPET_CHARS:
                snippet = snippet[:MAX_SNIPPET_CHARS].rstrip() + '…'
            results.append(
                WebSearchResult(
                    title=str(item.get('title') or '').strip(),
                    url=str(item.get('url') or '').strip(),
                    snippet=snippet,
                    score=float(item.get('score') or 0.0),
                )
            )

        return WebSearchResponse(
            query=trimmed_query,
            answer=str(data.get('answer') or '').strip(),
            results=tuple(results),
        )

    def close(self) -> None:
        try:
            self._client.close()
        except Exception:  # pragma: no cover
            pass


class WebSearchProvider:
    """Exposes web search as a CallableTool. The local parser (or structured tool layer)
    invokes `web_search(query=...)` and the runner returns a ToolResult flagged with
    `needs_followup` so the caller can ask the LLM to paraphrase in character.
    """

    def __init__(self, config: WebSearchConfig) -> None:
        self.config = config
        self._client: TavilyClient | None = None

    @property
    def enabled(self) -> bool:
        return self.config.enabled and bool(self.config.api_key)

    def _get_client(self) -> TavilyClient:
        if self._client is None:
            self._client = TavilyClient(self.config)
        return self._client

    def callable_tools(self) -> list[CallableTool]:
        if not self.enabled:
            return []

        return [
            CallableTool(
                ToolSpec(
                    name='web_search',
                    description=(
                        'Search the web with Tavily for fresh facts, news, weather, definitions, '
                        'or any external information. Returns a short answer plus top results. '
                        'Use whenever the user asks something that needs current/external data, '
                        'or explicitly says "найди в интернете", "погугли", "что такое X".'
                    ),
                    parameters=(
                        ToolParameter('query', 'string', 'The search query in natural language.'),
                    ),
                ),
                lambda call: self._search(str(call.arguments.get('query') or '').strip()),
            ),
        ]

    def _search(self, query: str) -> ToolResult:
        if not query:
            return ToolResult(
                action_name='web_search',
                message='Пустой поисковый запрос.',
                executed=False,
            )

        try:
            response = self._get_client().search(query)
        except httpx.HTTPStatusError as exc:
            return ToolResult(
                action_name='web_search',
                message=f'Tavily вернула HTTP {exc.response.status_code}.',
                executed=False,
                data={'error': str(exc)},
            )
        except (httpx.HTTPError, RuntimeError, ValueError) as exc:
            return ToolResult(
                action_name='web_search',
                message=f'Поиск не удался: {exc}',
                executed=False,
                data={'error': str(exc)},
            )

        if not response.results and not response.answer:
            return ToolResult(
                action_name='web_search',
                message=f'По запросу {query!r} ничего не нашлось.',
                executed=True,
                data={'query': query, 'needs_followup': False, 'results': []},
            )

        return ToolResult(
            action_name='web_search',
            message=response.format_short(),
            executed=True,
            data={
                'query': query,
                'needs_followup': True,
                'prompt_block': response.format_for_prompt(),
                'answer': response.answer,
                'results': [
                    {'title': r.title, 'url': r.url, 'snippet': r.snippet, 'score': r.score}
                    for r in response.results
                ],
            },
        )

    def close(self) -> None:
        if self._client is not None:
            self._client.close()
            self._client = None
