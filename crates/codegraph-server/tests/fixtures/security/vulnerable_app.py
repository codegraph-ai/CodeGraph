"""
Intentionally vulnerable Python app for security scanner testing.
Every function triggers at least one CWE detection.
"""
import os
import pickle
import hashlib
import subprocess
import sqlite3
import yaml
import logging
from flask import Flask, request, jsonify, make_response

app = Flask(__name__)
DEBUG = True  # CWE-16: Security misconfiguration

# CWE-798: Hardcoded credentials
DB_PASSWORD = "super_secret_password_123"
API_KEY = "sk-1234567890abcdef1234567890abcdef"


# === security_scan patterns ===

def unsafe_query(user_input):
    """CWE-89: SQL Injection"""
    conn = sqlite3.connect("app.db")
    cursor = conn.cursor()
    cursor.execute("SELECT * FROM users WHERE name = '" + user_input + "'")
    return cursor.fetchall()


def run_command(cmd):
    """CWE-78: OS Command Injection"""
    os.system(cmd)
    result = subprocess.Popen(cmd, shell=True)
    return result


def render_unsafe(user_html):
    """CWE-94: Code Injection"""
    eval(user_html)
    exec("print(" + user_html + ")")


def load_untrusted(data):
    """CWE-502: Deserialization"""
    obj = pickle.loads(data)
    config = yaml.load(data)
    return obj, config


def weak_hash(password):
    """CWE-328: Weak hash"""
    return hashlib.md5(password.encode()).hexdigest()


def weak_random():
    """CWE-338: Weak PRNG"""
    import random
    return random.random()


# === security_check_unchecked_returns (CWE-252) ===

def process_data(data):
    """Return values silently ignored"""
    validate_input(data)
    sanitize(data)
    check_permissions(data)
    result = transform(data)
    return result


def validate_input(data):
    return data is not None


def sanitize(data):
    return data.strip()


def check_permissions(data):
    return True


def transform(data):
    return data.upper()


# === security_check_resource_leaks (CWE-772) ===

def leaky_file_read(path):
    """Opens file without closing"""
    f = open(path, 'r')
    data = f.read()
    return data


def leaky_connection(host):
    """Opens socket without closing"""
    import socket
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.connect((host, 80))
    s.send(b"GET / HTTP/1.1\r\n\r\n")
    return s.recv(4096)


def leaky_db(dsn):
    """Opens DB connection without closing"""
    conn = sqlite3.connect(dsn)
    cursor = conn.cursor()
    cursor.execute("SELECT 1")
    return cursor.fetchone()


# === security_check_misconfig (CWE-16) ===

@app.route("/api/data")
def cors_misconfigured():
    """CORS wildcard + insecure cookies"""
    response = make_response(jsonify({"data": "secret"}))
    response.headers["Access-Control-Allow-Origin"] = "*"
    response.set_cookie("session", "abc123", secure=False, httponly=False)
    return response


def ssl_bypass():
    """SSL verification disabled"""
    import requests
    return requests.get("http://internal-api.local/data", verify=False)


# === security_check_input_validation (CWE-20) ===

@app.route("/api/users/<user_id>")
def get_user(user_id):
    """Parameter used without validation"""
    conn = sqlite3.connect("app.db")
    cursor = conn.cursor()
    cursor.execute("SELECT * FROM users WHERE id = " + user_id)
    return jsonify(cursor.fetchone())


@app.route("/api/files")
def read_file():
    """Path traversal — unvalidated file path"""
    filename = request.args.get("name")
    with open("/data/" + filename, "r") as f:
        return f.read()


# === security_check_error_exposure (CWE-209) ===

@app.route("/api/process")
def process_request():
    """Stack trace leaked to user"""
    try:
        data = request.get_json()
        result = unsafe_query(data["query"])
        return jsonify(result)
    except Exception as e:
        import traceback
        return jsonify({
            "error": str(e),
            "trace": traceback.format_exc(),
            "debug": repr(e)
        }), 500


@app.route("/api/login")
def login():
    """Internal error details in response"""
    try:
        user = request.args.get("user")
        password = request.args.get("pass")
        if weak_hash(password) != DB_PASSWORD:
            raise ValueError("Auth failed for " + user)
    except Exception as e:
        return str(e), 401


# === Taint flow: user input → dangerous sink ===

@app.route("/api/exec")
def execute():
    """Full taint chain: request → command execution"""
    cmd = request.args.get("cmd")
    run_command(cmd)
    return "done"


@app.route("/api/search")
def search():
    """Full taint chain: request → SQL query"""
    query = request.args.get("q")
    results = unsafe_query(query)
    return jsonify(results)


if __name__ == "__main__":
    app.run(debug=True, host="0.0.0.0")
