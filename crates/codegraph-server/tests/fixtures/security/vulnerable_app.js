/**
 * Intentionally vulnerable Node.js/Express app for security scanner testing.
 * Every function triggers at least one CWE detection.
 */
const express = require('express');
const { exec } = require('child_process');
const mysql = require('mysql');
const fs = require('fs');
const crypto = require('crypto');

const app = express();

// CWE-798: Hardcoded credentials
const DB_PASSWORD = "password123";
const JWT_SECRET = "mysupersecretkey";

// === security_scan patterns ===

// CWE-78: Command injection
function runCommand(userInput) {
    exec(userInput, (err, stdout) => {
        console.log(stdout);
    });
}

// CWE-94: Code injection
function dynamicEval(code) {
    return eval(code);
}

// CWE-79: XSS
function renderHtml(userContent) {
    document.getElementById('output').innerHTML = userContent;
    document.write(userContent);
}

// CWE-89: SQL injection
function getUser(userId) {
    const conn = mysql.createConnection({ host: 'localhost' });
    conn.query("SELECT * FROM users WHERE id = " + userId);
}

// CWE-328: Weak hash
function hashPassword(password) {
    return crypto.createHash('md5').update(password).digest('hex');
}

// CWE-338: Weak PRNG
function generateToken() {
    return Math.random().toString(36).substring(2);
}

// === security_check_unchecked_returns (CWE-252) ===

function processData(data) {
    validateInput(data);
    sanitize(data);
    checkPermissions(data);
    const result = transform(data);
    return result;
}

function validateInput(data) { return data != null; }
function sanitize(data) { return data.trim(); }
function checkPermissions(data) { return true; }
function transform(data) { return data.toUpperCase(); }

// === security_check_resource_leaks (CWE-772) ===

function leakyFileRead(path) {
    const fd = fs.openSync(path, 'r');
    const buf = Buffer.alloc(4096);
    fs.readSync(fd, buf);
    // fd never closed
    return buf.toString();
}

function leakyConnection(host) {
    const net = require('net');
    const socket = net.connect({ host, port: 80 });
    socket.write('GET / HTTP/1.1\r\n\r\n');
    // socket never closed
}

// === security_check_misconfig (CWE-16) ===

app.use((req, res, next) => {
    // CWE-16: CORS wildcard
    res.header('Access-Control-Allow-Origin', '*');
    next();
});

app.get('/api/data', (req, res) => {
    // CWE-614/1004: Insecure cookie
    res.cookie('session', 'abc123', { secure: false, httpOnly: false });
    res.json({ data: 'secret' });
});

// SSL bypass
// const agent = new https.Agent({ rejectUnauthorized: false });

// === security_check_input_validation (CWE-20) ===

app.get('/api/users/:id', (req, res) => {
    const userId = req.params.id;
    // Direct use in SQL without validation
    getUser(userId);
    res.json({ ok: true });
});

app.get('/api/files', (req, res) => {
    // CWE-22: Path traversal
    const filename = req.query.name;
    const data = fs.readFileSync('/data/' + filename, 'utf8');
    res.send(data);
});

// === security_check_error_exposure (CWE-209) ===

app.get('/api/process', (req, res) => {
    try {
        const result = JSON.parse(req.body);
        res.json(result);
    } catch (e) {
        // Leaks stack trace to client
        res.status(500).json({
            error: e.message,
            stack: e.stack
        });
    }
});

app.get('/api/login', (req, res) => {
    try {
        const user = req.query.user;
        if (hashPassword(req.query.pass) !== DB_PASSWORD) {
            throw new Error('Auth failed for ' + user);
        }
    } catch (e) {
        res.status(401).send(e.toString());
    }
});

// === Taint flow ===

app.get('/api/exec', (req, res) => {
    const cmd = req.query.cmd;
    runCommand(cmd);
    res.send('done');
});

app.listen(3000);
