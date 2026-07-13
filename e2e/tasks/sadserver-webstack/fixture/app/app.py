"""Minimal items API backed by SQLite, fronted by nginx (see ../nginx/app.conf)."""
import os
import sqlite3
from flask import Flask, jsonify, request

DB_PATH = os.environ.get("ITEMS_DB_PATH", "/data/items.db")
PORT = int(os.environ.get("PORT", "5000"))

app = Flask(__name__)


def get_db():
    conn = sqlite3.connect(DB_PATH)
    conn.execute("CREATE TABLE IF NOT EXISTS items (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
    return conn


@app.get("/health")
def health():
    return jsonify({"status": "ok"})


@app.post("/items")
def create_item():
    payload = request.get_json(silent=True) or {}
    name = payload.get("name")
    if not isinstance(name, str) or not name.strip():
        return jsonify({"error": "name is required"}), 422
    conn = get_db()
    cur = conn.execute("INSERT INTO items (name) VALUES (?)", (name.strip(),))
    conn.commit()
    item_id = cur.lastrowid
    conn.close()
    return jsonify({"id": item_id, "name": name.strip()}), 201


@app.get("/items")
def list_items():
    conn = get_db()
    rows = conn.execute("SELECT id, name FROM items ORDER BY id").fetchall()
    conn.close()
    return jsonify({"items": [{"id": r[0], "name": r[1]} for r in rows]})


if __name__ == "__main__":
    app.run(host="127.0.0.1", port=PORT)
