module Physics

using ..MathUtils: Vec3, distance, normalize, dot_product

struct Particle
    position::Vec3
    velocity::Vec3
    mass::Float64
end

function kinetic_energy(p::Particle)::Float64
    v_squared = dot_product(p.velocity, p.velocity)
    return 0.5 * p.mass * v_squared
end

function update_position(p::Particle, dt::Float64)::Particle
    new_pos = Vec3(
        p.position.x + p.velocity.x * dt,
        p.position.y + p.velocity.y * dt,
        p.position.z + p.velocity.z * dt
    )
    return Particle(new_pos, p.velocity, p.mass)
end

function apply_force(p::Particle, force::Vec3, dt::Float64)::Particle
    ax = force.x / p.mass
    ay = force.y / p.mass
    az = force.z / p.mass
    new_vel = Vec3(p.velocity.x + ax*dt, p.velocity.y + ay*dt, p.velocity.z + az*dt)
    return Particle(p.position, new_vel, p.mass)
end

end
