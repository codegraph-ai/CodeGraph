// Shapes example demonstrating protocols, classes, and inheritance

import Foundation

/// A protocol for drawable objects
protocol Drawable {
    func draw()
}

/// A protocol for resizable objects
protocol Resizable {
    func resize(scale: Double)
}

/// A generic container struct
struct Container<T> {
    var items: [T] = []

    mutating func add(_ item: T) {
        items.append(item)
    }

    func get(_ index: Int) -> T? {
        guard index < items.count else { return nil }
        return items[index]
    }
}

/// Enum for colors
enum Color {
    case red
    case green
    case blue
    case custom(r: Int, g: Int, b: Int)
}

/// Base shape class
class Shape: Drawable {
    var name: String
    var color: Color

    init(name: String, color: Color = .red) {
        self.name = name
        self.color = color
    }

    func draw() {
        print("Drawing \(name)")
    }
}

/// Circle class inheriting from Shape
class Circle: Shape, Resizable {
    var radius: Double

    init(radius: Double, color: Color = .blue) {
        self.radius = radius
        super.init(name: "Circle", color: color)
    }

    override func draw() {
        print("Drawing circle with radius \(radius)")
    }

    func resize(scale: Double) {
        radius *= scale
    }

    func area() -> Double {
        return Double.pi * radius * radius
    }
}

/// Rectangle class
class Rectangle: Shape, Resizable {
    var width: Double
    var height: Double

    init(width: Double, height: Double, color: Color = .green) {
        self.width = width
        self.height = height
        super.init(name: "Rectangle", color: color)
    }

    override func draw() {
        print("Drawing rectangle \(width)x\(height)")
    }

    func resize(scale: Double) {
        width *= scale
        height *= scale
    }

    func area() -> Double {
        return width * height
    }
}

/// Extension for Shape
extension Shape {
    func describe() -> String {
        return "A \(name) with color \(color)"
    }
}

/// Async function example
func loadShapesAsync() async -> [Shape] {
    return [Circle(radius: 5), Rectangle(width: 10, height: 5)]
}
