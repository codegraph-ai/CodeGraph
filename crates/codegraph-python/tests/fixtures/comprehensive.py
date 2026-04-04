# Comprehensive test file with all relationship types
import os
from typing import List, Optional
from abc import ABC, abstractmethod

class Animal(ABC):
    """Base animal class."""
    
    def __init__(self, name: str):
        self.name = name
    
    @abstractmethod
    def make_sound(self) -> str:
        pass
    
    def move(self):
        print(f"{self.name} is moving")

class Dog(Animal):
    """Dog class inheriting from Animal."""
    
    def make_sound(self) -> str:
        return "Woof!"
    
    def fetch(self, item: str):
        print(f"{self.name} is fetching {item}")
        return self.move()

class Cat(Animal):
    """Cat class inheriting from Animal."""
    
    def make_sound(self) -> str:
        return "Meow!"
    
    def scratch(self):
        self.move()
        print(f"{self.name} is scratching")

def create_animal(animal_type: str) -> Optional[Animal]:
    """Factory function to create animals."""
    if animal_type == "dog":
        return Dog("Buddy")
    elif animal_type == "cat":
        return Cat("Whiskers")
    return None

def main():
    """Main function demonstrating calls."""
    dog = create_animal("dog")
    cat = create_animal("cat")
    
    if dog:
        dog.make_sound()
        dog.fetch("ball")
    
    if cat:
        cat.make_sound()
        cat.scratch()
    
    print("Done!")

if __name__ == "__main__":
    main()
