// Sample C# application for parser verification
using System;
using System.Collections.Generic;
using System.Threading.Tasks;

namespace MyApp.Models
{
    /// <summary>
    /// Base interface for all entities
    /// </summary>
    public interface IEntity
    {
        int Id { get; }
        string Name { get; }
        string ToDisplayString();
    }

    /// <summary>
    /// Interface for serializable objects
    /// </summary>
    public interface ISerializable
    {
        string Serialize();
        void Deserialize(string data);
    }

    /// <summary>
    /// Base class for all entities
    /// </summary>
    public abstract class Entity : IEntity
    {
        public int Id { get; protected set; }
        public string Name { get; protected set; }

        protected Entity(int id, string name)
        {
            Id = id;
            Name = name;
        }

        public virtual string ToDisplayString()
        {
            return $"{GetType().Name}(Id={Id}, Name={Name})";
        }
    }

    /// <summary>
    /// User model with authentication
    /// </summary>
    public class User : Entity, ISerializable
    {
        private readonly List<string> _roles;

        public string Email { get; private set; }

        public User(int id, string name, string email) : base(id, name)
        {
            Email = email;
            _roles = new List<string>();
        }

        public void AddRole(string role)
        {
            _roles.Add(role);
            ValidateRole(role);
        }

        public bool IsAdmin()
        {
            return _roles.Contains("admin");
        }

        public static User FindByEmail(string email)
        {
            return QueryDatabase(email);
        }

        public string Serialize()
        {
            return $"{{\"id\":{Id},\"name\":\"{Name}\",\"email\":\"{Email}\"}}";
        }

        public void Deserialize(string data)
        {
            // Parse JSON and set properties
            ParseJson(data);
        }

        private void ValidateRole(string role)
        {
            var allowedRoles = new[] { "admin", "user", "guest" };
            if (!Array.Exists(allowedRoles, r => r == role))
            {
                throw new ArgumentException("Invalid role");
            }
        }

        private static User QueryDatabase(string email)
        {
            // Simulated database query
            return new User(1, "Test", email);
        }

        private void ParseJson(string data)
        {
            // JSON parsing logic
        }
    }

    /// <summary>
    /// Product model
    /// </summary>
    public class Product : Entity
    {
        public decimal Price { get; set; }
        public string Category { get; set; }

        public Product(int id, string name, decimal price) : base(id, name)
        {
            Price = price;
        }

        public decimal GetDiscountedPrice(decimal percent)
        {
            return CalculateDiscount(Price, percent);
        }

        private decimal CalculateDiscount(decimal original, decimal percent)
        {
            return original * (1 - percent / 100m);
        }
    }

    /// <summary>
    /// Generic repository interface
    /// </summary>
    public interface IRepository<T> where T : IEntity
    {
        T Find(int id);
        void Save(T entity);
        IEnumerable<T> GetAll();
    }

    /// <summary>
    /// Record type for configuration
    /// </summary>
    public record AppConfig(string DatabaseUrl, int MaxConnections);

    /// <summary>
    /// Struct for coordinates
    /// </summary>
    public struct Point
    {
        public int X { get; set; }
        public int Y { get; set; }

        public Point(int x, int y)
        {
            X = x;
            Y = y;
        }

        public double DistanceTo(Point other)
        {
            return Math.Sqrt(Math.Pow(other.X - X, 2) + Math.Pow(other.Y - Y, 2));
        }
    }

    /// <summary>
    /// Status enumeration
    /// </summary>
    public enum OrderStatus
    {
        Pending,
        Processing,
        Shipped,
        Delivered,
        Cancelled
    }
}

namespace MyApp.Services
{
    using MyApp.Models;

    /// <summary>
    /// User service for managing users
    /// </summary>
    public class UserService
    {
        private readonly IRepository<User> _repository;

        public UserService(IRepository<User> repository)
        {
            _repository = repository;
        }

        public User CreateUser(string name, string email)
        {
            var user = new User(GenerateId(), name, email);
            _repository.Save(user);
            SendWelcomeEmail(user);
            return user;
        }

        public User FindUser(int id)
        {
            return _repository.Find(id);
        }

        public async Task<User> CreateUserAsync(string name, string email)
        {
            var user = new User(GenerateId(), name, email);
            await Task.Run(() => _repository.Save(user));
            await SendWelcomeEmailAsync(user);
            return user;
        }

        private int GenerateId()
        {
            return new Random().Next(1, int.MaxValue);
        }

        private void SendWelcomeEmail(User user)
        {
            EmailService.Send(user.Email, "Welcome!");
        }

        private async Task SendWelcomeEmailAsync(User user)
        {
            await EmailService.SendAsync(user.Email, "Welcome!");
        }
    }

    /// <summary>
    /// Email service placeholder
    /// </summary>
    public static class EmailService
    {
        public static void Send(string email, string message)
        {
            Console.WriteLine($"Sending to {email}: {message}");
        }

        public static async Task SendAsync(string email, string message)
        {
            await Task.Delay(100);
            Console.WriteLine($"Async sending to {email}: {message}");
        }
    }
}
