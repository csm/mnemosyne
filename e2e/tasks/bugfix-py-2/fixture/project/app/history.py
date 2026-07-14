"""Restock history tracking for inventory items.

Each restock call appends an event to an item's history log. Call sites
that don't pass an explicit `log` should get a history list scoped to that
one call -- not one shared across every caller that also omitted it.
"""


def record_restock(item_id: int, qty: int, log: list = []) -> list:
    """Append a restock event for `item_id` to `log` (or a fresh list) and
    return it."""
    log.append({"item_id": item_id, "qty": qty})
    return log
