// Sample Kotlin application for parser verification
package com.myapp.models

import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking

/**
 * Base interface for all entities
 */
interface Entity {
    val id: Int
    val name: String
    fun toDisplayString(): String
}

/**
 * Interface for serializable objects
 */
interface Serializable {
    fun serialize(): String
    fun deserialize(data: String)
}

/**
 * Base class for all entities
 */
abstract class BaseEntity(
    override val id: Int,
    override val name: String
) : Entity {
    override fun toDisplayString(): String {
        return "${this::class.simpleName}(id=$id, name=$name)"
    }
}

/**
 * User model with authentication
 */
class User(
    id: Int,
    name: String,
    val email: String
) : BaseEntity(id, name), Serializable {

    private val roles = mutableListOf<String>()

    fun addRole(role: String) {
        roles.add(role)
        validateRole(role)
    }

    fun isAdmin(): Boolean {
        return roles.contains("admin")
    }

    override fun serialize(): String {
        return """{"id":$id,"name":"$name","email":"$email"}"""
    }

    override fun deserialize(data: String) {
        parseJson(data)
    }

    private fun validateRole(role: String) {
        val allowedRoles = listOf("admin", "user", "guest")
        require(role in allowedRoles) { "Invalid role" }
    }

    private fun parseJson(data: String) {
        // JSON parsing logic
    }

    companion object {
        fun findByEmail(email: String): User {
            return queryDatabase(email)
        }

        private fun queryDatabase(email: String): User {
            return User(1, "Test", email)
        }
    }
}

/**
 * Product model
 */
class Product(
    id: Int,
    name: String,
    var price: Double
) : BaseEntity(id, name) {

    var category: String? = null

    fun getDiscountedPrice(percent: Double): Double {
        return calculateDiscount(price, percent)
    }

    private fun calculateDiscount(original: Double, percent: Double): Double {
        return original * (1 - percent / 100.0)
    }
}

/**
 * Data class for configuration
 */
data class AppConfig(
    val databaseUrl: String,
    val maxConnections: Int
)

/**
 * Sealed class for results
 */
sealed class Result<out T> {
    data class Success<T>(val data: T) : Result<T>()
    data class Error(val message: String) : Result<Nothing>()
    object Loading : Result<Nothing>()
}

/**
 * Generic repository interface
 */
interface Repository<T : Entity> {
    fun find(id: Int): T?
    fun save(entity: T)
    fun getAll(): List<T>
}

/**
 * Status enumeration
 */
enum class OrderStatus {
    PENDING,
    PROCESSING,
    SHIPPED,
    DELIVERED,
    CANCELLED
}

/**
 * Object singleton for email service
 */
object EmailService {
    fun send(email: String, message: String) {
        println("Sending to $email: $message")
    }

    suspend fun sendAsync(email: String, message: String) {
        delay(100)
        println("Async sending to $email: $message")
    }
}

/**
 * User service for managing users
 */
class UserService(private val repository: Repository<User>) {

    fun createUser(name: String, email: String): User {
        val user = User(generateId(), name, email)
        repository.save(user)
        sendWelcomeEmail(user)
        return user
    }

    fun findUser(id: Int): User? {
        return repository.find(id)
    }

    suspend fun createUserAsync(name: String, email: String): User {
        val user = User(generateId(), name, email)
        repository.save(user)
        sendWelcomeEmailAsync(user)
        return user
    }

    private fun generateId(): Int {
        return (Math.random() * Int.MAX_VALUE).toInt()
    }

    private fun sendWelcomeEmail(user: User) {
        EmailService.send(user.email, "Welcome!")
    }

    private suspend fun sendWelcomeEmailAsync(user: User) {
        EmailService.sendAsync(user.email, "Welcome!")
    }
}

/**
 * Extension functions
 */
fun String.isValidEmail(): Boolean {
    return this.contains("@") && this.contains(".")
}

fun Entity.describe(): String {
    return "Entity: ${toDisplayString()}"
}

/**
 * Top-level function
 */
fun main() {
    val user = User(1, "John", "john@example.com")
    println(user.toDisplayString())
}
