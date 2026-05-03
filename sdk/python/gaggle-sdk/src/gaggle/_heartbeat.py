"""WebSocket heartbeat (ping/pong) manager."""

import asyncio
import json
import logging
import time

from gaggle.exceptions import TimeoutError

logger = logging.getLogger("gaggle")


class Heartbeat:
    """Manages WebSocket heartbeat (ping/pong) to detect dead connections.

    The heartbeat sends periodic ping messages and tracks activity on the
    connection. If no messages (sent or received) occur within the timeout
    period, the connection is considered dead and a TimeoutError is raised.

    Args:
        send_callback: Async callable that takes a dict or JSON str to send.
        interval: Seconds between heartbeat pings. Default: 30.0.
        timeout: Seconds of inactivity before considering connection dead.
                 Default: 90.0 (3x interval).

    Example:
        async def send(msg):
            await websocket.send(json.dumps(msg))

        heartbeat = Heartbeat(send_callback=send)
        await heartbeat.start()

        # In your message receive loop:
        heartbeat.record_activity()
    """

    def __init__(
        self,
        send_callback,  # async callable that takes a dict/str to send
        interval: float = 30.0,
        timeout: float = 90.0,
    ):
        self._send = send_callback
        self._interval = interval
        self._timeout = timeout
        self._last_message_time: float = time.monotonic()
        self._task: asyncio.Task | None = None
        self._running = False

    def record_activity(self):
        """Record that a message was sent or received.

        Call this whenever any message is sent or received to update
        the last activity timestamp.
        """
        self._last_message_time = time.monotonic()

    async def start(self):
        """Start the heartbeat background task.

        Creates an asyncio task that periodically sends pings and checks
        for connection timeout.
        """
        if self._running:
            logger.warning("Heartbeat already running")
            return

        self._running = True
        self._task = asyncio.create_task(self._run())
        logger.debug(f"Heartbeat started (interval={self._interval}s, timeout={self._timeout}s)")

    async def stop(self):
        """Stop the heartbeat background task.

        Cancels the heartbeat task and waits for it to complete.
        """
        self._running = False
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
            self._task = None
            logger.debug("Heartbeat stopped")

    async def _run(self):
        """Main heartbeat loop."""
        try:
            while self._running:
                await asyncio.sleep(self._interval)

                if not self._running:
                    break

                # Check for timeout
                elapsed = time.monotonic() - self._last_message_time
                if elapsed > self._timeout:
                    logger.warning(
                        f"Heartbeat timeout: no activity for {elapsed:.1f}s "
                        f"(threshold: {self._timeout}s)"
                    )
                    raise TimeoutError(
                        f"Heartbeat timeout: no activity for {elapsed:.0f}s"
                    )

                # Send ping
                try:
                    ping_msg = {
                        "type": "ping",
                        "timestamp": int(time.time() * 1000),
                    }
                    await self._send(json.dumps(ping_msg))
                    self.record_activity()
                    logger.debug(f"Heartbeat ping sent (last activity: {elapsed:.1f}s ago)")
                except Exception as e:
                    logger.error(f"Failed to send heartbeat ping: {e}")
                    raise

        except asyncio.CancelledError:
            logger.debug("Heartbeat task cancelled")
            raise
        except Exception as e:
            logger.error(f"Heartbeat task failed: {e}")
            raise

    @property
    def is_running(self) -> bool:
        """Check if heartbeat is currently running."""
        return self._running and self._task is not None and not self._task.done()

    @property
    def last_activity_elapsed(self) -> float:
        """Get seconds elapsed since last recorded activity."""
        return time.monotonic() - self._last_message_time
