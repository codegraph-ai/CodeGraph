; Sample Clojure application for codegraph parser tests

(ns myapp.core
  (:require [clojure.string :as str]
            [clojure.set :refer [union difference]])
  (:import (java.util Date)))

;;; Protocols

(defprotocol Describable
  "Things that can describe themselves"
  (describe [this])
  (summary [this verbose?]))

(defprotocol Identifiable
  (get-id [this])
  (get-name [this]))

;;; Records

(defrecord User [id name email roles]
  Describable
  (describe [this]
    (str "User: " (:name this) " <" (:email this) ">"))
  (summary [this verbose?]
    (if verbose?
      (describe this)
      (:name this)))

  Identifiable
  (get-id [this] (:id this))
  (get-name [this] (:name this)))

(defrecord Product [id name price category]
  Describable
  (describe [this]
    (str "Product: " (:name this) " $" (:price this)))
  (summary [this _verbose?]
    (:name this))

  Identifiable
  (get-id [this] (:id this))
  (get-name [this] (:name this)))

;;; Functions

(defn make-user
  "Create a new User record"
  [id name email]
  (->User id name email #{}))

(defn make-product
  "Create a new Product record"
  [id name price category]
  (->Product id name price category))

(defn- validate-email
  "Check email format (private)"
  [email]
  (and (string? email)
       (str/includes? email "@")))

(defn- validate-price
  "Ensure price is positive (private)"
  [price]
  (and (number? price) (pos? price)))

(defn add-role
  "Add a role to a user"
  [user role]
  (update user :roles conj role))

(defn has-role?
  "Check if user has a given role"
  [user role]
  (contains? (:roles user) role))

(defn admin?
  "Is the user an admin?"
  [user]
  (has-role? user :admin))

(defn find-user
  "Find a user by id in a collection"
  [users id]
  (first (filter #(= (:id %) id) users)))

(defn calculate-discount
  "Apply a percentage discount to a price"
  [price percent]
  (let [factor (- 1 (/ percent 100.0))]
    (* price factor)))

(defn discounted-price
  "Get discounted price for a product"
  [product percent]
  (calculate-discount (:price product) percent))

(defn users-with-role
  "Return all users with a specific role"
  [users role]
  (filter #(has-role? % role) users))

(defn send-welcome-email
  "Simulate sending a welcome email"
  [user]
  (println (str "Welcome " (:name user) "! Sent to " (:email user))))

(defn create-user-service
  "Build a simple user service map"
  []
  {:users []
   :next-id (atom 1)})

(defn register-user
  "Register a new user in the service"
  [service name email]
  (if (validate-email email)
    (let [id    @(:next-id service)
          user  (make-user id name email)
          _     (swap! (:next-id service) inc)]
      (-> service
          (update :users conj user)
          (doto (send-welcome-email user))
          user))
    (throw (ex-info "Invalid email" {:email email}))))

(defn top-products
  "Return the N most expensive products"
  [products n]
  (take n (sort-by :price > products)))
