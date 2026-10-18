# Carrefour CLI

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

A fast, unofficial command-line interface to shop at **Carrefour** (French supermarket) directly from your terminal. Search products, manage your cart, and select pickup/delivery slots without leaving the command line.

> **Disclaimer**: This is an unofficial tool. It is not affiliated with, endorsed by, or connected to Carrefour in any way. Use at your own risk. The tool relies on reverse-engineered APIs that may change without notice.

## Features

- **Product Search** - Search the Carrefour catalog with filters (brand, availability)
- **Cart Management** - Add, remove, update quantities, view cart summary
- **Time Slots** - View available pickup/delivery slots and select one
- **One-time Login** - Sign in once in a Chrome window; the session renews itself
- **43 Endpoints** - Orders, loyalty, receipts, shopping lists... via `carrefour api`
- **JSON Output** - Machine-readable output for scripting
- **Shell Completions** - Bash, Zsh, Fish, PowerShell, Elvish
- **Cloudflare Bypass** - Requests run inside a headless Chrome

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
- Google Chrome or Chromium (set `CARREFOUR_CHROME` if it is not in a standard location)
- A Carrefour.fr account with a store/pickup point selected (search works without one)

## Quick Start

### 1. Log In

```bash
carrefour auth login
```

A Chrome window opens on the Carrefour login page. Sign in yourself (captcha +
email code) and **leave the window open**: it closes by itself once the cookies
are captured. Carrefour's login is gated by a Turnstile captcha, so this step
cannot be automated.

Afterwards the CLI renews the storefront session on its own from the SSO cookie
(`c4iamsecuretk`): automatically on a 401/403, or with `carrefour auth refresh`.
The SSO cookie lives 24 h at most and dies after 60 min without activity — run
`carrefour auth refresh` from cron every 30 min to keep it alive.

Prefer a browser UI? `carrefour auth serve` opens a local page on
http://127.0.0.1:8765 to sign in, check the session and copy/download/import
the cookies (bound to localhost, API calls need a custom header so other sites
cannot drive it).

Already have cookies? `carrefour auth cookies --cookies '<header, JSON map or JSON array>'`
(only a JSON array export carries the domain needed for the SSO cookie).

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

### Any Endpoint

The CLI embeds the 43 endpoint descriptions of
[CarrefourDriveMCP](https://github.com/maximeallanic/CarrefourDriveMCP) (`tools/*.json`):

```bash
carrefour api                                   # list (* = needs login)
carrefour api get_loyalty_order_receipts --describe
carrefour api get_orders | jq '.'
carrefour api get_products_by_gtins gtins=3276554163158,3533631469573
```

`submit_checkout_payment` charges a real payment and requires `--yes`.
Adding an endpoint = dropping a JSON file in `tools/` and rebuilding.

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

### 401 / "Session expired" Errors

The SSO session expired (24 h max, 60 min idle). Sign in again:

```bash
carrefour auth login
```

### "No store selected" Error

You need to select a store on carrefour.fr before using the CLI:

1. Go to https://www.carrefour.fr
2. Click on the store selector (top of page)
3. Choose a Drive/pickup location
4. Run `carrefour auth refresh`

### Product Not Found

The CLI uses EAN (barcode) numbers. Make sure you're using the correct EAN from search results, not a product URL or name.

## How It Works

This CLI interacts with the same APIs that the Carrefour website uses.

- **Cloudflare**: plain HTTP clients (curl, reqwest, even TLS-impersonating ones)
  get `403 cf-mitigated: challenge`. So every request is a `fetch` issued from a
  page of a headless Chrome (UA masked, `AutomationControlled` disabled), driven
  over the DevTools protocol.
- **Login**: the login window is a plain Chrome with nothing attached (Turnstile
  rejects automated browsers); the CLI polls its tab list and reads the cookies
  once the OAuth loop lands back on carrefour.fr.
- **Renewal**: replaying `moncompte.carrefour.fr/iam/oauth2/.../authorize` →
  `www.carrefour.fr/login/check` mints fresh storefront cookies from the SSO cookie.
- Cookies are kept in `cookies.json` (mode 0600) next to `config.json`.

Auth flow and endpoint specs come from
[CarrefourDriveMCP](https://github.com/maximeallanic/CarrefourDriveMCP) (MIT).

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
