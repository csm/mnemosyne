"""Hidden regression suite for bugfix-py. NEVER shipped in the agent image.

Superset of tests/test_happy_path.py: covers both seeded bugs plus the
happy-path behaviors. Run with the fixed-up project's `app` package on
PYTHONPATH (grade.py does this by running pytest with rootdir=<snapshot>/project).
"""
from datetime import date, datetime, timezone

import pytest

from app import create_app
from app.pagination import paginate, total_pages
from app.timeutil import is_due_today, local_date


@pytest.fixture()
def client():
    app = create_app()
    app.testing = True
    return app.test_client()


# --- bug 1: off-by-one in pagination (shallow) -----------------------------


def test_paginate_full_page_has_exactly_page_size_items():
    items = list(range(20))
    page = paginate(items, page=1, page_size=10)
    assert page == list(range(0, 10))


def test_paginate_second_page_starts_where_first_left_off():
    items = list(range(20))
    first = paginate(items, page=1, page_size=10)
    second = paginate(items, page=2, page_size=10)
    assert first + second == items


def test_paginate_last_partial_page():
    items = list(range(15))
    assert paginate(items, page=2, page_size=10) == list(range(10, 15))


def test_total_pages_matches_paginate_coverage():
    items = list(range(25))
    n = total_pages(len(items), page_size=10)
    covered = []
    for p in range(1, n + 1):
        covered.extend(paginate(items, page=p, page_size=10))
    assert covered == items


def test_api_list_tasks_full_page_via_http(client):
    for i in range(15):
        client.post(
            "/tasks",
            json={"title": f"task {i}", "due_at": "2024-06-01T15:00:00Z"},
        )
    resp = client.get("/tasks?page=1&page_size=10")
    body = resp.get_json()
    assert body["total_items"] == 15
    assert len(body["items"]) == 10
    assert [t["title"] for t in body["items"]] == [f"task {i}" for i in range(10)]


# --- bug 2: UTC-offset sign error in local date (deep) ---------------------


def test_local_date_crosses_boundary_with_negative_offset():
    # 02:00 UTC on Jan 1 is still Dec 31 evening in US Eastern (UTC-5).
    dt = datetime(2024, 1, 1, 2, 0, tzinfo=timezone.utc)
    assert local_date(dt, -5) == date(2023, 12, 31)


def test_local_date_crosses_boundary_with_positive_offset():
    # 22:00 UTC on Jan 1 is already Jan 2 morning in JST (UTC+9).
    dt = datetime(2024, 1, 1, 22, 0, tzinfo=timezone.utc)
    assert local_date(dt, 9) == date(2024, 1, 2)


def test_local_date_matches_utc_when_offset_is_zero():
    dt = datetime(2024, 1, 1, 2, 0, tzinfo=timezone.utc)
    assert local_date(dt, 0) == date(2024, 1, 1)


def test_is_due_today_uses_local_calendar_day_not_utc_day():
    # Due at 02:00 UTC Jan 1 == Dec 31 21:00 local (UTC-5).
    due = datetime(2024, 1, 1, 2, 0, tzinfo=timezone.utc)
    # "Now" is 23:00 UTC Dec 31 == Dec 31 18:00 local (UTC-5): same local day.
    today_ref = datetime(2023, 12, 31, 23, 0, tzinfo=timezone.utc)
    assert is_due_today(due, -5, today_ref) is True


def test_is_due_today_false_across_local_midnight():
    # Due at 04:00 UTC Jan 2 == Jan 1 23:00 local (UTC-5).
    due = datetime(2024, 1, 2, 4, 0, tzinfo=timezone.utc)
    # "Now" is 06:00 UTC Jan 2 == Jan 2 01:00 local (UTC-5): different local day.
    today_ref = datetime(2024, 1, 2, 6, 0, tzinfo=timezone.utc)
    assert is_due_today(due, -5, today_ref) is False
