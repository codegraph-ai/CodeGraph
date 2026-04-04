"""Main module for the test project."""

from utils import helper_function

def main():
    """Entry point for the application."""
    print("Hello from main!")
    result = helper_function(42)
    return result

if __name__ == "__main__":
    main()
