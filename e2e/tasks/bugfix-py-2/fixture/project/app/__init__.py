"""Flask app factory for the inventory API."""
from flask import Flask

from .errors import register_error_handlers
from .routes import bp
from .service import InventoryService


def create_app() -> Flask:
    app = Flask(__name__)
    app.extensions["inventory_service"] = InventoryService()
    app.register_blueprint(bp)
    register_error_handlers(app)
    return app
