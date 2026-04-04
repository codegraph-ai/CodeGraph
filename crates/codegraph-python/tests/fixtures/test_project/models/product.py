"""Product model."""

class Product:
    """Represents a product."""
    
    def __init__(self, name, price):
        self.name = name
        self.price = price
    
    def get_price(self):
        """Get the product price."""
        return self.price
