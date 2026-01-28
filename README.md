# Carrefour CLI

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

A fast, unofficial command-line interface to shop at **Carrefour** (French supermarket) directly from your terminal. Search products, manage your cart, and select pickup/delivery slots without leaving the command line.

> **Disclaimer**: This is an unofficial tool. It is not affiliated with, endorsed by, or connected to Carrefour in any way. Use at your own risk. The tool relies on reverse-engineered APIs that may change without notice.

## Features

- **Product Search** - Search the Carrefour catalog with filters (brand, availability)
- **Cart Management** - Add, remove, update quantities, view cart summary
- **Time Slots** - View available pickup/delivery slots and select one
- **Multiple Auth Methods** - Browser cookies or automated via FlareSolverr
- **JSON Output** - Machine-readable output for scripting
- **Shell Completions** - Bash, Zsh, Fish, PowerShell, Elvish
- **Cloudflare Bypass** - Built-in browser impersonation using [rquest](https://github.com/penumbra-x/rquest)

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/misnard/rs-carrefour-cli.git
cd carrefour-cli

# Build in release mode
cargo build --release

# The binary will be at ./target/release/carrefour
# Optionally, copy it to your PATH
cp target/release/carrefour ~/.local/bin/
```

### Requirements

- Rust 1.70+ (for building)
- A Carrefour.fr account with a store/pickup point selected
- Browser cookies OR [FlareSolverr](https://github.com/FlareSolverr/FlareSolverr) for authentication

## Quick Start

### 1. Authentication Setup

The Carrefour API is protected by Cloudflare. You need to provide valid browser cookies.

#### Option A: Using FlareSolverr (Recommended for automation)

```bash
# Start FlareSolverr (requires Docker)
docker run -d -p 8191:8191 ghcr.io/flaresolverr/flaresolverr:latest

# Fetch cookies automatically
carrefour auth flaresolverr

# Note: This creates an anonymous session. For full functionality,
# log into carrefour.fr in your browser first, then use Option B.
```

#### Option B: Browser Cookies (Full account access)

1. Open https://www.carrefour.fr in your browser
2. Log in to your account
3. Select your preferred store/pickup point
4. Open DevTools (`F12`) > Application > Cookies > www.carrefour.fr
5. Copy all cookie values as a string: `name1=value1; name2=value2; ...`
6. Set cookies in the CLI:

```bash
carrefour auth cookies --cookies "FRONTONE_SESSID=xxx; __cf_bm=xxx; ..."
```

### 2. Verify Authentication

```bash
carrefour auth status
```

### 3. Start Shopping!

```bash
# Search for products
carrefour search "milk"

# Add to cart (use EAN from search results)
carrefour cart add 3560070837984

# View your cart
carrefour cart show

# Check available pickup slots
carrefour slots show

# Select a slot
carrefour slots select <SLOT_REF>
```

## Usage

### Search Products

```bash
# Basic search
carrefour search "pasta"

# Limit results
carrefour search "cheese" --limit 5

# Filter by brand
carrefour search "yogurt" --brand Danone

# Only show available products
carrefour search "bread" --available

# JSON output (for scripting)
carrefour search "coffee" --format json
```

### Cart Management

```bash
# View cart
carrefour cart show
carrefour cart show --format json

# Add product by EAN (barcode)
carrefour cart add 3560070837984
carrefour cart add 3560070837984 --quantity 3

# Update quantity
carrefour cart update 3560070837984 --quantity 5

# Remove product
carrefour cart remove 3560070837984

# Clear entire cart
carrefour cart clear
```

### Time Slots

```bash
# View available slots
carrefour slots show
carrefour slots show --format json

# Select a slot (use ref from slots show)
carrefour slots select abc12345-6789-...
```

### Shell Completions

```bash
# Bash
carrefour completions bash > ~/.local/share/bash-completion/completions/carrefour

# Zsh
carrefour completions zsh > ~/.zfunc/_carrefour

# Fish
carrefour completions fish > ~/.config/fish/completions/carrefour.fish

# PowerShell
carrefour completions powershell >> $PROFILE
```

### Configuration

```bash
# Show current config
carrefour config
```

Config is stored at:
- Linux: `~/.config/carrefour-cli/config.json`
- macOS: `~/Library/Application Support/carrefour-cli/config.json`
- Windows: `%APPDATA%\carrefour-cli\config.json`

## Troubleshooting

### 403 Forbidden Errors

Cloudflare cookies expire frequently. Re-authenticate:

```bash
carrefour auth flaresolverr
# or get fresh cookies from your browser
```

### "No store selected" Error

You need to select a store on carrefour.fr before using the CLI:

1. Go to https://www.carrefour.fr
2. Click on the store selector (top of page)
3. Choose a Drive/pickup location
4. Re-copy cookies to the CLI

### Product Not Found

The CLI uses EAN (barcode) numbers. Make sure you're using the correct EAN from search results, not a product URL or name.

## How It Works

This CLI interacts with the same APIs that the Carrefour website uses. It uses:

- **rquest** - An HTTP client with browser TLS fingerprint impersonation to bypass Cloudflare bot detection
- **Session cookies** - To maintain authentication state
- The autocomplete API for product search
- The cart API for basket management
- The timeslots API for delivery/pickup scheduling

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## Legal Notice

This project is for educational purposes. It is not affiliated with Carrefour. The Carrefour name and logo are trademarks of Carrefour S.A.

By using this tool, you agree to Carrefour's Terms of Service. The maintainers are not responsible for any misuse or for any consequences arising from the use of this tool.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
