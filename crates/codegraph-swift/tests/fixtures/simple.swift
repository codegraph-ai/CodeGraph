// Simple Swift file for testing

import Foundation

/// A simple class for testing
class Person {
    var name: String
    var age: Int

    init(name: String, age: Int) {
        self.name = name
        self.age = age
    }

    func greet() -> String {
        return "Hello, \(name)!"
    }
}

/// A free function
func greetWorld() -> String {
    return "Hello, World!"
}
