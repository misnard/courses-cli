# Courses CLI

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.82%2B-orange.svg)](https://www.rust-lang.org/)

Do your grocery shopping ("faire les courses") at French drives from the
terminal: find a store, search products, fill the cart, then book the slot and
pay on the site.

> **Disclaimer**: This is an unofficial tool, not affiliated with, endorsed by,
> or connected to any of the retailers below. Use at your own risk. It relies on
> reverse-engineered APIs that may change without notice.

## Providers

| Provider | `--provider` | Store finder | Search | Cart | Login | Extras |
| --- | --- | --- | --- | --- | --- | --- |
| Carrefour | `carrefour` (default) | — | ✓ | ✓ | ✓ | slots, 43 endpoints (`api`), self-renewing session |
| E.Leclerc Drive | `leclerc` | ✓ | ✓ | ✓ | ✓ | |
| Chronodrive | `chronodrive` | | | | | in progress |
| Intermarché | `intermarche` | | | | | not supported: DataDome blocks headless Chrome |
| Auchan | `auchan` | | | | | in progress |
| Super U | `superu` | | | | | in progress |
| Casino proximité | `casino` | ✓ | ✓ | ✓ | ✓ (unverified) | Petit Casino, Casino Shop, Spar, Vival |

Pick the provider per command (`courses --provider leclerc search lait`) or once
for the shell (`export COURSES_PROVIDER=leclerc`).

## Installation

```bash
git clone https://github.com/misnard/rs-carrefour-cli.git
cd rs-carrefour-cli
cargo build --release
cp target/release/courses ~/.local/bin/
```

Requirements:

- Rust 1.82+ (for building)
- Google Chrome or Chromium (set `COURSES_CHROME` if it is not in a standard location)

## Quick Start

```bash
courses --provider leclerc store 44000                 # stores near a postal code or city
courses --provider leclerc store 44000 --pick 053701   # shop there (remembered)
courses --provider leclerc search "lait" --limit 5     # names, prices, stock, IDs
courses --provider leclerc cart add 2612 --quantity 2  # product ID from search
courses --provider leclerc cart show
```

## Usage

These commands work the same for every provider:

```bash
courses search "yogurt" --brand Danone --available --limit 5
courses search "coffee" --format json      # normalized JSON for scripting
courses search "nutella" --nutriscore      # fill missing Nutri-Scores from Open Food Facts
courses cart show [--format json]
courses cart add <ID> [--quantity 3]       # sets the quantity: not a delta
courses cart update <ID> --quantity 5
courses cart remove <ID>
courses cart clear
courses store [<postal code or city> [--pick <id>]]
courses auth login | status | logout
courses config
```

`cart add` fails when the cart does not end up holding the requested quantity
(unknown ID, not enough stock).

### Carrefour

```bash
courses auth login
```

A Chrome window opens on the Carrefour login page. Sign in yourself (captcha +
email code) and **leave the window open**: it closes by itself once the cookies
are captured. Carrefour's login is gated by a Turnstile captcha, so this step
cannot be automated.

Afterwards the CLI renews the storefront session on its own from the SSO cookie
(`c4iamsecuretk`): automatically on a 401/403, or with `courses auth refresh`.
The SSO cookie lives 24 h at most and dies after 60 min without activity: run
`courses auth refresh` from cron every 30 min to keep it alive.

Pick your store on carrefour.fr (store selector at the top of the page), then
run `courses auth refresh`. Product IDs are EANs.

Prefer a browser UI? `courses auth serve` opens a local page on
http://127.0.0.1:8765 to sign in, check the session and copy/download/import
the cookies (bound to localhost, API calls need a custom header so other sites
cannot drive it). Already have cookies?
`courses auth cookies --cookies '<header, JSON map or JSON array>'` (only a JSON
array export carries the domain needed for the SSO cookie; `CARREFOUR_COOKIES`
does the same from the environment).

Time slots:

```bash
courses slots show
courses slots select <SLOT_REF>
```

Any of the 43 endpoints of
[CarrefourDriveMCP](https://github.com/maximeallanic/CarrefourDriveMCP) (`tools/*.json`):

```bash
courses api                                   # list (* = needs login)
courses api get_loyalty_order_receipts --describe
courses api get_orders | jq '.'
courses api get_products_by_gtins gtins=3276554163158,3533631469573
```

`submit_checkout_payment` charges a real payment and requires `--yes`.
Adding an endpoint = dropping a JSON file in `tools/` and rebuilding.

### E.Leclerc Drive

Search and cart work without an account: the cart then belongs to the CLI's own
anonymous session. `courses --provider leclerc auth login` opens a Chrome window
on your drive: sign in there and press Enter, and the CLI keeps that signed-in
session, so you can book the slot and pay on leclercdrive.fr. The same window
clears a DataDome captcha if one ever blocks the CLI.

Endpoints come from [mcp-leclerc-drive](https://github.com/skunkobi/mcp-leclerc-drive) (MIT).

### Casino proximité

```bash
courses --provider casino store 75011              # Petit Casino, Casino Shop, Spar, Vival nearby
courses --provider casino store 75011 --pick TL324
courses --provider casino search lait
courses --provider casino cart add 10604230 -q 2
```

No account needed: the cart lives in the site's cookies for 90 days. Slot,
order and payment happen on mescoursesdeproximite.com. Endpoints partly from
[casino-proximite-mcp](https://github.com/ncleton/casino-proximite-mcp) (MIT).

### Nutri-Score from Open Food Facts

`courses search nutella --nutriscore` fills in the Nutri-Score (A to E) from
[Open Food Facts](https://world.openfoodfacts.org) for products the store shows
without one, by EAN, for every provider that returns EANs. It makes one batched
anonymous request, plus one per product missing from OFF's search index, and
only when something needs filling; without the flag, Open Food Facts is never
contacted. Data © Open Food Facts contributors, ODbL.

### Shell Completions

```bash
courses completions bash > ~/.local/share/bash-completion/completions/courses
courses completions zsh > ~/.zfunc/_courses
courses completions fish > ~/.config/fish/completions/courses.fish
courses completions powershell >> $PROFILE
```

### Configuration

Picked stores live in `config.json`; each provider keeps its own cookie jar
(`<provider>-cookies.json`, mode 0600) and Chrome profile (`<provider>-profile`)
next to it:

- Linux: `~/.config/courses-cli/`
- macOS: `~/Library/Application Support/courses-cli/`

`COURSES_CONFIG_DIR` points elsewhere (tests, several accounts). A
`carrefour-cli` directory from older versions is moved there on first run.

## Troubleshooting

- **401 / "Session expired"** (Carrefour): the SSO session expired (24 h max,
  60 min idle). Run `courses auth login` again.
- **"Blocked by the site's bot protection"**: run
  `courses --provider <name> auth login` and solve the captcha in the window.
- **"No store picked"**: run `courses --provider <name> store <postal code>`.
- **"Chrome did not start"**: another `courses` command is running for the same
  provider (each provider's Chrome profile is single-instance).

## How It Works

- **Bot protections**: plain HTTP clients (curl, reqwest, even TLS-impersonating
  ones) get `403 cf-mitigated: challenge` from Cloudflare (Carrefour) and a
  captcha from DataDome (Leclerc). So every request is a `fetch` issued from a
  page of a headless Chrome (UA masked with `--user-agent`,
  `AutomationControlled` disabled), driven over the DevTools protocol. Masking
  the UA through CDP instead leaves the client-hint brands empty, and DataDome
  answers that with a captcha.
- **Providers**: each one implements the `Shop` trait (`src/providers/`); the
  shared commands (`src/commands/shop.rs`) print what it returns.
- **Carrefour login**: the login window is a plain Chrome with nothing attached
  (Turnstile rejects automated browsers); the CLI polls its tab list and reads
  the cookies once the OAuth loop lands back on carrefour.fr. Replaying
  `moncompte.carrefour.fr/iam/oauth2/.../authorize` → `www.carrefour.fr/login/check`
  then mints fresh storefront cookies from the SSO cookie.

Carrefour auth flow and endpoint specs come from
[CarrefourDriveMCP](https://github.com/maximeallanic/CarrefourDriveMCP) (MIT).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Legal Notice

This project is for educational purposes. Carrefour, E.Leclerc, Chronodrive,
Intermarché, Auchan, Super U and Casino are trademarks of their respective
owners. By using this tool, you agree to each retailer's Terms of Service. The
maintainers are not responsible for any misuse or for any consequences arising
from the use of this tool.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
