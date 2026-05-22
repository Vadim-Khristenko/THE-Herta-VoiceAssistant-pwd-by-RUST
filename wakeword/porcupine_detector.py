import logging

import numpy as np

try:
    import pvporcupine
except ImportError:  # pragma: no cover - depends on the local virtual environment
    pvporcupine = None


logger = logging.getLogger(__name__)


class PorcupineWakeWordDetector:
    def __init__(
        self,
        *,
        access_key: str,
        keyword_paths: tuple[str, ...],
        sensitivity: float = 0.5,
    ) -> None:
        if pvporcupine is None:
            raise RuntimeError(
                "Wake-word mode 'porcupine' requires the 'pvporcupine' package. "
                "Run: python -m pip install pvporcupine"
            )
        if not access_key:
            raise RuntimeError('PORCUPINE_ACCESS_KEY is not configured.')
        if not keyword_paths:
            raise RuntimeError('PORCUPINE_KEYWORD_PATHS is empty. Provide at least one .ppn file.')

        clamped_sensitivity = max(0.0, min(1.0, sensitivity))
        sensitivities = [clamped_sensitivity] * len(keyword_paths)

        self._porcupine = pvporcupine.create(
            access_key=access_key,
            keyword_paths=list(keyword_paths),
            sensitivities=sensitivities,
        )
        self.sample_rate: int = self._porcupine.sample_rate
        self.frame_length: int = self._porcupine.frame_length
        self.keyword_paths: tuple[str, ...] = tuple(keyword_paths)

    def process(self, chunk: np.ndarray) -> int:
        """Return keyword index (>=0) if detected, -1 otherwise."""
        if chunk.size != self.frame_length:
            return -1

        if chunk.dtype != np.int16:
            scaled = np.clip(chunk.astype(np.float32) * 32768.0, -32768.0, 32767.0)
            pcm = scaled.astype(np.int16)
        else:
            pcm = chunk

        try:
            return int(self._porcupine.process(pcm))
        except Exception as exc:
            logger.warning("Porcupine process failed: %s", exc)
            return -1

    def close(self) -> None:
        try:
            self._porcupine.delete()
        except Exception:  # pragma: no cover - best-effort cleanup
            pass
