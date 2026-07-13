"""Visible happy-path tests. Do not modify this file."""
from datetime import datetime, timezone

import pytest

from app import create_app


@pytest.fixture()
def client():
    app = create_app()
    app.testing = True
    return app.test_client()


def test_health(client):
    resp = client.get("/health")
    assert resp.status_code == 200
    assert resp.get_json() == {"status": "ok"}


def test_create_and_get_task(client):
    resp = client.post(
        "/tasks",
        json={"title": "write report", "due_at": "2024-06-01T15:00:00Z", "tags": ["work"]},
    )
    assert resp.status_code == 201
    body = resp.get_json()
    assert body["title"] == "write report"
    assert body["tags"] == ["work"]


def test_list_tasks_small_page(client):
    for i in range(3):
        client.post(
            "/tasks",
            json={"title": f"task {i}", "due_at": "2024-06-01T15:00:00Z"},
        )
    resp = client.get("/tasks?page=1&page_size=10")
    assert resp.status_code == 200
    body = resp.get_json()
    assert body["total_items"] == 3
    assert len(body["items"]) == 3


def test_due_today_utc(client):
    # Middle of the UTC day, so it is "today" regardless of a UTC offset
    # sign error -- this test intentionally does not probe timezone
    # boundaries (see the hidden regression suite for that).
    today_noon = datetime.now(timezone.utc).replace(
        hour=12, minute=0, second=0, microsecond=0
    )
    client.post(
        "/tasks",
        json={"title": "due now", "due_at": today_noon.isoformat().replace("+00:00", "Z")},
    )
    resp = client.get("/tasks/due-today?tz_offset_hours=0")
    assert resp.status_code == 200
    titles = [t["title"] for t in resp.get_json()["items"]]
    assert "due now" in titles


def test_validation_rejects_missing_title(client):
    resp = client.post("/tasks", json={"due_at": "2024-06-01T15:00:00Z"})
    assert resp.status_code == 422
