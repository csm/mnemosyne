"""Visible happy-path tests. Do not modify this file."""
import pytest

from app import create_app


@pytest.fixture()
def client():
    app = create_app()
    app.testing = True
    return app.test_client()


def test_health(client):
    resp = client.get("/health")
    assert resp.status_code == 200
    assert resp.get_json() == {"status": "ok"}


def test_create_and_list_item(client):
    resp = client.post("/items", json={"sku": "MUG-1", "name": "Mug", "qty": 10})
    assert resp.status_code == 201
    body = resp.get_json()
    assert body["sku"] == "MUG-1"
    assert body["qty"] == 10

    resp = client.get("/items")
    assert resp.status_code == 200
    assert len(resp.get_json()["items"]) == 1


def test_restock_increases_qty(client):
    created = client.post("/items", json={"sku": "MUG-1", "name": "Mug", "qty": 10}).get_json()
    resp = client.post(f"/items/{created['id']}/restock", json={"qty": 5})
    assert resp.status_code == 200
    assert resp.get_json()["qty"] == 15


def test_import_ascii_line(client):
    resp = client.post("/items/import", data=b"MUG-2,Coffee Mug,20")
    assert resp.status_code == 201
    body = resp.get_json()
    assert body["name"] == "Coffee Mug"


def test_validation_rejects_missing_name(client):
    resp = client.post("/items", json={"sku": "MUG-1", "qty": 10})
    assert resp.status_code == 422
