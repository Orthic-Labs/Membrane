class Item:
    """A purchasable line item."""

    def __init__(self, name, price, quantity=1):
        self.name = name
        self.price = price
        self.quantity = quantity

    def total(self):
        """Line total: price times quantity."""
        return self.price * self.quantity
