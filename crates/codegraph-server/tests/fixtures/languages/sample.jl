module MyApp

using DataFrames
import JSON: parse as json_parse

struct User
    name::String
    email::String
end

function create_user(name::String, email::String)::User
    return User(name, email)
end

function greet(user::User)
    if isempty(user.name)
        println("Hello, stranger")
    else
        println("Hello, $(user.name)")
    end
end

end
