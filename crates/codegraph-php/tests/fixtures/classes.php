<?php

/**
 * Represents a person
 */
class Person {
    public string $name;
    private int $age;

    public function __construct(string $name, int $age) {
        $this->name = $name;
        $this->age = $age;
    }

    public function getName(): string {
        return $this->name;
    }

    public function getAge(): int {
        return $this->age;
    }

    public static function create(string $name, int $age): Person {
        return new Person($name, $age);
    }
}

abstract class Animal {
    protected string $species;

    abstract public function makeSound(): string;

    public function getSpecies(): string {
        return $this->species;
    }
}

class Dog extends Animal {
    public function __construct() {
        $this->species = "Canis familiaris";
    }

    public function makeSound(): string {
        return "Woof!";
    }

    public function fetch(): void {
        echo "Fetching!";
    }
}
