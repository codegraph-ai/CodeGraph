local json = require("json")
local utils = require("utils")

-- Entity base "class"
local Entity = {}
Entity.__index = Entity

function Entity.new(id, name)
    local self = setmetatable({}, Entity)
    self.id = id
    self.name = name
    return self
end

function Entity:display()
    return string.format("Entity(%d, %s)", self.id, self.name)
end

-- User class inheriting from Entity
local User = setmetatable({}, { __index = Entity })
User.__index = User

function User.new(id, name, email)
    local self = Entity.new(id, name)
    setmetatable(self, User)
    self.email = email
    self.roles = {}
    return self
end

function User:addRole(role)
    if self:hasRole(role) then
        return
    end
    table.insert(self.roles, role)
    self:validateRole(role)
end

function User:hasRole(role)
    for _, r in ipairs(self.roles) do
        if r == role then
            return true
        end
    end
    return false
end

function User:isAdmin()
    return self:hasRole("admin")
end

local function validateRole(role)
    local allowed = { admin = true, user = true, guest = true }
    if not allowed[role] then
        error("Invalid role: " .. role)
    end
end

-- Product
local Product = setmetatable({}, { __index = Entity })
Product.__index = Product

function Product.new(id, name, price)
    local self = Entity.new(id, name)
    setmetatable(self, Product)
    self.price = price
    return self
end

function Product:discountedPrice(percent)
    return calculateDiscount(self.price, percent)
end

local function calculateDiscount(original, percent)
    return original * (1 - percent / 100)
end

-- UserService
local UserService = {}
UserService.__index = UserService

function UserService.new()
    local self = setmetatable({}, UserService)
    self.users = {}
    return self
end

function UserService:createUser(name, email)
    local id = #self.users + 1
    local user = User.new(id, name, email)
    table.insert(self.users, user)
    self:sendWelcomeEmail(user)
    return user
end

function UserService:findUser(id)
    for _, user in ipairs(self.users) do
        if user.id == id then
            return user
        end
    end
    return nil
end

function UserService:sendWelcomeEmail(user)
    print("Welcome " .. user.name .. "!")
end

return { Entity = Entity, User = User, Product = Product, UserService = UserService }
