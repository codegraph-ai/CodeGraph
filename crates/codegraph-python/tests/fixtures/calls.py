def greet(name):
    print(f"Hello, {name}!")
    return name.upper()

def main():
    greet("World")
    result = greet("Alice")
    print(result)

class Calculator:
    def add(self, a, b):
        return a + b
    
    def multiply(self, a, b):
        return self.add(a, 0) + a * b

calc = Calculator()
calc.add(5, 3)
calc.multiply(4, 2)
