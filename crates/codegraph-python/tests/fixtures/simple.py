"""Simple Python module for testing basic parsing."""


def greet(name: str) -> str:
    """Greet someone by name."""
    return f"Hello, {name}!"


class Person:
    """A simple person class."""
    
    def __init__(self, name: str, age: int):
        self.name = name
        self.age = age
    
    def introduce(self) -> str:
        """Return introduction string."""
        return f"My name is {self.name} and I am {self.age} years old."
