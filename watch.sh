#!/bin/bash

# Ensure cargo watch is installed
if ! command -v cargo-watch &> /dev/null; then
    echo "cargo-watch is not installed. Please install it first."
    exit 1
fi

# Run cargo watch to monitor the content directory and execute `cargo run` on change
cargo watch -w content -w templates -x 'run --release'
