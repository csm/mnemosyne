"""HTTP surface for the inventory API."""
from flask import Blueprint, current_app, jsonify, request

from .validation import validate_item_payload

bp = Blueprint("inventory", __name__)


def _service():
    return current_app.extensions["inventory_service"]


def _item_json(item) -> dict:
    return {
        "id": item.id,
        "sku": item.sku,
        "name": item.name,
        "qty": item.qty,
        "history": item.history,
    }


@bp.get("/health")
def health():
    return jsonify({"status": "ok"})


@bp.post("/items")
def create_item():
    sku, name, qty = validate_item_payload(request.get_json(silent=True) or {})
    item = _service().create_item(sku, name, qty)
    return jsonify(_item_json(item)), 201


@bp.get("/items")
def list_items():
    return jsonify({"items": [_item_json(i) for i in _service().list_items()]})


@bp.post("/items/<int:item_id>/restock")
def restock(item_id):
    body = request.get_json(silent=True) or {}
    qty = body.get("qty")
    if not isinstance(qty, int) or isinstance(qty, bool) or qty <= 0:
        return jsonify({"error": "qty must be a positive integer"}), 422
    item = _service().restock(item_id, qty)
    return jsonify(_item_json(item))


@bp.post("/items/import")
def import_item():
    raw = request.get_data()
    item = _service().import_line(raw)
    return jsonify(_item_json(item)), 201
