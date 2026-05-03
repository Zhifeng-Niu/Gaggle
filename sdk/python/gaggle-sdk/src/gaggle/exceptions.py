"""Gaggle SDK exception hierarchy."""


class GaggleError(Exception):
    """Base exception for all Gaggle SDK errors."""

    def __init__(self, message: str, code: str | None = None):
        super().__init__(message)
        self.message = message
        self.code = code or self.__class__.__name__

    def __str__(self) -> str:
        return self.message


class ConnectionError(GaggleError):
    """Network connection error."""

    def __init__(self, message: str):
        super().__init__(message, "CONNECTION_ERROR")


class AuthenticationError(GaggleError):
    """Authentication failed (401)."""

    def __init__(self, message: str = "Authentication failed"):
        super().__init__(message, "AUTHENTICATION_ERROR")


class ForbiddenError(GaggleError):
    """Access forbidden (403)."""

    def __init__(self, message: str = "Access forbidden"):
        super().__init__(message, "FORBIDDEN_ERROR")


class NotFoundError(GaggleError):
    """Resource not found (404)."""

    def __init__(self, message: str = "Resource not found"):
        super().__init__(message, "NOT_FOUND_ERROR")


class SpaceNotFoundError(NotFoundError):
    """Space not found."""

    def __init__(self, space_id: str):
        super().__init__(f"Space not found: {space_id}")
        self.space_id = space_id


class ValidationError(GaggleError):
    """Invalid request data (400)."""

    def __init__(self, message: str):
        super().__init__(message, "VALIDATION_ERROR")


class SpaceClosedError(GaggleError):
    """Operation not allowed on closed space."""

    def __init__(self, space_id: str, status: str):
        super().__init__(
            f"Space {space_id} is {status}, operation not allowed"
        )
        self.space_id = space_id
        self.status = status


class ServerError(GaggleError):
    """Server error (5xx)."""

    def __init__(self, message: str = "Internal server error"):
        super().__init__(message, "SERVER_ERROR")


class RateLimitError(GaggleError):
    """Rate limit exceeded (429)."""

    def __init__(self, message: str = "Rate limit exceeded"):
        super().__init__(message, "RATE_LIMIT_ERROR")


class TimeoutError(GaggleError):
    """Request timeout."""

    def __init__(self, message: str = "Request timeout"):
        super().__init__(message, "TIMEOUT_ERROR")


class ReconnectFailedError(GaggleError):
    """WebSocket reconnection failed."""

    def __init__(self, message: str = "WebSocket reconnection failed"):
        super().__init__(message, "RECONNECT_FAILED_ERROR")


class WsProtocolError(GaggleError):
    """WebSocket protocol error."""

    def __init__(self, message: str):
        super().__init__(message, "WS_PROTOCOL_ERROR")


def status_code_to_exception(
    status_code: int, response_data: dict | None = None
) -> GaggleError:
    """Convert HTTP status code to appropriate exception.

    Args:
        status_code: HTTP status code
        response_data: Optional parsed error response

    Returns:
        Appropriate GaggleError subclass
    """
    error_message = "Unknown error"
    if response_data and "error" in response_data:
        error_info = response_data["error"]
        if isinstance(error_info, dict):
            error_message = error_info.get("message", error_message)
        elif isinstance(error_info, str):
            error_message = error_info
    elif response_data and "message" in response_data:
        error_message = response_data["message"]

    match status_code:
        case 400:
            return ValidationError(error_message)
        case 401:
            return AuthenticationError(error_message)
        case 403:
            return ForbiddenError(error_message)
        case 404:
            return NotFoundError(error_message)
        case 429:
            return RateLimitError(error_message)
        case code if 500 <= code < 600:
            return ServerError(error_message)
        case _:
            return GaggleError(error_message)
