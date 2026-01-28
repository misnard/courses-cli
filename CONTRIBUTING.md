# Contributing to Carrefour CLI

Thank you for your interest in contributing to Carrefour CLI! This document provides guidelines and information for contributors.

## Code of Conduct

Be respectful and constructive. We're all here to build something useful together.

## How to Contribute

### Reporting Bugs

1. Check if the bug has already been reported in [Issues](https://github.com/misnard/rs-carrefour-cli/issues)
2. If not, create a new issue with:
   - A clear, descriptive title
   - Steps to reproduce the bug
   - Expected vs actual behavior
   - Your environment (OS, Rust version)
   - Relevant error messages (redact any personal info!)

### Suggesting Features

1. Check existing issues for similar suggestions
2. Create a new issue describing:
   - The feature and its use case
   - Why it would be useful
   - Possible implementation approach (optional)

### Pull Requests

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/my-feature`
3. Make your changes
4. Ensure code compiles: `cargo build`
5. Run clippy: `cargo clippy`
6. Format code: `cargo fmt`
7. Commit with a clear message
8. Push and open a Pull Request

## Development Setup

```bash
# Clone your fork
git clone https://github.com/misnard/rs-carrefour-cli.git
cd carrefour-cli

# Build
cargo build

# Run
cargo run -- --help

# Run with arguments
cargo run -- search "test"
```

## Project Structure

```
src/
├── main.rs          # CLI entry point and argument parsing
├── client.rs        # HTTP client and API calls
├── config.rs        # Configuration management
├── error.rs         # Custom error types
├── models/
│   └── mod.rs       # Data models for API responses
└── commands/
    ├── mod.rs       # Command module exports
    ├── auth.rs      # Authentication commands
    ├── cart.rs      # Cart management commands
    ├── search.rs    # Product search
    └── slots.rs     # Time slot management
```

## Coding Guidelines

- Follow Rust idioms and best practices
- Use `cargo fmt` for formatting
- Use `cargo clippy` and fix warnings
- Write descriptive commit messages
- Keep changes focused and atomic
- Add comments for complex logic

## API Changes

If you discover new Carrefour API endpoints or changes:

1. Document the endpoint (URL, method, headers, body)
2. Test thoroughly before submitting
3. Note any authentication requirements
4. Be careful not to include personal data in examples

## Security

- **NEVER** commit cookies, tokens, or credentials
- Mask personal data in examples and error messages
- Report security vulnerabilities privately

## Questions?

Open an issue with the "question" label if you need help!
