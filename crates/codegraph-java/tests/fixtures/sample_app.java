// Sample Java application for parser verification
package com.myapp.models;

import java.util.List;
import java.util.ArrayList;
import java.util.concurrent.CompletableFuture;

/**
 * Base interface for all entities
 */
public interface Entity {
    int getId();
    String getName();
    String toDisplayString();
}

/**
 * Interface for serializable objects
 */
public interface Serializable {
    String serialize();
    void deserialize(String data);
}

/**
 * Base class for all entities
 */
public abstract class BaseEntity implements Entity {
    protected int id;
    protected String name;

    protected BaseEntity(int id, String name) {
        this.id = id;
        this.name = name;
    }

    @Override
    public int getId() {
        return id;
    }

    @Override
    public String getName() {
        return name;
    }

    @Override
    public String toDisplayString() {
        return getClass().getSimpleName() + "(id=" + id + ", name=" + name + ")";
    }
}

/**
 * User model with authentication
 */
public class User extends BaseEntity implements Serializable {
    private final List<String> roles;
    private String email;

    public User(int id, String name, String email) {
        super(id, name);
        this.email = email;
        this.roles = new ArrayList<>();
    }

    public String getEmail() {
        return email;
    }

    public void addRole(String role) {
        roles.add(role);
        validateRole(role);
    }

    public boolean isAdmin() {
        return roles.contains("admin");
    }

    public static User findByEmail(String email) {
        return queryDatabase(email);
    }

    @Override
    public String serialize() {
        return "{\"id\":" + id + ",\"name\":\"" + name + "\",\"email\":\"" + email + "\"}";
    }

    @Override
    public void deserialize(String data) {
        parseJson(data);
    }

    private void validateRole(String role) {
        String[] allowedRoles = {"admin", "user", "guest"};
        boolean valid = false;
        for (String allowed : allowedRoles) {
            if (allowed.equals(role)) {
                valid = true;
                break;
            }
        }
        if (!valid) {
            throw new IllegalArgumentException("Invalid role");
        }
    }

    private static User queryDatabase(String email) {
        return new User(1, "Test", email);
    }

    private void parseJson(String data) {
        // JSON parsing logic
    }
}

/**
 * Product model
 */
public class Product extends BaseEntity {
    private double price;
    private String category;

    public Product(int id, String name, double price) {
        super(id, name);
        this.price = price;
    }

    public double getPrice() {
        return price;
    }

    public void setPrice(double price) {
        this.price = price;
    }

    public String getCategory() {
        return category;
    }

    public void setCategory(String category) {
        this.category = category;
    }

    public double getDiscountedPrice(double percent) {
        return calculateDiscount(price, percent);
    }

    private double calculateDiscount(double original, double percent) {
        return original * (1 - percent / 100.0);
    }
}

/**
 * Generic repository interface
 */
public interface Repository<T extends Entity> {
    T find(int id);
    void save(T entity);
    List<T> getAll();
}

/**
 * Status enumeration
 */
public enum OrderStatus {
    PENDING,
    PROCESSING,
    SHIPPED,
    DELIVERED,
    CANCELLED
}

/**
 * Record type for configuration (Java 14+)
 */
public record AppConfig(String databaseUrl, int maxConnections) {}

/**
 * User service for managing users
 */
public class UserService {
    private final Repository<User> repository;

    public UserService(Repository<User> repository) {
        this.repository = repository;
    }

    public User createUser(String name, String email) {
        User user = new User(generateId(), name, email);
        repository.save(user);
        sendWelcomeEmail(user);
        return user;
    }

    public User findUser(int id) {
        return repository.find(id);
    }

    public CompletableFuture<User> createUserAsync(String name, String email) {
        return CompletableFuture.supplyAsync(() -> {
            User user = new User(generateId(), name, email);
            repository.save(user);
            sendWelcomeEmailAsync(user);
            return user;
        });
    }

    private int generateId() {
        return (int) (Math.random() * Integer.MAX_VALUE);
    }

    private void sendWelcomeEmail(User user) {
        EmailService.send(user.getEmail(), "Welcome!");
    }

    private void sendWelcomeEmailAsync(User user) {
        EmailService.sendAsync(user.getEmail(), "Welcome!");
    }
}

/**
 * Email service placeholder
 */
public class EmailService {
    public static void send(String email, String message) {
        System.out.println("Sending to " + email + ": " + message);
    }

    public static CompletableFuture<Void> sendAsync(String email, String message) {
        return CompletableFuture.runAsync(() -> {
            System.out.println("Async sending to " + email + ": " + message);
        });
    }
}
