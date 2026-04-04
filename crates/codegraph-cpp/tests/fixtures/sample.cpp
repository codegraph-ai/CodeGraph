// Sample C++ file for integration testing
#include <iostream>
#include <vector>
#include <memory>

namespace myproject {

/**
 * A generic container class for storing items
 */
template<typename T>
class Container {
public:
    Container() = default;
    ~Container() = default;

    void add(T item) {
        items.push_back(std::move(item));
    }

    T& get(size_t index) {
        return items[index];
    }

    size_t size() const {
        return items.size();
    }

private:
    std::vector<T> items;
};

/// Base interface for processable objects
class Base {
public:
    virtual ~Base() = default;
    virtual void process() = 0;
};

/// Derived implementation of Base
class Derived : public Base {
public:
    void process() override {
        std::cout << "Processing" << std::endl;
    }
};

/// Helper function
void helper(int x) {
    std::cout << x << std::endl;
}

int main() {
    Container<int> container;
    container.add(1);
    container.add(2);

    auto derived = std::make_unique<Derived>();
    derived->process();

    helper(container.size());

    return 0;
}

} // namespace myproject
