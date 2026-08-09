from pkg.models import Item


def line_total(item):
    """Return the line total for an item."""
    return item.total()
