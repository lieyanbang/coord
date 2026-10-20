# Common dev tasks for coord (Windows primary).
set shell := ["bash", "-cu"]
set windows-shell := ["pwsh.exe", "-NoLogo", "-NoProfile", "-Command"]
url := "http://127.0.0.1:7788/"
db := ".coord/dev.db"

default:
    @just --list

build:
    cargo build --locked

release:
    cargo build --release --locked

test:
    cargo test --all-features --locked

lint:
    cargo clippy --all-targets --all-features --locked -- -D warnings

fmt:
    cargo fmt --all

# Isolated dev port and persistent DB; stop using Ctrl+C. Never delete on start.
serve:
    cargo run --locked --bin coord -- serve --addr 127.0.0.1:7788 --db {{db}}

top:
    cargo run --locked --bin coord -- --url {{url}} top
