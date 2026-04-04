<?php

/**
 * A simple greeting function
 */
function hello(string $name): string {
    return "Hello, " . $name . "!";
}

function add(int $a, int $b): int {
    return $a + $b;
}

function main(): void {
    echo hello("World");
    echo add(1, 2);
}
