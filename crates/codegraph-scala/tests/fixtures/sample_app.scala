import scala.collection.mutable
import scala.concurrent.Future
import scala.concurrent.ExecutionContext.Implicits.global

abstract class Entity(val id: Int, val name: String) {
  def display(): String
}

class User(id: Int, name: String, val email: String) extends Entity(id, name) {
  private val roles = mutable.ListBuffer[String]()

  override def display(): String = s"User($name, $email)"

  def addRole(role: String): Unit = {
    if (!roles.contains(role)) {
      roles += role
      validateRole(role)
    }
  }

  def isAdmin: Boolean = roles.contains("admin")

  private def validateRole(role: String): Unit = {
    val allowed = Set("admin", "user", "guest")
    if (!allowed.contains(role)) {
      throw new IllegalArgumentException(s"Invalid role: $role")
    }
  }
}

class Product(id: Int, name: String, val price: Double) extends Entity(id, name) {
  override def display(): String = s"Product($name, $$$price)"

  def discountedPrice(percent: Double): Double = {
    calculateDiscount(price, percent)
  }

  private def calculateDiscount(original: Double, percent: Double): Double = {
    original * (1 - percent / 100)
  }
}

object UserService {
  private val users = mutable.ListBuffer[User]()

  def createUser(name: String, email: String): Future[User] = Future {
    val user = new User(users.length + 1, name, email)
    users += user
    sendWelcomeEmail(user)
    user
  }

  def findUser(id: Int): Option[User] = {
    users.find(_.id == id)
  }

  private def sendWelcomeEmail(user: User): Unit = {
    println(s"Welcome ${user.name}!")
  }
}

trait Serializable {
  def toJson: String
}
