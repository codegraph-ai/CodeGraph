// Test fixture: covers classes, templates, namespaces.

#include <vector>
#include <string>

namespace shop {

struct Order {
    int item_count;
    int unit_price_cents;
};

int compute_total(const Order& order) {
    return order.item_count * order.unit_price_cents;
}

template <typename T>
class Box {
public:
    void add(const T& item) { items_.push_back(item); }
    size_t size() const { return items_.size(); }

private:
    std::vector<T> items_;
};

class Discountable {
public:
    virtual int apply_discount(int percent) const = 0;
    virtual ~Discountable() = default;
};

class FixedOrder : public Discountable {
public:
    explicit FixedOrder(Order o) : order_(o) {}
    int apply_discount(int percent) const override {
        int total = compute_total(order_);
        return total - (total * percent / 100);
    }

private:
    Order order_;
};

}  // namespace shop
