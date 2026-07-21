"""Tests for the Paddle Log API.

Step 4 of the demo walkthrough runs these inside the sandbox.
One test is deliberately marked xfail — see README step 5.
"""

from datetime import date

import pytest
from fastapi.testclient import TestClient

from app.main import app, store
from app.storage import longest_consecutive_run

client = TestClient(app)


@pytest.fixture(autouse=True)
def clear_store():
    store._sessions.clear()
    store._next_id = 1
    yield


def log(paddler: str, day: str, distance_km: float = 5.0) -> dict:
    response = client.post(
        "/sessions",
        json={
            "paddler": paddler,
            "spot": "Santa Cruz Harbor",
            "logged_on": day,
            "distance_km": distance_km,
        },
    )
    assert response.status_code == 201
    return response.json()


def test_healthz():
    assert client.get("/healthz").json() == {"status": "ok"}


def test_create_and_fetch_session():
    created = log("Jay", "2026-07-01", 8.5)
    fetched = client.get(f"/sessions/{created['id']}")
    assert fetched.status_code == 200
    assert fetched.json()["distance_km"] == 8.5
    assert fetched.json()["conditions"] == "glassy"


def test_missing_session_is_404():
    assert client.get("/sessions/999").status_code == 404


def test_list_filters_by_paddler():
    log("Jay", "2026-07-01")
    log("Sam", "2026-07-02")
    assert len(client.get("/sessions").json()) == 2
    assert len(client.get("/sessions", params={"paddler": "jay"}).json()) == 1


def test_delete_session():
    created = log("Jay", "2026-07-01")
    assert client.delete(f"/sessions/{created['id']}").status_code == 204
    assert client.get(f"/sessions/{created['id']}").status_code == 404


def test_stats_aggregate():
    log("Jay", "2026-07-01", 4.0)
    log("Jay", "2026-07-02", 6.0)
    stats = client.get("/paddlers/Jay/stats").json()
    assert stats["sessions"] == 2
    assert stats["total_km"] == 10.0
    assert stats["longest_km"] == 6.0
    assert stats["best_streak_days"] == 2


def test_rejects_negative_distance():
    response = client.post(
        "/sessions",
        json={
            "paddler": "Jay",
            "spot": "Capitola",
            "logged_on": "2026-07-01",
            "distance_km": -3,
        },
    )
    assert response.status_code == 422


@pytest.mark.parametrize(
    ("days", "expected"),
    [
        (set(), 0),
        ({date(2026, 7, 1)}, 1),
        ({date(2026, 7, 1), date(2026, 7, 3)}, 1),
        ({date(2026, 7, 1), date(2026, 7, 2), date(2026, 7, 3)}, 3),
    ],
)
def test_longest_consecutive_run(days, expected):
    assert longest_consecutive_run(days) == expected


@pytest.mark.xfail(reason="Demo step 5: ask the agent to make this pass.", strict=True)
def test_stats_ignores_duplicate_days():
    """Two sessions on one day should count as a single streak day.

    The streak logic already handles this, but `sessions` counts rows — decide
    with the agent what the right answer is and make this pass.
    """
    log("Jay", "2026-07-01", 3.0)
    log("Jay", "2026-07-01", 4.0)
    stats = client.get("/paddlers/Jay/stats").json()
    assert stats["best_streak_days"] == 1
    assert stats["unique_days"] == 1
