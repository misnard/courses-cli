# Contributing to Courses CLI

Thank you for your interest in contributing to Courses CLI! This document provides guidelines and information for contributors.

## Code of Conduct

Be respectful and constructive. We're all here to build something useful together.

## How to Contribute

### Reporting Bugs

1. Check if the bug has already been reported in [Issues](https://github.com/misnard/courses-cli/issues)
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
git clone https://github.com/misnard/courses-cli.git
cd courses-cli

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
├── main.rs            # CLI entry point and argument parsing
├── browser.rs         # Headless Chrome driven over CDP: every request is an in-page fetch
├── config.rs          # config.json (picked stores) and the config directory
├── providers/
│   ├── mod.rs         # Shop trait, normalized Product/Cart/Store, Web helper
│   ├── carrefour.rs   # Carrefour adapter over client.rs
│   ├── leclerc.rs     # E.Leclerc Drive
│   └── ...            # one file per store chain
├── commands/
│   ├── shop.rs        # search / cart / store / auth, shared by every provider
│   ├── auth.rs        # Carrefour authentication
│   ├── api.rs         # Carrefour's 43 endpoints from tools/*.json
│   ├── serve.rs       # Carrefour local login page
│   └── slots.rs       # Carrefour time slots
├── client.rs          # Carrefour API client
├── session.rs         # Carrefour cookie jar, SSO renewal and login
├── spec.rs            # tools/*.json spec → request
├── models/mod.rs      # Carrefour response models
└── error.rs           # Carrefour errors
```

## Coding Guidelines

- Follow Rust idioms and best practices
- Use `cargo fmt` for formatting
- Use `cargo clippy` and fix warnings
- Write descriptive commit messages
- Keep changes focused and atomic
- Add comments for complex logic

## Adding a Provider

1. Implement `Shop` in `src/providers/<name>.rs` and register it in
   `src/providers/mod.rs` (`Provider` enum, `name()`, `open()`). Implement only
   what the site allows; the trait's defaults answer "not supported".
2. Use `Web` (`providers/mod.rs`): it gives the provider its own Chrome profile
   and cookie jar, and sends each request from a real page of the site, which
   bot protections such as DataDome require.
3. Document the endpoints (URL, method, headers, body) in the module comment,
   and credit the project you took them from.
4. Add a unit test that parses a small captured sample (no network), and
   test live with the CLI before submitting.
5. Be careful not to include personal data in examples.

## Security

- **NEVER** commit cookies, tokens, or credentials
- Mask personal data in examples and error messages
- Report security vulnerabilities privately

## Questions?

Open an issue with the "question" label if you need help!
