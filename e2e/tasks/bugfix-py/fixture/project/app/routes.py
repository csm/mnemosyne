"""HTTP surface for the task API."""
from flask import Blueprint, current_app, jsonify, request

from .validation import validate_task_payload

bp = Blueprint("tasks", __name__)


def _service():
    return current_app.extensions["task_service"]


def _task_json(task) -> dict:
    return {
        "id": task.id,
        "title": task.title,
        "due_at": task.due_at.isoformat(),
        "tags": task.tags,
        "done": task.done,
    }


@bp.get("/health")
def health():
    return jsonify({"status": "ok"})


@bp.post("/tasks")
def create_task():
    title, due_at, tags = validate_task_payload(request.get_json(silent=True) or {})
    task = _service().create_task(title, due_at, tags)
    return jsonify(_task_json(task)), 201


@bp.get("/tasks")
def list_tasks():
    page = int(request.args.get("page", 1))
    page_size = int(request.args.get("page_size", 10))
    result = _service().list_page(page, page_size)
    result["items"] = [_task_json(t) for t in result["items"]]
    return jsonify(result)


@bp.get("/tasks/due-today")
def due_today():
    tz_offset_hours = float(request.args.get("tz_offset_hours", 0))
    tasks = _service().due_today(tz_offset_hours)
    return jsonify({"items": [_task_json(t) for t in tasks]})
