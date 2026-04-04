<?php

interface Readable {
    public function read(): string;
}

interface Writable {
    public function write(string $data): void;
}

interface ReadWritable extends Readable, Writable {
    public function seek(int $position): void;
}

interface Countable {
    public function count(): int;
}

class FileStream implements ReadWritable {
    private string $path;
    private int $position = 0;

    public function __construct(string $path) {
        $this->path = $path;
    }

    public function read(): string {
        return file_get_contents($this->path);
    }

    public function write(string $data): void {
        file_put_contents($this->path, $data);
    }

    public function seek(int $position): void {
        $this->position = $position;
    }
}

class Counter implements Countable {
    private int $value = 0;

    public function increment(): void {
        $this->value++;
    }

    public function count(): int {
        return $this->value;
    }
}
