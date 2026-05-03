"""Unit tests for gaggle.exceptions module.

Tests exception hierarchy, status_code_to_exception mapping,
and custom exception behavior.
"""

import pytest

from gaggle.exceptions import (
    AuthenticationError,
    ConnectionError,
    ForbiddenError,
    GaggleError,
    NotFoundError,
    RateLimitError,
    ReconnectFailedError,
    ServerError,
    SpaceClosedError,
    SpaceNotFoundError,
    TimeoutError,
    ValidationError,
    WsProtocolError,
    status_code_to_exception,
)


# ── Exception Hierarchy ──────────────────────────────────────────


class TestExceptionHierarchy:
    """All exceptions must inherit from GaggleError."""

    @pytest.mark.parametrize(
        "exc_cls",
        [
            ConnectionError,
            AuthenticationError,
            ForbiddenError,
            NotFoundError,
            SpaceNotFoundError,
            ValidationError,
            SpaceClosedError,
            ServerError,
            RateLimitError,
            TimeoutError,
            ReconnectFailedError,
            WsProtocolError,
        ],
    )
    def test_inherits_from_gaggle_error(self, exc_cls):
        assert issubclass(exc_cls, GaggleError)

    @pytest.mark.parametrize(
        "exc_cls",
        [
            ConnectionError,
            AuthenticationError,
            ForbiddenError,
            NotFoundError,
            SpaceNotFoundError,
            ValidationError,
            SpaceClosedError,
            ServerError,
            RateLimitError,
            TimeoutError,
            ReconnectFailedError,
            WsProtocolError,
        ],
    )
    def test_inherits_from_base_exception(self, exc_cls):
        assert issubclass(exc_cls, Exception)

    def test_space_not_found_inherits_from_not_found(self):
        assert issubclass(SpaceNotFoundError, NotFoundError)


# ── GaggleError Base ─────────────────────────────────────────────


class TestGaggleError:
    def test_message_preserved(self):
        err = GaggleError("something went wrong")
        assert err.message == "something went wrong"
        assert str(err) == "something went wrong"

    def test_code_defaults_to_class_name(self):
        err = GaggleError("fail")
        assert err.code == "GaggleError"

    def test_custom_code(self):
        err = GaggleError("fail", code="CUSTOM_CODE")
        assert err.code == "CUSTOM_CODE"

    def test_can_be_raised_and_caught(self):
        with pytest.raises(GaggleError) as exc_info:
            raise GaggleError("boom")
        assert exc_info.value.message == "boom"


# ── Individual Exception Classes ─────────────────────────────────


class TestConnectionError:
    def test_code(self):
        err = ConnectionError("network down")
        assert err.code == "CONNECTION_ERROR"

    def test_message(self):
        err = ConnectionError("network down")
        assert err.message == "network down"
        assert str(err) == "network down"


class TestAuthenticationError:
    def test_default_message(self):
        err = AuthenticationError()
        assert err.message == "Authentication failed"

    def test_custom_message(self):
        err = AuthenticationError("invalid token")
        assert err.message == "invalid token"

    def test_code(self):
        err = AuthenticationError()
        assert err.code == "AUTHENTICATION_ERROR"


class TestForbiddenError:
    def test_default_message(self):
        err = ForbiddenError()
        assert err.message == "Access forbidden"

    def test_code(self):
        assert ForbiddenError().code == "FORBIDDEN_ERROR"


class TestNotFoundError:
    def test_default_message(self):
        err = NotFoundError()
        assert err.message == "Resource not found"

    def test_code(self):
        assert NotFoundError().code == "NOT_FOUND_ERROR"


class TestSpaceNotFoundError:
    def test_message_includes_space_id(self):
        err = SpaceNotFoundError("space_abc123")
        assert "space_abc123" in err.message
        assert err.space_id == "space_abc123"

    def test_inherits_not_found_code(self):
        err = SpaceNotFoundError("space_abc123")
        # SpaceNotFoundError calls NotFoundError.__init__ which sets NOT_FOUND_ERROR
        assert err.code == "NOT_FOUND_ERROR"


class TestValidationError:
    def test_message(self):
        err = ValidationError("invalid field")
        assert err.message == "invalid field"

    def test_code(self):
        assert ValidationError("x").code == "VALIDATION_ERROR"


class TestSpaceClosedError:
    def test_message_includes_space_id_and_status(self):
        err = SpaceClosedError("space_xyz", "concluded")
        assert "space_xyz" in err.message
        assert "concluded" in err.message
        assert err.space_id == "space_xyz"
        assert err.status == "concluded"


class TestServerError:
    def test_default_message(self):
        err = ServerError()
        assert err.message == "Internal server error"

    def test_custom_message(self):
        err = ServerError("database crash")
        assert err.message == "database crash"

    def test_code(self):
        assert ServerError().code == "SERVER_ERROR"


class TestRateLimitError:
    def test_default_message(self):
        err = RateLimitError()
        assert err.message == "Rate limit exceeded"

    def test_code(self):
        assert RateLimitError().code == "RATE_LIMIT_ERROR"


class TestTimeoutError:
    def test_default_message(self):
        err = TimeoutError()
        assert err.message == "Request timeout"

    def test_code(self):
        assert TimeoutError().code == "TIMEOUT_ERROR"


class TestReconnectFailedError:
    def test_default_message(self):
        err = ReconnectFailedError()
        assert err.message == "WebSocket reconnection failed"

    def test_code(self):
        assert ReconnectFailedError().code == "RECONNECT_FAILED_ERROR"


class TestWsProtocolError:
    def test_message(self):
        err = WsProtocolError("invalid frame")
        assert err.message == "invalid frame"

    def test_code(self):
        assert WsProtocolError("x").code == "WS_PROTOCOL_ERROR"


# ── status_code_to_exception ─────────────────────────────────────


class TestStatusCodeToException:
    """Test HTTP status code to exception mapping."""

    def test_400_returns_validation_error(self):
        exc = status_code_to_exception(400)
        assert isinstance(exc, ValidationError)

    def test_401_returns_authentication_error(self):
        exc = status_code_to_exception(401)
        assert isinstance(exc, AuthenticationError)

    def test_403_returns_forbidden_error(self):
        exc = status_code_to_exception(403)
        assert isinstance(exc, ForbiddenError)

    def test_404_returns_not_found_error(self):
        exc = status_code_to_exception(404)
        assert isinstance(exc, NotFoundError)

    def test_429_returns_rate_limit_error(self):
        exc = status_code_to_exception(429)
        assert isinstance(exc, RateLimitError)

    def test_500_returns_server_error(self):
        exc = status_code_to_exception(500)
        assert isinstance(exc, ServerError)

    def test_502_returns_server_error(self):
        exc = status_code_to_exception(502)
        assert isinstance(exc, ServerError)

    def test_503_returns_server_error(self):
        exc = status_code_to_exception(503)
        assert isinstance(exc, ServerError)

    def test_599_returns_server_error(self):
        exc = status_code_to_exception(599)
        assert isinstance(exc, ServerError)

    def test_unknown_code_returns_gaggle_error(self):
        exc = status_code_to_exception(418)
        assert type(exc) is GaggleError
        assert isinstance(exc, GaggleError)

    def test_422_returns_gaggle_error(self):
        # 422 is not in the match statement, falls to default
        exc = status_code_to_exception(422)
        assert type(exc) is GaggleError

    def test_all_returned_are_gaggle_error_subclasses(self):
        """Every result from status_code_to_exception must be a GaggleError."""
        for code in [400, 401, 403, 404, 429, 500, 502, 503, 418]:
            exc = status_code_to_exception(code)
            assert isinstance(exc, GaggleError)

    def test_response_data_error_dict_message(self):
        exc = status_code_to_exception(401, {"error": {"message": "Token expired"}})
        assert isinstance(exc, AuthenticationError)
        assert exc.message == "Token expired"

    def test_response_data_error_string(self):
        exc = status_code_to_exception(401, {"error": "Token expired"})
        assert isinstance(exc, AuthenticationError)
        assert exc.message == "Token expired"

    def test_response_data_top_level_message(self):
        exc = status_code_to_exception(500, {"message": "Database unavailable"})
        assert isinstance(exc, ServerError)
        assert exc.message == "Database unavailable"

    def test_response_data_empty_dict(self):
        exc = status_code_to_exception(400, {})
        assert isinstance(exc, ValidationError)
        assert exc.message == "Unknown error"

    def test_response_data_none(self):
        exc = status_code_to_exception(404, None)
        assert isinstance(exc, NotFoundError)
        assert exc.message == "Unknown error"

    def test_response_data_error_dict_no_message_key(self):
        exc = status_code_to_exception(403, {"error": {"code": "FORBIDDEN"}})
        assert isinstance(exc, ForbiddenError)
        # Falls back to "Unknown error" since "message" key is missing
        assert exc.message == "Unknown error"

    def test_response_data_error_empty_string(self):
        exc = status_code_to_exception(401, {"error": ""})
        assert isinstance(exc, AuthenticationError)
        # Empty string is a valid string value, passed through as-is
        assert exc.message == ""
