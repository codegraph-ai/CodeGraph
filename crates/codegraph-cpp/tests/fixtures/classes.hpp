// Header file with class definitions
#ifndef CLASSES_HPP
#define CLASSES_HPP

#include <string>

namespace shapes {

/// A 2D point
struct Point {
    int x;
    int y;
};

/// Shape interface
class Shape {
public:
    virtual ~Shape() = default;
    virtual double area() const = 0;
    virtual double perimeter() const = 0;
};

/// Circle implementation
class Circle : public Shape {
public:
    Circle(Point center, double radius);
    double area() const override;
    double perimeter() const override;

private:
    Point center_;
    double radius_;
};

/// Rectangle implementation
class Rectangle : public Shape {
public:
    Rectangle(Point topLeft, double width, double height);
    double area() const override;
    double perimeter() const override;

private:
    Point topLeft_;
    double width_;
    double height_;
};

/// Enum class for colors
enum class Color {
    Red,
    Green,
    Blue
};

/// Regular enum for status
enum Status {
    Pending,
    Active,
    Done
};

} // namespace shapes

#endif // CLASSES_HPP
