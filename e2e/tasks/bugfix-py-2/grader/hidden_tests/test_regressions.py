"""Hidden regression suite for bugfix-py-2. NEVER shipped in the agent image.

Superset of tests/test_happy_path.py: covers both seeded bugs plus the
happy-path behaviors. Run with the fixed-up project's `app` package on
PYTHONPATH (grade.py does this by running pytest with rootdir=<snapshot>/project).
"""
import pytest

from app import create_app
from app.history import record_restock
from app.importing import parse_import_line


@pytest.fixture()
def client():
    app = create_app()
    app.testing = True
    return app.test_client()


# --- bug 1: mutable default argument leaks history across items (shallow) --


def test_record_restock_defaults_do_not_leak_across_calls():
    log_a = record_restock(1, 5)
    log_b = record_restock(2, 7)
    assert log_a == [{"item_id": 1, "qty": 5}]
    assert log_b == [{"item_id": 2, "qty": 7}]


def test_api_restock_history_isolated_between_items(client):
    a = client.post("/items", json={"sku": "A", "name": "Item A", "qty": 1}).get_json()
    b = client.post("/items", json={"sku": "B", "name": "Item B", "qty": 1}).get_json()

    client.post(f"/items/{a['id']}/restock", json={"qty": 5})
    client.post(f"/items/{b['id']}/restock", json={"qty": 7})

    items = client.get("/items").get_json()["items"]
    item_a = next(i for i in items if i["id"] == a["id"])
    item_b = next(i for i in items if i["id"] == b["id"])
    assert item_a["history"] == [{"item_id": a["id"], "qty": 5}]
    assert item_b["history"] == [{"item_id": b["id"], "qty": 7}]


# --- bug 2: import mis-decodes non-ASCII product names (deep) -------------


def test_parse_import_line_decodes_utf8_accents():
    raw = "SKU-9,Café Mug,3".encode("utf-8")
    sku, name, qty = parse_import_line(raw)
    assert name == "Café Mug"
    assert qty == 3


def test_api_import_preserves_utf8_name(client):
    resp = client.post("/items/import", data="SKU-9,Café Mug,3".encode("utf-8"))
    assert resp.status_code == 201
    assert resp.get_json()["name"] == "Café Mug"


def test_parse_import_line_ascii_unaffected():
    sku, name, qty = parse_import_line(b"SKU-1,Plain Mug,10")
    assert name == "Plain Mug"
