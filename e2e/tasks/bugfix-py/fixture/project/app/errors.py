"""Custom exceptions and their Flask error-handler wiring."""
from flask import Flask, jsonify


class ApiError(Exception):
    status_code = 400

    def __init__(self, message: str):
        super().__init__(message)
        self.message = message


class ValidationError(ApiError):
    status_code = 422


class NotFoundError(ApiError):
    status_code = 404


def register_error_handlers(app: Flask) -> None:
    @app.errorhandler(ApiError)
    def handle_api_error(err: ApiError):
        return jsonify({"error": err.message}), err.status_code
