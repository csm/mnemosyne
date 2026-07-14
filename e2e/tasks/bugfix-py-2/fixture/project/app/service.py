"""Business logic for inventory: sits between the routes and the store."""
from .errors import NotFoundError
from .history import record_restock
from .importing import parse_import_line
from .models import Item, InventoryStore


class InventoryService:
    def __init__(self, store: InventoryStore | None = None) -> None:
        self.store = store or InventoryStore()

    def create_item(self, sku: str, name: str, qty: int) -> Item:
        return self.store.add(sku=sku, name=name, qty=qty)

    def get_item(self, item_id: int) -> Item:
        item = self.store.get(item_id)
        if item is None:
            raise NotFoundError(f"no item with id {item_id}")
        return item

    def list_items(self) -> list[Item]:
        return self.store.all()

    def restock(self, item_id: int, qty: int) -> Item:
        item = self.get_item(item_id)
        item.qty += qty
        item.history = record_restock(item_id, qty)
        return item

    def import_line(self, raw: bytes) -> Item:
        sku, name, qty = parse_import_line(raw)
        return self.create_item(sku, name, qty)
