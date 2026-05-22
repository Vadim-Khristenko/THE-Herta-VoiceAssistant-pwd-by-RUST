import logging
import time

import numpy as np

from config import WakeWordConfig
from wakeword.matcher import match_wake_word
from wakeword.porcupine_detector import PorcupineWakeWordDetector


logger = logging.getLogger(__name__)

VALID_MODES = frozenset({'text', 'porcupine', 'both'})


class WakeWordCoordinator:
    """Combines Porcupine (audio-level) and text-matcher (post-STT) wake-word detection.

    Tracks an armed-window: once a wake word is observed (either by Porcupine
    or by the text matcher), subsequent utterances within `follow_up_seconds`
    are processed without requiring the wake word again.
    """

    def __init__(self, config: WakeWordConfig) -> None:
        self.config = config
        self._armed_until: float = 0.0
        self._porcupine: PorcupineWakeWordDetector | None = None
        self._init_error: str | None = None

        if not config.enabled:
            return

        if config.mode not in VALID_MODES:
            self._init_error = f"WAKEWORD_MODE={config.mode!r}; expected one of {sorted(VALID_MODES)}."
            logger.warning(self._init_error)
            return

        if config.mode in ('porcupine', 'both'):
            try:
                self._porcupine = PorcupineWakeWordDetector(
                    access_key=config.porcupine_access_key or '',
                    keyword_paths=config.porcupine_keyword_paths,
                    sensitivity=config.porcupine_sensitivity,
                )
                logger.info(
                    "Porcupine wake word ready. sample_rate=%d frame_length=%d keywords=%d",
                    self._porcupine.sample_rate,
                    self._porcupine.frame_length,
                    len(self._porcupine.keyword_paths),
                )
            except RuntimeError as exc:
                self._init_error = str(exc)
                if config.mode == 'porcupine':
                    logger.error("Porcupine init failed: %s", exc)
                else:
                    logger.warning("Porcupine init failed, falling back to text matcher: %s", exc)
                self._porcupine = None

    @property
    def enabled(self) -> bool:
        return self.config.enabled

    @property
    def init_error(self) -> str | None:
        return self._init_error

    @property
    def porcupine_active(self) -> bool:
        return self._porcupine is not None

    @property
    def porcupine_frame_length(self) -> int | None:
        return self._porcupine.frame_length if self._porcupine else None

    def is_armed(self) -> bool:
        if not self.config.enabled:
            return True
        return time.monotonic() < self._armed_until

    def arm(self, duration: float | None = None) -> None:
        if not self.config.enabled:
            return
        window = self.config.follow_up_seconds if duration is None else duration
        self._armed_until = max(self._armed_until, time.monotonic() + window)

    def disarm(self) -> None:
        self._armed_until = 0.0

    def process_audio_chunk(self, chunk: np.ndarray) -> bool:
        """Feed an audio chunk into Porcupine. Returns True if wake word triggered."""
        if self._porcupine is None:
            return False

        flat = np.asarray(chunk).reshape(-1)
        if flat.size != self._porcupine.frame_length:
            return False

        result = self._porcupine.process(flat)
        if result >= 0:
            self.arm()
            logger.info("Wake word detected via Porcupine (keyword_index=%d).", result)
            return True
        return False

    def process_transcript(self, text: str) -> tuple[bool, str, bool]:
        """Decide what to do with a transcribed utterance.

        Returns (should_process, command_text, wake_word_only).
        - should_process=True means run a normal turn with `command_text`.
        - wake_word_only=True means the user said only the wake word (no command).
        - When wake word is disabled, every transcript is processed as-is.
        """
        if not self.config.enabled:
            return True, text, False

        if self.config.mode in ('text', 'both'):
            matched, remainder = match_wake_word(text, self.config.phrases)
            if matched:
                self.arm()
                if remainder:
                    return True, remainder, False
                return False, '', True

        if self.is_armed():
            return True, text, False

        return False, '', False

    def close(self) -> None:
        if self._porcupine is not None:
            self._porcupine.close()
            self._porcupine = None
