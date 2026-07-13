"""In-memory task store."""
from dataclasses import dataclass, field
from datetime import datetime
from itertools import count


@dataclass
class Task:
    id: int
    title: str
    due_at: datetime  # always UTC, tzinfo-aware
    tags: list[str] = field(default_factory=list)
    done: bool = False


class TaskStore:
    """A process-local task store. Good enough for a demo API; a real
    service would back this with a database."""

    def __init__(self) -> None:
        self._tasks: dict[int, Task] = {}
        self._ids = count(1)

    def add(self, title: str, due_at: datetime, tags: list[str] | None) -> Task:
        task = Task(id=next(self._ids), title=title, due_at=due_at, tags=tags or [])
        self._tasks[task.id] = task
        return task

    def all(self) -> list[Task]:
        return sorted(self._tasks.values(), key=lambda t: t.id)

    def get(self, task_id: int) -> Task | None:
        return self._tasks.get(task_id)
