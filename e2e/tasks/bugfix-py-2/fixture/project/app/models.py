"""In-memory inventory store."""
from dataclasses import dataclass, field
from itertools import count


@dataclass
class Item:
    id: int
    sku: str
    name: str
    qty: int
    history: list[dict] = field(default_factory=list)


class InventoryStore:
    """A process-local inventory store. Good enough for a demo API; a real
    service would back this with a database."""

    def __init__(self) -> None:
        self._items: dict[int, Item] = {}
        self._ids = count(1)

    def add(self, sku: str, name: str, qty: int) -> Item:
        item = Item(id=next(self._ids), sku=sku, name=name, qty=qty)
        self._items[item.id] = item
        return item

    def all(self) -> list[Item]:
        return sorted(self._items.values(), key=lambda i: i.id)

    def get(self, item_id: int) -> Item | None:
        return self._items.get(item_id)
