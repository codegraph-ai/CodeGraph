<?php

namespace App\Controllers;

use App\Models\User;
use App\Services\AuthService;
use App\Exceptions\AuthException;
use Psr\Log\LoggerInterface as Logger;

class AuthController {
    private AuthService $authService;
    private Logger $logger;

    public function __construct(AuthService $authService, Logger $logger) {
        $this->authService = $authService;
        $this->logger = $logger;
    }

    public function login(string $email, string $password): User {
        $this->logger->info("Login attempt for: " . $email);

        try {
            return $this->authService->authenticate($email, $password);
        } catch (AuthException $e) {
            $this->logger->error("Login failed: " . $e->getMessage());
            throw $e;
        }
    }

    public function logout(User $user): void {
        $this->authService->invalidateSession($user);
        $this->logger->info("User logged out: " . $user->getEmail());
    }
}
