"""Request payload validation for the inventory API."""
from .errors import ValidationError


def validate_item_payload(payload: dict) -> tuple[str, str, int]:
    if not isinstance(payload, dict):
        raise ValidationError("request body must be a JSON object")

    sku = payload.get("sku")
    if not isinstance(sku, str) or not sku.strip():
        raise ValidationError("sku is required and must be a non-empty string")

    name = payload.get("name")
    if not isinstance(name, str) or not name.strip():
        raise ValidationError("name is required and must be a non-empty string")

    qty = payload.get("qty")
    if not isinstance(qty, int) or isinstance(qty, bool) or qty < 0:
        raise ValidationError("qty is required and must be a non-negative integer")

    return sku.strip(), name.strip(), qty
