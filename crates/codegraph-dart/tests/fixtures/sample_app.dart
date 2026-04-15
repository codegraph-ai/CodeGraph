import 'dart:async';
import 'dart:convert';

abstract class Entity {
  final int id;
  final String name;
  Entity(this.id, this.name);
  String display();
}

class User extends Entity {
  final String email;
  List<String> _roles = [];

  User(int id, String name, this.email) : super(id, name);

  @override
  String display() => 'User($name, $email)';

  void addRole(String role) {
    if (_roles.contains(role)) return;
    _roles.add(role);
    _validateRole(role);
  }

  bool get isAdmin => _roles.contains('admin');

  void _validateRole(String role) {
    final allowed = ['admin', 'user', 'guest'];
    if (!allowed.contains(role)) {
      throw ArgumentError('Invalid role: $role');
    }
  }
}

class Product extends Entity {
  final double price;

  Product(int id, String name, this.price) : super(id, name);

  @override
  String display() => 'Product($name, \$$price)';

  double discountedPrice(double percent) {
    return _calculateDiscount(price, percent);
  }

  double _calculateDiscount(double original, double percent) {
    return original * (1 - percent / 100);
  }
}

class UserService {
  final List<User> _users = [];

  Future<User> createUser(String name, String email) async {
    final user = User(_users.length + 1, name, email);
    _users.add(user);
    await _sendWelcomeEmail(user);
    return user;
  }

  User? findUser(int id) {
    return _users.where((u) => u.id == id).firstOrNull;
  }

  Future<void> _sendWelcomeEmail(User user) async {
    print('Welcome ${user.name}!');
  }
}
