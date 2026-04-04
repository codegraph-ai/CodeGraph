# Sample Ruby application for parser verification
require 'json'
require_relative './helper'

module MyApp
  module Models
    # Base class for all entities
    class Entity
      attr_accessor :id, :name

      def initialize(id, name)
        @id = id
        @name = name
      end

      def to_s
        "#{self.class.name}(id=#{@id}, name=#{@name})"
      end
    end

    # User model with authentication
    class User < Entity
      include Serializable
      extend ClassMethods

      attr_reader :email

      def initialize(id, name, email)
        super(id, name)
        @email = email
        @roles = []
      end

      def add_role(role)
        @roles << role
        validate_role(role)
      end

      def admin?
        @roles.include?('admin')
      end

      def self.find_by_email(email)
        # Query database
        query_database(email)
      end

      private

      def validate_role(role)
        allowed_roles = ['admin', 'user', 'guest']
        raise "Invalid role" unless allowed_roles.include?(role)
      end
    end

    # Product model
    class Product < Entity
      attr_accessor :price, :category

      def initialize(id, name, price)
        super(id, name)
        @price = price
      end

      def discounted_price(percent)
        calculate_discount(@price, percent)
      end

      private

      def calculate_discount(original, percent)
        original * (1 - percent / 100.0)
      end
    end
  end

  module Services
    class UserService
      def initialize(repository)
        @repository = repository
      end

      def create_user(name, email)
        user = Models::User.new(generate_id, name, email)
        @repository.save(user)
        send_welcome_email(user)
        user
      end

      def find_user(id)
        @repository.find(id)
      end

      private

      def generate_id
        SecureRandom.uuid
      end

      def send_welcome_email(user)
        EmailService.send(user.email, "Welcome!")
      end
    end
  end
end
