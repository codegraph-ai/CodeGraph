// Intentionally vulnerable Rust code for security scanner testing.
// Every function triggers at least one CWE detection.

use std::fs::File;
use std::io::Read;
use std::process::Command;

// CWE-798: Hardcoded credentials
const DB_PASSWORD: &str = "admin123";
const API_SECRET: &str = "sk_live_abcdef1234567890";

// === security_scan patterns ===

/// CWE-78: Command injection
fn run_user_command(input: &str) {
    let output = Command::new("sh")
        .arg("-c")
        .arg(input)
        .output()
        .expect("failed");
    println!("{:?}", output);
}

/// CWE-704: Unsafe transmute
fn dangerous_cast(bytes: &[u8]) -> &str {
    unsafe { std::mem::transmute(bytes) }
}

/// CWE-676: Unsafe block
unsafe fn raw_pointer_deref(ptr: *const u8) -> u8 {
    *ptr
}

/// CWE-676: Unwrap on Result
fn risky_parse(input: &str) -> i32 {
    input.parse::<i32>().unwrap()
}

// === security_check_unchecked_returns (CWE-252) ===

fn process_pipeline(data: &str) {
    validate(data);
    sanitize(data);
    check_auth(data);
    let result = transform(data);
    println!("{}", result);
}

fn validate(data: &str) -> bool { !data.is_empty() }
fn sanitize(data: &str) -> String { data.trim().to_string() }
fn check_auth(data: &str) -> Result<(), String> { Ok(()) }
fn transform(data: &str) -> String { data.to_uppercase() }

// === security_check_resource_leaks (CWE-772) ===

fn leaky_file_read(path: &str) -> String {
    let mut f = File::open(path).unwrap();
    let mut contents = String::new();
    f.read_to_string(&mut contents).unwrap();
    // f is not explicitly closed (Rust RAII handles it, but pattern is flagged)
    contents
}

fn leaky_tcp(addr: &str) -> Vec<u8> {
    use std::net::TcpStream;
    use std::io::Write;
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
    let mut buf = vec![0u8; 4096];
    stream.read(&mut buf).unwrap();
    buf
}

// === security_check_misconfig (CWE-16) ===

fn insecure_tls() {
    // CWE-295: SSL verification disabled
    // reqwest::Client::builder().danger_accept_invalid_certs(true)
    let _debug = true; // debug mode
}

fn cors_handler() -> &'static str {
    // Access-Control-Allow-Origin: *
    "Access-Control-Allow-Origin: *"
}

// === security_check_input_validation (CWE-20) ===

fn get_item(items: &[String], index: usize) -> &str {
    // Direct array access without bounds check
    &items[index]
}

fn build_path(user_input: &str) -> String {
    // Path traversal: unvalidated user input in file path
    format!("/data/{}", user_input)
}

// === security_check_error_exposure (CWE-209) ===

fn handle_request(input: &str) -> String {
    match risky_parse(input) {
        result => format!("OK: {}", result),
    }
}

fn api_error_handler(err: &dyn std::error::Error) -> String {
    // Leaks internal error details
    format!("Error: {}", err.to_string())
}

fn main() {
    run_user_command("ls");
    let _ = dangerous_cast(b"hello");
}
