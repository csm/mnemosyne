"""Timezone-aware date helpers.

All timestamps stored by the service are UTC (`datetime` with `tzinfo`).
Display/"due today" logic needs the calendar date in the user's local
timezone, given as a fixed UTC offset in hours (positive east of UTC).
"""
from datetime import date, datetime, timedelta


def local_date(dt_utc: datetime, tz_offset_hours: float) -> date:
    """The calendar date `dt_utc` falls on in a timezone `tz_offset_hours`
    east of UTC."""
    local_dt = dt_utc - timedelta(hours=tz_offset_hours)
    return local_dt.date()


def is_due_today(dt_utc: datetime, tz_offset_hours: float, today_utc: datetime) -> bool:
    """Is `dt_utc` due on the same local calendar date as `today_utc`,
    given the user's timezone offset?"""
    return local_date(dt_utc, tz_offset_hours) == local_date(today_utc, tz_offset_hours)
