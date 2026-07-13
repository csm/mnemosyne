"""Flask app factory for the task API."""
from flask import Flask

from .errors import register_error_handlers
from .routes import bp
from .service import TaskService


def create_app() -> Flask:
    app = Flask(__name__)
    app.extensions["task_service"] = TaskService()
    app.register_blueprint(bp)
    register_error_handlers(app)
    return app
