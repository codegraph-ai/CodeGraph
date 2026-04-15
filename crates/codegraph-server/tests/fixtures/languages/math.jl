module MathUtils

export distance, normalize, dot_product

struct Vec3
    x::Float64
    y::Float64
    z::Float64
end

function distance(a::Vec3, b::Vec3)::Float64
    dx = a.x - b.x
    dy = a.y - b.y
    dz = a.z - b.z
    return sqrt(dx^2 + dy^2 + dz^2)
end

function normalize(v::Vec3)::Vec3
    d = distance(v, Vec3(0, 0, 0))
    if d == 0
        return v
    end
    return Vec3(v.x/d, v.y/d, v.z/d)
end

function dot_product(a::Vec3, b::Vec3)::Float64
    return a.x*b.x + a.y*b.y + a.z*b.z
end

end
