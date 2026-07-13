"""Request payload validation for the task API."""
from datetime import datetime, timezone

from .errors import ValidationError


def parse_due_at(raw: str) -> datetime:
    try:
        dt = datetime.fromisoformat(raw.replace("Z", "+00:00"))
    except (TypeError, ValueError) as exc:
        raise ValidationError(f"due_at is not a valid ISO-8601 timestamp: {raw!r}") from exc
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt.astimezone(timezone.utc)


def validate_task_payload(payload: dict) -> tuple[str, datetime, list[str]]:
    if not isinstance(payload, dict):
        raise ValidationError("request body must be a JSON object")

    title = payload.get("title")
    if not isinstance(title, str) or not title.strip():
        raise ValidationError("title is required and must be a non-empty string")

    due_at_raw = payload.get("due_at")
    if not isinstance(due_at_raw, str):
        raise ValidationError("due_at is required and must be a string")
    due_at = parse_due_at(due_at_raw)

    tags = payload.get("tags", [])
    if not isinstance(tags, list) or not all(isinstance(t, str) for t in tags):
        raise ValidationError("tags must be a list of strings")

    return title.strip(), due_at, tags
