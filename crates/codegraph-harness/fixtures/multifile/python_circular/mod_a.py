"""Test fixture: mod_a imports B from mod_b — half of a circular import."""

from mod_b import B


class A:
    def __init__(self) -> None:
        self.b = B()

    def describe(self) -> str:
        return f"A holding {self.b}"
