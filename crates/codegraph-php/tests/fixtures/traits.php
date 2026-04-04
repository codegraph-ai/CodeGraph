<?php

trait Loggable {
    public function log(string $message): void {
        echo "[LOG] " . $message . "\n";
    }

    public function debug(string $message): void {
        echo "[DEBUG] " . $message . "\n";
    }
}

trait Serializable {
    public function serialize(): string {
        return json_encode($this);
    }

    public function unserialize(string $data): void {
        // Implementation
    }
}

class Logger {
    use Loggable;

    private string $name;

    public function __construct(string $name) {
        $this->name = $name;
    }

    public function info(string $message): void {
        $this->log("[{$this->name}] " . $message);
    }
}

class DataObject {
    use Loggable, Serializable;

    public array $data;

    public function __construct(array $data) {
        $this->data = $data;
    }
}
