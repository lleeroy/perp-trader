# Perp Trader

Automated perpetual futures trading bot implementing market-neutral strategies for point farming on decentralized exchanges.

## Tech Stack

- **Language:** Rust (async/await, tokio runtime)
- **Databases:** PostgreSQL (SQLx), MongoDB
- **APIs:** REST, WebSocket, EIP-191 signatures
- **Crypto:** AES-256-GCM encryption, Argon2id key derivation, Ethereum/Solana signing
- **Infrastructure:** Docker, Fly.io, Telegram alerts

## Architecture

```
src/
├── trader/         # Trading orchestration & multi-wallet management
│   ├── client.rs   # Strategy execution, position monitoring
│   ├── strategy.rs # Market-neutral allocation algorithms
│   └── wallet.rs   # Encrypted credential management
├── perp/           # Exchange integrations (trait-based)
│   ├── lighter/    # Lighter DEX client with auto API key registration
│   └── ranger/     # Ranger exchange client
├── storage/        # Persistence layer
│   ├── database.rs # MongoDB singleton with connection pooling
│   └── storage_*.rs# PostgreSQL repositories
├── model/          # Domain models (Position, Strategy, Token)
└── alert/          # Telegram notifications
```

## Key Features

- **Multi-wallet orchestration** - Parallel position management across 40+ wallets
- **Market-neutral strategies** - Balanced long/short allocations with configurable leverage
- **Liquidation monitoring** - Real-time position health checks with 13% threshold
- **Auto-recovery** - Failed strategy retry mechanism with exponential backoff
- **Secure key storage** - AES-256-GCM encrypted private keys with Argon2id derivation

## Configuration

```bash
cp .env.example .env
cp config.example.toml config.toml
# Edit files with your credentials
```

## Build & Run

```bash
# Development
cargo run

# Production
cargo build --release
./target/release/perp-trader

# Docker
docker build -t perp-trader .
```

## License

MIT
