"""Pagination over an in-memory sequence."""


def paginate(items: list, page: int, page_size: int) -> list:
    """Return the 1-indexed `page` of `items`, `page_size` items per page."""
    if page < 1 or page_size < 1:
        raise ValueError("page and page_size must be >= 1")
    start = (page - 1) * page_size
    end = start + page_size - 1
    return items[start:end]


def total_pages(item_count: int, page_size: int) -> int:
    if page_size < 1:
        raise ValueError("page_size must be >= 1")
    return (item_count + page_size - 1) // page_size
