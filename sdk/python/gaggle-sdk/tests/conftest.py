"""Shared test fixtures for gaggle-sdk tests."""

import pytest
from unittest.mock import AsyncMock


@pytest.fixture
def base_url():
    return "http://localhost:8080"


@pytest.fixture
def agent_api_key():
    return "gag_test_api_key_12345"


@pytest.fixture
def user_api_key():
    return "usr_test_user_key_67890"
