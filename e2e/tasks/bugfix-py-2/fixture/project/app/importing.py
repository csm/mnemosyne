"""CSV-line import for bulk-adding inventory items.

Each line is `sku,name,qty`, arriving as raw bytes exactly as read off the
wire (e.g. from an uploaded file); `name` may contain non-ASCII characters
(accents, currency symbols) since supplier catalogs are exported as UTF-8.
"""


def parse_import_line(raw: bytes) -> tuple[str, str, int]:
    """Parse one import line given as raw bytes into (sku, name, qty)."""
    line = raw.decode("latin-1")
    sku, name, qty = line.strip().split(",")
    return sku, name, int(qty)
