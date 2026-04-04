"""User model."""

class User:
    """Represents a user."""
    
    def __init__(self, name, email):
        self.name = name
        self.email = email
    
    def get_display_name(self):
        """Get the display name."""
        return f"{self.name} <{self.email}>"
