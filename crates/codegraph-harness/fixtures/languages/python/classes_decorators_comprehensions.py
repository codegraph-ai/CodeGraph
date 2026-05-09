"""Test fixture: covers classes, decorators, comprehensions."""

from functools import wraps


def trace(fn):
    """Decorator that prints calls — exercises decorator analysis."""
    @wraps(fn)
    def wrapper(*args, **kwargs):
        print(f"calling {fn.__name__}")
        return fn(*args, **kwargs)
    return wrapper


class Animal:
    def __init__(self, name: str, age: int) -> None:
        self.name = name
        self.age = age

    @property
    def description(self) -> str:
        return f"{self.name} ({self.age})"

    @staticmethod
    def kingdom() -> str:
        return "Animalia"


class Dog(Animal):
    def __init__(self, name: str, age: int, breed: str) -> None:
        super().__init__(name, age)
        self.breed = breed

    @trace
    def bark(self) -> str:
        return f"{self.name} says woof"


def collect_names(animals: list[Animal]) -> list[str]:
    """Comprehension over a list of objects."""
    return [a.name for a in animals if a.age > 0]


def name_to_age(animals: list[Animal]) -> dict[str, int]:
    """Dict comprehension."""
    return {a.name: a.age for a in animals}
