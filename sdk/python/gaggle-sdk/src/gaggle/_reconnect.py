"""Exponential backoff with full jitter reconnect policy."""

import random
import logging

logger = logging.getLogger("gaggle")


class ReconnectPolicy:
    """WebSocket reconnection policy with exponential backoff and full jitter.

    This implements the "Full Jitter" algorithm from:
    https://aws.amazon.com/blogs/architecture/exponential-backoff-and-jitter/

    The delay is calculated as: random(0, min(base_delay * 2^attempt, max_delay))

    Args:
        base_delay: Initial delay in seconds before first reconnect attempt.
        max_delay: Maximum delay cap in seconds.
        max_attempts: Maximum number of reconnect attempts. None means unlimited.

    Example:
        policy = ReconnectPolicy(base_delay=1.0, max_delay=30.0, max_attempts=5)
        for attempt in range(10):
            if not policy.should_retry(attempt):
                break
            delay = policy.next_delay(attempt)
            await asyncio.sleep(delay)
    """

    def __init__(
        self,
        base_delay: float = 1.0,
        max_delay: float = 30.0,
        max_attempts: int | None = None,
    ):
        self._base_delay = base_delay
        self._max_delay = max_delay
        self._max_attempts = max_attempts

    @property
    def base_delay(self) -> float:
        return self._base_delay

    @property
    def max_delay(self) -> float:
        return self._max_delay

    @property
    def max_attempts(self) -> int | None:
        return self._max_attempts

    def next_delay(self, attempt: int) -> float:
        """Calculate delay for the next reconnect attempt.

        Args:
            attempt: The attempt number (0-indexed).

        Returns:
            Delay in seconds as a float.
        """
        exponential_delay = self._base_delay * (2 ** attempt)
        capped_delay = min(exponential_delay, self._max_delay)
        jittered_delay = random.uniform(0, capped_delay)
        logger.debug(
            f"Reconnect attempt {attempt}: delay={jittered_delay:.2f}s "
            f"(exponential={exponential_delay:.2f}s, capped={capped_delay:.2f}s)"
        )
        return jittered_delay

    def should_retry(self, attempt: int) -> bool:
        """Check if another reconnect attempt should be made.

        Args:
            attempt: The attempt number (0-indexed).

        Returns:
            True if retry should proceed, False otherwise.
        """
        if self._max_attempts is None:
            return True
        return attempt < self._max_attempts
