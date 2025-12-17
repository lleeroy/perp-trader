# Perp Trader

Automated trading bot for perpetual futures on decentralized exchanges. Executes market-neutral strategies (balanced long/short positions) to minimize directional exposure while capturing funding rates.

## What it does

- Opens balanced long/short positions with configurable wallet management
- Monitors positions for liquidation risk (closes if within 13% of liquidation price)
- Auto-closes positions after configurable time period (4-8 hours)
- Sends Telegram alerts on failures
- Persists all positions/strategies to PostgreSQL for recovery

## Tech Stack

- **Rust** with Tokio async runtime
- **PostgreSQL** (SQLx) for positions & strategies
- **MongoDB** for wallet accounts
- **AES-256-GCM + Argon2id** for encrypting private keys at rest

## Architecture

```
src/
├── trader/           # Trading logic
│   ├── client.rs     # Main orchestrator, manages position lifecycle
│   ├── strategy.rs   # Allocation algorithm (balances long/short)
│   └── wallet.rs     # Loads encrypted credentials
├── perp/             # Exchange clients
│   ├── traits.rs     # PerpExchange trait
│   ├── lighter/      # Lighter DEX implementation
│   └── ranger/       # Ranger exchange
├── storage/          # Database layer
└── alert/            # Telegram notifications
```

## How it works

1. **Position sizing** — Calculates optimal position sizes based on available balance
2. **Allocation** — Assigns positions to long/short sides, ensuring `|total_long - total_short| < $2` for market neutrality
3. **Execution** — Opens all positions in parallel via `futures::try_join_all`
4. **Monitoring** — Background task checks liquidation distance every 15s
5. **Closure** — Closes positions when scheduled time reached or liquidation risk detected

## Key implementation details

**Exchange abstraction:**
```rust
#[async_trait]
pub trait PerpExchange: Send + Sync {
    async fn open_position(&self, token: Token, side: PositionSide,
                           close_at: DateTime<Utc>, amount_usdc: Decimal) -> Result<Position, TradingError>;
    async fn close_position(&self, position: &Position) -> Result<Position, TradingError>;
    // ...
}
```

**Credential encryption:**
```rust
// Keys encrypted with AES-256-GCM, derived via Argon2id
// Format: [version:1B][salt:16B][nonce:12B][ciphertext]
pub fn encrypt_private_key(private_key_hex: &str, password: &str) -> Result<String>
```

## Setup

```bash
cp .env.example .env
cp config.example.toml config.toml
# Fill in your credentials

cargo run
```

## Config

```toml
[trading]
min_leverage = 2.0
max_leverage = 3.0
min_duration_hours = 4
max_duration_hours = 8

[monitoring]
check_interval_seconds = 60
```

## Deployment

Runs on Fly.io:
```bash
fly deploy
```

## License

MIT
