<?php

// PHP 8.1+ Enums
enum Status: string {
    case Pending = 'pending';
    case Active = 'active';
    case Completed = 'completed';
    case Cancelled = 'cancelled';

    public function label(): string {
        return match($this) {
            self::Pending => 'Pending Approval',
            self::Active => 'Currently Active',
            self::Completed => 'Successfully Completed',
            self::Cancelled => 'Cancelled',
        };
    }

    public function isFinished(): bool {
        return $this === self::Completed || $this === self::Cancelled;
    }
}

// PHP 8.0+ Constructor Property Promotion
class Point {
    public function __construct(
        public readonly float $x,
        public readonly float $y,
        public readonly float $z = 0.0,
    ) {}

    public function distanceTo(Point $other): float {
        return sqrt(
            pow($this->x - $other->x, 2) +
            pow($this->y - $other->y, 2) +
            pow($this->z - $other->z, 2)
        );
    }
}

// PHP 8.0+ Named Arguments and Union Types
class Config {
    public function __construct(
        private string|array $value,
        private bool $readonly = false,
    ) {}

    public function getValue(): string|array {
        return $this->value;
    }

    public function isReadonly(): bool {
        return $this->readonly;
    }
}

// PHP 8.0+ Attributes
#[Attribute(Attribute::TARGET_METHOD)]
class Route {
    public function __construct(
        public string $path,
        public string $method = 'GET',
    ) {}
}

class ApiController {
    #[Route('/api/users', method: 'GET')]
    public function listUsers(): array {
        return [];
    }

    #[Route('/api/users', method: 'POST')]
    public function createUser(array $data): array {
        return $data;
    }
}

// PHP 8.1+ Readonly Classes
readonly class ImmutableUser {
    public function __construct(
        public string $name,
        public string $email,
    ) {}
}
