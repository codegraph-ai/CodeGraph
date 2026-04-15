const std = @import("std");
const mem = @import("std").mem;

pub const Entity = struct {
    id: u64,
    name: []const u8,

    pub fn display(self: Entity) void {
        std.debug.print("Entity({d}, {s})\n", .{ self.id, self.name });
    }
};

pub const User = struct {
    entity: Entity,
    email: []const u8,
    roles: [8]?[]const u8 = .{null} ** 8,
    role_count: usize = 0,

    pub fn init(id: u64, name: []const u8, email: []const u8) User {
        return .{
            .entity = .{ .id = id, .name = name },
            .email = email,
        };
    }

    pub fn addRole(self: *User, role: []const u8) !void {
        if (self.role_count >= self.roles.len) return error.TooManyRoles;
        for (self.roles[0..self.role_count]) |existing| {
            if (existing) |r| {
                if (mem.eql(u8, r, role)) return;
            }
        }
        self.roles[self.role_count] = role;
        self.role_count += 1;
    }

    pub fn isAdmin(self: User) bool {
        for (self.roles[0..self.role_count]) |role| {
            if (role) |r| {
                if (mem.eql(u8, r, "admin")) return true;
            }
        }
        return false;
    }
};

pub const Product = struct {
    entity: Entity,
    price: f64,

    pub fn init(id: u64, name: []const u8, price: f64) Product {
        return .{
            .entity = .{ .id = id, .name = name },
            .price = price,
        };
    }

    pub fn discountedPrice(self: Product, percent: f64) f64 {
        return calculateDiscount(self.price, percent);
    }
};

fn calculateDiscount(original: f64, percent: f64) f64 {
    return original * (1.0 - percent / 100.0);
}

test "user roles" {
    var user = User.init(1, "Alice", "alice@example.com");
    try user.addRole("admin");
    try std.testing.expect(user.isAdmin());
}
