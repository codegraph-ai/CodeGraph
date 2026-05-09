// Basic Rust fixture for the codegraph harness.
// Exercises: fn, struct, impl, trait, dispatch, re-export.

pub struct Order {
    pub item_count: u32,
    pub unit_price_cents: u32,
}

pub fn compute_total(order: &Order) -> u32 {
    order.item_count * order.unit_price_cents
}

pub trait Discountable {
    fn apply_discount(&self, percent: u32) -> u32;
}

impl Discountable for Order {
    fn apply_discount(&self, percent: u32) -> u32 {
        let total = compute_total(self);
        total - (total * percent / 100)
    }
}

pub fn final_price(order: &Order, discount_percent: u32) -> u32 {
    order.apply_discount(discount_percent)
}

pub use compute_total as total;
