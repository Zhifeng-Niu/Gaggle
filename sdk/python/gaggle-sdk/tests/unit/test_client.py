"""Unit tests for gaggle._client.GaggleClient.

Tests REST client method signatures, request construction, and error handling
using mocked HTTP responses.
"""

import pytest
from unittest.mock import AsyncMock, MagicMock, patch

from gaggle._client import GaggleClient
from gaggle.exceptions import (
    AuthenticationError,
    ConnectionError as GaggleConnectionError,
    ForbiddenError,
    NotFoundError,
    ServerError,
    ValidationError,
)


# ── Fixtures ───────────────────────────────────────────────────────


@pytest.fixture
def client():
    """Create a GaggleClient with mocked httpx client."""
    c = GaggleClient(api_key="gag_test_key", base_url="http://localhost:8080")
    c._client = MagicMock()
    return c


def _mock_response(status_code: int, json_data: dict | None = None):
    """Create a mock httpx.Response."""
    resp = MagicMock()
    resp.status_code = status_code
    resp.json.return_value = json_data or {}
    return resp


# ── Client Initialization ──────────────────────────────────────────


class TestClientInit:
    def test_strips_trailing_slash(self):
        c = GaggleClient(api_key="k", base_url="http://host/")
        assert c._base_url == "http://host"

    def test_stores_config(self):
        c = GaggleClient(api_key="gag_abc", base_url="http://host", timeout=5.0)
        assert c._api_key == "gag_abc"
        assert c._timeout == 5.0


class TestEnsureClient:
    @pytest.mark.asyncio
    async def test_raises_when_not_initialized(self):
        c = GaggleClient(api_key="k")
        with pytest.raises(Exception, match="Client not initialized"):
            await c._request("GET", "/test")


# ── Error Handling ─────────────────────────────────────────────────


class TestErrorHandling:
    @pytest.mark.asyncio
    async def test_400_raises_validation_error(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(400, {"error": "Bad input"})
        )
        with pytest.raises(ValidationError, match="Bad input"):
            await client._request("GET", "/bad")

    @pytest.mark.asyncio
    async def test_401_raises_authentication_error(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(401, {"error": "Token expired"})
        )
        with pytest.raises(AuthenticationError, match="Token expired"):
            await client._request("GET", "/protected")

    @pytest.mark.asyncio
    async def test_403_raises_forbidden_error(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(403, {"error": "No access"})
        )
        with pytest.raises(ForbiddenError):
            await client._request("GET", "/forbidden")

    @pytest.mark.asyncio
    async def test_404_raises_not_found_error(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(404, {"error": "Not found"})
        )
        with pytest.raises(NotFoundError):
            await client._request("GET", "/missing")

    @pytest.mark.asyncio
    async def test_500_raises_server_error(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(500, {"message": "DB down"})
        )
        with pytest.raises(ServerError, match="DB down"):
            await client._request("GET", "/broken")


# ── Health Check ───────────────────────────────────────────────────


class TestHealthCheck:
    @pytest.mark.asyncio
    async def test_health_check(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"status": "ok"})
        )
        result = await client.health_check()
        assert result == {"status": "ok"}
        client._client.request.assert_called_once_with("GET", "/health")


# ── Space APIs ─────────────────────────────────────────────────────


class TestSpaceAPIs:
    _SPACE_DATA = {
        "id": "sp_1",
        "name": "Test",
        "creator_id": "agent_1",
        "agent_ids": ["agent_1", "agent_2"],
        "status": "active",
        "created_at": 1000,
        "updated_at": 1000,
    }

    _MSG_DATA = {
        "id": "msg_1",
        "space_id": "sp_1",
        "sender_id": "agent_1",
        "msg_type": "text",
        "content": "hello",
        "timestamp": 1000,
        "round": 1,
    }

    @pytest.mark.asyncio
    async def test_create_space(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, self._SPACE_DATA)
        )
        result = await client.create_space("Test", ["agent_2"], {"topic": "price"})
        assert result.id == "sp_1"
        client._client.request.assert_called_once()
        call_kwargs = client._client.request.call_args
        assert call_kwargs[0][0] == "POST"
        assert "/api/v1/spaces" in call_kwargs[0][1]

    @pytest.mark.asyncio
    async def test_send_message(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, self._MSG_DATA)
        )
        result = await client.send_message("sp_1", "hello")
        assert result.content == "hello"

    @pytest.mark.asyncio
    async def test_send_message_with_inline_proposal(self, client):
        msg_data = {**self._MSG_DATA, "id": "msg_2", "content": "propose", "msg_type": "proposal"}
        client._client.request = AsyncMock(
            return_value=_mock_response(200, msg_data)
        )
        result = await client.send_message(
            "sp_1",
            "propose",
            msg_type="proposal",
            proposal={"dimensions": {"price": 100}},
        )
        call_kwargs = client._client.request.call_args
        body = call_kwargs[1]["json"]
        assert "proposal" in body
        assert body["proposal"]["dimensions"]["price"] == 100

    @pytest.mark.asyncio
    async def test_close_space(self, client):
        data = {**self._SPACE_DATA, "status": "concluded"}
        client._client.request = AsyncMock(
            return_value=_mock_response(200, data)
        )
        result = await client.close_space("sp_1", "concluded")
        assert result.status == "concluded"


# ── Phase 9: Rules APIs ───────────────────────────────────────────


class TestRulesAPIs:
    @pytest.mark.asyncio
    async def test_get_rules(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"visibility": "public"})
        )
        result = await client.get_rules("sp_1")
        assert result["visibility"] == "public"

    @pytest.mark.asyncio
    async def test_update_rules(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"visibility": "private"})
        )
        result = await client.update_rules("sp_1", {"visibility": "private"})
        assert result["visibility"] == "private"
        call_kwargs = client._client.request.call_args
        assert call_kwargs[0][0] == "PUT"

    @pytest.mark.asyncio
    async def test_get_rule_transitions(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"transitions": []})
        )
        result = await client.get_rule_transitions("sp_1")
        assert "transitions" in result


# ── Phase 9: SubSpace APIs ────────────────────────────────────────


class TestSubSpaceAPIs:
    @pytest.mark.asyncio
    async def test_create_subspace(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"id": "sub_1", "name": "Sub"})
        )
        result = await client.create_subspace("sp_1", "Sub")
        assert result["id"] == "sub_1"

    @pytest.mark.asyncio
    async def test_list_subspaces(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, [{"id": "sub_1"}])
        )
        result = await client.list_subspaces("sp_1")
        assert len(result) == 1

    @pytest.mark.asyncio
    async def test_send_subspace_message(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"id": "msg_sub"})
        )
        result = await client.send_subspace_message("sub_1", "hello")
        assert result["id"] == "msg_sub"

    @pytest.mark.asyncio
    async def test_close_subspace(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"id": "sub_1", "status": "concluded"})
        )
        result = await client.close_subspace("sub_1", "concluded")
        assert result["status"] == "concluded"


# ── Phase 10: Coalition APIs ──────────────────────────────────────


class TestCoalitionAPIs:
    @pytest.mark.asyncio
    async def test_create_coalition(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"id": "coal_1", "name": "Alliance"})
        )
        result = await client.create_coalition("sp_1", "Alliance")
        assert result["name"] == "Alliance"

    @pytest.mark.asyncio
    async def test_join_coalition(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"id": "coal_1", "joined": True})
        )
        result = await client.join_coalition("coal_1")
        assert result["joined"] is True

    @pytest.mark.asyncio
    async def test_disband_coalition(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"disbanded": True})
        )
        result = await client.disband_coalition("coal_1")
        assert result["disbanded"] is True


# ── Phase 11: Delegation APIs ─────────────────────────────────────


class TestDelegationAPIs:
    @pytest.mark.asyncio
    async def test_create_delegation(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"id": "del_1", "scope": "vote"})
        )
        result = await client.create_delegation("sp_1", "agent_2", "vote")
        assert result["scope"] == "vote"

    @pytest.mark.asyncio
    async def test_revoke_delegation(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"revoked": True})
        )
        result = await client.revoke_delegation("del_1")
        assert result["revoked"] is True
        call_kwargs = client._client.request.call_args
        assert call_kwargs[0][0] == "DELETE"


# ── Phase 12: Recruitment APIs ────────────────────────────────────


class TestRecruitmentAPIs:
    @pytest.mark.asyncio
    async def test_create_recruitment(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"id": "rec_1", "target_id": "agent_3"})
        )
        result = await client.create_recruitment("sp_1", "agent_3")
        assert result["target_id"] == "agent_3"

    @pytest.mark.asyncio
    async def test_accept_recruitment(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"accepted": True})
        )
        result = await client.accept_recruitment("sp_1", "rec_1")
        assert result["accepted"] is True

    @pytest.mark.asyncio
    async def test_reject_recruitment(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, {"rejected": True})
        )
        result = await client.reject_recruitment("sp_1", "rec_1")
        assert result["rejected"] is True

    @pytest.mark.asyncio
    async def test_list_recruitments(self, client):
        client._client.request = AsyncMock(
            return_value=_mock_response(200, [{"id": "rec_1"}])
        )
        result = await client.list_recruitments("sp_1")
        assert len(result) == 1
