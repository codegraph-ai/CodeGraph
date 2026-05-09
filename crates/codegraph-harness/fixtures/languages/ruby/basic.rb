# Test fixture: covers classes, modules, blocks/iterators.

module Shop
  class Order
    attr_reader :item_count, :unit_price_cents

    def initialize(item_count, unit_price_cents)
      @item_count = item_count
      @unit_price_cents = unit_price_cents
    end

    def compute_total
      item_count * unit_price_cents
    end
  end

  module Discountable
    def apply_discount(percent)
      total = compute_total
      total - (total * percent / 100)
    end
  end

  class DiscountedOrder < Order
    include Discountable
  end

  def self.collect_totals(orders)
    orders.map(&:compute_total).reject { |t| t.zero? }
  end
end
