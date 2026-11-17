// Release builds run without a console window; debug builds keep it for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = rustpira::app::run() {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}
