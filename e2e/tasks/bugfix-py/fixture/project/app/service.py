"""Business logic for tasks: sits between the routes and the store."""
from datetime import datetime, timezone

from .errors import NotFoundError
from .models import Task, TaskStore
from .pagination import paginate, total_pages
from .timeutil import is_due_today


class TaskService:
    def __init__(self, store: TaskStore | None = None) -> None:
        self.store = store or TaskStore()

    def create_task(self, title: str, due_at: datetime, tags: list[str]) -> Task:
        return self.store.add(title=title, due_at=due_at, tags=tags)

    def get_task(self, task_id: int) -> Task:
        task = self.store.get(task_id)
        if task is None:
            raise NotFoundError(f"no task with id {task_id}")
        return task

    def list_page(self, page: int, page_size: int) -> dict:
        items = self.store.all()
        return {
            "items": paginate(items, page, page_size),
            "page": page,
            "page_size": page_size,
            "total_items": len(items),
            "total_pages": total_pages(len(items), page_size),
        }

    def due_today(self, tz_offset_hours: float, now_utc: datetime | None = None) -> list[Task]:
        now_utc = now_utc or datetime.now(timezone.utc)
        return [
            t for t in self.store.all()
            if not t.done and is_due_today(t.due_at, tz_offset_hours, now_utc)
        ]
