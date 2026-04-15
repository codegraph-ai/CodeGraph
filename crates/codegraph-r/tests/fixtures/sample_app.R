library(jsonlite)
library(httr)
source("utils.R")

# Entity constructor
Entity <- function(id, name) {
  obj <- list(id = id, name = name)
  class(obj) <- "Entity"
  return(obj)
}

display <- function(entity) {
  sprintf("Entity(%d, %s)", entity$id, entity$name)
}

# User constructor
User <- function(id, name, email) {
  obj <- Entity(id, name)
  obj$email <- email
  obj$roles <- character(0)
  class(obj) <- c("User", "Entity")
  return(obj)
}

add_role <- function(user, role) {
  if (role %in% user$roles) {
    return(user)
  }
  validate_role(role)
  user$roles <- c(user$roles, role)
  return(user)
}

is_admin <- function(user) {
  "admin" %in% user$roles
}

validate_role <- function(role) {
  allowed <- c("admin", "user", "guest")
  if (!(role %in% allowed)) {
    stop(paste("Invalid role:", role))
  }
}

# Product constructor
Product <- function(id, name, price) {
  obj <- Entity(id, name)
  obj$price <- price
  class(obj) <- c("Product", "Entity")
  return(obj)
}

discounted_price <- function(product, percent) {
  calculate_discount(product$price, percent)
}

calculate_discount <- function(original, percent) {
  original * (1 - percent / 100)
}

# UserService
create_user <- function(users, name, email) {
  id <- length(users) + 1
  user <- User(id, name, email)
  users[[id]] <- user
  send_welcome_email(user)
  return(list(users = users, user = user))
}

find_user <- function(users, id) {
  for (user in users) {
    if (user$id == id) {
      return(user)
    }
  }
  return(NULL)
}

send_welcome_email <- function(user) {
  cat(paste("Welcome", user$name, "!\n"))
}
