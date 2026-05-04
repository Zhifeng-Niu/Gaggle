"""Unit tests for gaggle._client retry logic.

Tests that _request retries on transient errors (502/503/504/connect)
and eventually raises or succeeds as expected.
"""

import pytest
from unittest.mock import AsyncMock, MagicMock, patch

from gaggle._client import GaggleClient
from gaggle.exceptions import (
    ConnectionError as GaggleConnectionError,
    ServerError,
)


def _mock_response(status_code: int, json_data: dict | None = None):
    resp = MagicMock()
    resp.status_code = status_code
    resp.json.return_value = json_data or {}
    return resp


@pytest.fixture
def client():
    c = GaggleClient(api_key="gag_test", base_url="http://localhost:8080", max_retries=2)
    c._client = MagicMock()
    return c


# ── Retry on transient server errors ────────────────────────────────


class TestRetryOnTransientErrors:
    @pytest.mark.asyncio
    async def test_retries_502_then_succeeds(self, client):
        """502 should be retried, then succeed on next attempt."""
        client._client.request = AsyncMock(
            side_effect=[
                _mock_response(502),
                _mock_response(200, {"status": "ok"}),
            ]
        )
        with patch("asyncio.sleep", new_callable=AsyncMock) as mock_sleep:
            result = await client._request("GET", "/health")
        assert result == {"status": "ok"}
        assert client._client.request.call_count == 2
        mock_sleep.assert_called_once()

    @pytest.mark.asyncio
    async def test_retries_503_then_succeeds(self, client):
        client._client.request = AsyncMock(
            side_effect=[
                _mock_response(503),
                _mock_response(200, {"ok": True}),
            ]
        )
        with patch("asyncio.sleep", new_callable=AsyncMock):
            result = await client._request("GET", "/test")
        assert result == {"ok": True}

    @pytest.mark.asyncio
    async def test_retries_504_exhausted_raises(self, client):
        """All retries exhausted on 504 should raise ServerError."""
        client._client.request = AsyncMock(
            return_value=_mock_response(504)
        )
        with patch("asyncio.sleep", new_callable=AsyncMock):
            with pytest.raises(ServerError):
                await client._request("GET", "/test")
        # max_retries=2, so 3 attempts total
        assert client._client.request.call_count == 3

    @pytest.mark.asyncio
    async def test_retries_502_three_times_then_success(self, client):
        """Multiple retries before success."""
        client._client.request = AsyncMock(
            side_effect=[
                _mock_response(502),
                _mock_response(502),
                _mock_response(200, {"data": "finally"}),
            ]
        )
        with patch("asyncio.sleep", new_callable=AsyncMock):
            result = await client._request("GET", "/test")
        assert result == {"data": "finally"}
        assert client._client.request.call_count == 3


# ── Retry on connection errors ──────────────────────────────────────


class TestRetryOnConnectionError:
    @pytest.mark.asyncio
    async def test_retries_connect_error_then_succeeds(self, client):
        import httpx

        client._client.request = AsyncMock(
            side_effect=[
                httpx.ConnectError("refused"),
                _mock_response(200, {"ok": True}),
            ]
        )
        with patch("asyncio.sleep", new_callable=AsyncMock):
            result = await client._request("GET", "/test")
        assert result == {"ok": True}

    @pytest.mark.asyncio
    async def test_connect_error_exhausted_raises(self, client):
        import httpx

        client._client.request = AsyncMock(
            side_effect=httpx.ConnectError("refused")
        )
        with patch("asyncio.sleep", new_callable=AsyncMock):
            with pytest.raises(GaggleConnectionError, match="3 attempts"):
                await client._request("GET", "/test")
        assert client._client.request.call_count == 3


# ── No retry on non-transient errors ────────────────────────────────


class TestNoRetryOnNonTransient:
    @pytest.mark.asyncio
    async def test_400_no_retry(self, client):
        """400 should NOT be retried."""
        client._client.request = AsyncMock(
            return_value=_mock_response(400, {"error": "Bad input"})
        )
        from gaggle.exceptions import ValidationError

        with pytest.raises(ValidationError):
            await client._request("GET", "/bad")
        assert client._client.request.call_count == 1

    @pytest.mark.asyncio
    async def test_401_no_retry(self, client):
        from gaggle.exceptions import AuthenticationError

        client._client.request = AsyncMock(
            return_value=_mock_response(401, {"error": "Unauthorized"})
        )
        with pytest.raises(AuthenticationError):
            await client._request("GET", "/protected")
        assert client._client.request.call_count == 1

    @pytest.mark.asyncio
    async def test_500_no_retry(self, client):
        """500 is a server error but NOT retried (only 502/503/504 retry)."""
        client._client.request = AsyncMock(
            return_value=_mock_response(500, {"error": "Internal"})
        )
        with pytest.raises(ServerError):
            await client._request("GET", "/broken")
        assert client._client.request.call_count == 1


# ── Backoff timing ──────────────────────────────────────────────────


class TestBackoffTiming:
    @pytest.mark.asyncio
    async def test_exponential_backoff(self, client):
        """Verify backoff increases: 0.5s → 1.0s."""
        client._client.request = AsyncMock(
            side_effect=[
                _mock_response(502),
                _mock_response(502),
                _mock_response(200, {"ok": True}),
            ]
        )
        with patch("asyncio.sleep", new_callable=AsyncMock) as mock_sleep:
            await client._request("GET", "/test")

        assert mock_sleep.call_count == 2
        calls = [c.args[0] for c in mock_sleep.call_args_list]
        assert calls[0] == pytest.approx(0.5, abs=0.01)
        assert calls[1] == pytest.approx(1.0, abs=0.01)
