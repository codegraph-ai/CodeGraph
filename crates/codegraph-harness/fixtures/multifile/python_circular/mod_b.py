"""Test fixture: mod_b imports A from mod_a — closes the cycle with mod_a."""

from mod_a import A


class B:
    def __init__(self) -> None:
        self.parent: A | None = None

    def describe(self) -> str:
        return "B"
