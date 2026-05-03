"""Local SQLite offline event queue for missed WebSocket events."""

import aiosqlite
import json
import logging
import time
from pathlib import Path

logger = logging.getLogger("gaggle")


class OfflineQueue:
    """Local SQLite queue for events missed during disconnection.

    When the WebSocket connection is lost, important events can be queued
    locally for replay once reconnected. This uses SQLite for persistence
    and supports both in-memory and file-based storage.

    Args:
        db_path: Path to SQLite database file. Uses in-memory storage if None.

    Example:
        queue = OfflineQueue(":memory:")
        await queue.open()
        await queue.push("new_message", {"space_id": "abc", "content": "hello"})
        events = await queue.pop_all()
        await queue.close()
    """

    def __init__(self, db_path: str | Path | None = None):
        self._db_path = str(db_path) if db_path else ":memory:"
        self._db: aiosqlite.Connection | None = None

    async def open(self):
        """Open the database connection and create schema if needed."""
        self._db = await aiosqlite.connect(self._db_path)
        await self._db.execute(
            """
            CREATE TABLE IF NOT EXISTS offline_events (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at REAL NOT NULL
            )
        """
        )
        # Create index for faster queries
        await self._db.execute(
            "CREATE INDEX IF NOT EXISTS idx_created_at ON offline_events(created_at)"
        )
        await self._db.commit()
        logger.debug(f"Offline queue opened: {self._db_path}")

    async def close(self):
        """Close the database connection."""
        if self._db:
            await self._db.close()
            self._db = None
            logger.debug("Offline queue closed")

    async def push(self, event_type: str, payload: dict):
        """Store an event for later replay.

        Args:
            event_type: The type of event (e.g., "new_message", "new_proposal").
            payload: Event data as a dictionary.
        """
        if not self._db:
            logger.warning("Cannot push event: queue not open")
            return

        await self._db.execute(
            "INSERT INTO offline_events (event_type, payload, created_at) VALUES (?, ?, ?)",
            (event_type, json.dumps(payload), time.time()),
        )
        await self._db.commit()
        logger.debug(f"Queued offline event: {event_type}")

    async def pop_all(self) -> list[tuple[str, dict]]:
        """Return and delete all queued events.

        Events are returned in FIFO order (by sequence number).

        Returns:
            List of tuples (event_type, payload).
        """
        if not self._db:
            return []

        cursor = await self._db.execute(
            "SELECT seq, event_type, payload FROM offline_events ORDER BY seq"
        )
        rows = await cursor.fetchall()

        if rows:
            await self._db.execute("DELETE FROM offline_events")
            await self._db.commit()
            logger.info(f"Replaying {len(rows)} offline events")

        return [(row[1], json.loads(row[2])) for row in rows]

    async def clear(self):
        """Remove all queued events without returning them."""
        if self._db:
            await self._db.execute("DELETE FROM offline_events")
            await self._db.commit()
            logger.debug("Offline queue cleared")

    async def count(self) -> int:
        """Get the number of queued events."""
        if not self._db:
            return 0

        cursor = await self._db.execute("SELECT COUNT(*) FROM offline_events")
        result = await cursor.fetchone()
        return result[0] if result else 0

    @property
    def is_open(self) -> bool:
        """Check if the queue is open."""
        return self._db is not None
