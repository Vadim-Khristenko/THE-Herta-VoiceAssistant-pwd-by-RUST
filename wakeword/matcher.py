import re
from typing import Final


WAKE_WORD_BOUNDARY_CHARS: Final[str] = ' ,.;:!?…—-—"\'`«»()[]'
NON_WORD_CHARS_RE: Final[re.Pattern[str]] = re.compile(r'[^\w\s]+', re.UNICODE)
WHITESPACE_RE: Final[re.Pattern[str]] = re.compile(r'\s+')


def _normalize_for_match(text: str) -> str:
    """Lowercase and collapse punctuation to spaces so 'Эй, герта!' matches 'эй герта'."""
    text = NON_WORD_CHARS_RE.sub(' ', text)
    text = WHITESPACE_RE.sub(' ', text).strip()
    return text.lower()


def match_wake_word(text: str, phrases: tuple[str, ...]) -> tuple[bool, str]:
    """
    Check if `text` starts with any wake-word phrase.

    Matching is tolerant of punctuation inside the utterance: 'Эй, герта!'
    matches the phrase 'эй герта'. The returned remainder is the command
    portion after the wake word, with leading punctuation stripped.
    """
    normalized = _normalize_for_match(text)
    if not normalized:
        return False, ''

    sorted_phrases = sorted(
        {_normalize_for_match(phrase) for phrase in phrases if phrase},
        key=len,
        reverse=True,
    )

    for phrase in sorted_phrases:
        if not phrase:
            continue

        if normalized == phrase:
            return True, ''

        if normalized.startswith(phrase + ' '):
            remainder = normalized[len(phrase):].lstrip()
            return True, remainder

    return False, normalized
