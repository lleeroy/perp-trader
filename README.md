# Perp Trader - Automated Perpetual DEX Points Farming

A robust Rust application that automatically farms points on multiple perpetual DEX platforms by opening hedged long/short positions across different exchanges.

## Features

- **Atomic Position Opening**: Simultaneously opens LONG on Backpack and SHORT on Hibachi with automatic rollback on failure
- **Risk Management**: Monitors liquidation risk and alerts when collateral ratio falls below threshold
- **Real-time Monitoring**: Tracks positions every 60 seconds, calculates PnL, and checks for divergence
- **Graceful Degradation**: Continues operation even if one exchange fails
- **Database Persistence**: SQLite database for tracking all positions and trades
- **Configurable Parameters**: Flexible leverage, duration, and risk thresholds
- **Automated Position Closing**: Automatically closes positions when duration expires

## Architecture

```
src/
├── config.rs           # Configuration management
├── db/                 # Database layer
│   ├── mod.rs         # Pool initialization
│   ├── repository.rs  # Database operations
│   └── schema.rs      # SQL schema definitions
├── error.rs           # Error types
├── model/             # Data models
│   ├── exchange.rs    # Exchange enum
│   └── position.rs    # Position and hedge pair models
├── perp/              # Exchange implementations
│   ├── traits.rs      # PerpExchange trait
│   ├── backpack/      # Backpack client
│   └── hibachi/       # Hibachi client
├── trader/            # Trading logic
│   ├── hedge_manager.rs    # Atomic position opening
│   ├── monitor.rs          # Position monitoring
│   ├── closer.rs           # Automatic closing
│   └── orchestrator.rs     # Main coordinator
└── main.rs            # Application entry point
```

## Configuration

### Environment Variables

Copy `.env.example` to `.env` and configure:

```bash
# Logging
RUST_LOG=info

# Database
APP__DATABASE__URL=sqlite://perp_trader.db

# Trading parameters
APP__TRADING__MIN_LEVERAGE=2.0
APP__TRADING__MAX_LEVERAGE=3.0
APP__TRADING__MIN_DURATION_HOURS=4
APP__TRADING__MAX_DURATION_HOURS=8
APP__TRADING__MIN_COLLATERAL_RATIO=1.5
APP__TRADING__MAX_PNL_DIVERGENCE=0.05

# Exchange credentials
APP__EXCHANGES__BACKPACK__API_KEY=your_key
APP__EXCHANGES__BACKPACK__API_SECRET=your_secret
APP__EXCHANGES__HIBACHI__API_KEY=your_key
APP__EXCHANGES__HIBACHI__API_SECRET=your_secret
```

### Configuration File

Alternatively, use `config.toml` (copy from `config.example.toml`):

```toml
[trading]
min_leverage = 2.0
max_leverage = 3.0
min_duration_hours = 4
max_duration_hours = 8

[exchanges.backpack]
api_key = "your_key"
api_secret = "your_secret"
```

## Installation

```bash
# Clone the repository
git clone <repository-url>
cd perp-trader

# Install dependencies
cargo build --release

# Set up configuration
cp config.example.toml config.toml
# Edit config.toml with your API keys

# Or use environment variables
cp .env.example .env
# Edit .env with your API keys
```

## Usage

```bash
# Run the application
cargo run --release

# Or with custom log level
RUST_LOG=debug cargo run --release
```

## How It Works

### 1. Position Opening

- Generates random leverage (2-3x) and duration (4-8h)
- Validates sufficient collateral on both exchanges
- Atomically opens LONG on Backpack and SHORT on Hibachi
- If one fails, rolls back the other
- Saves positions to database

### 2. Position Monitoring

- Checks all open positions every 60 seconds
- Updates current prices from exchanges
- Calculates unrealized PnL
- Checks collateral ratio for liquidation risk
- Alerts if ratio < 150%
- Monitors PnL divergence between hedge pairs

### 3. Position Closing

- Automatically closes positions when duration expires
- Closes both legs simultaneously
- Implements graceful degradation if one exchange fails
- Logs realized PnL and points earned
- Updates database with final results

## Database Schema

### hedge_pairs
- Tracks paired positions across exchanges
- Stores leverage, size, and duration
- Records total PnL and points earned

### positions
- Individual positions on each exchange
- Tracks entry price, current price, and PnL
- Links to hedge_pair via foreign key
- Stores exchange-specific position IDs

## Risk Management

- **Leverage Control**: Random 2-3x (configurable)
- **Duration Limits**: 4-8 hours (configurable)
- **Collateral Monitoring**: Alerts at 150% ratio
- **PnL Divergence**: Alerts at 5% divergence
- **Atomic Operations**: All-or-nothing position opening
- **Graceful Degradation**: Continues on partial failures

## API Implementation Status

### Backpack & Hibachi Clients
- ✅ Client structure and trait implementation
- ⚠️  API calls are placeholder implementations
- 🔧 TODO: Implement actual REST API calls
- 🔧 TODO: Add authentication/signing
- 🔧 TODO: Handle rate limiting

To complete the integration:
1. Implement real API calls in `backpack/client.rs` and `hibachi/client.rs`
2. Add proper authentication headers
3. Handle exchange-specific response formats
4. Implement error handling and retries

## Development

```bash
# Run tests
cargo test

# Check code
cargo check

# Format code
cargo fmt

# Lint
cargo clippy
```

## Safety & Best Practices

- Uses SQLite with WAL mode for concurrent access
- Connection pooling for efficient database usage
- Structured error handling with `thiserror`
- Async/await with Tokio runtime
- Type-safe decimal arithmetic with `rust_decimal`
- Configurable parameters without code changes

## License

MIT

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

