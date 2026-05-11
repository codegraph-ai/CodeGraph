"""Test fixture for find_dead_imports — has imports that aren't used."""

import os
import sys
import json
import math

# Only `math` is actually used below; os/sys/json are dead.


def circle_area(radius: float) -> float:
    return math.pi * radius * radius


def square_area(side: float) -> float:
    return side * side
