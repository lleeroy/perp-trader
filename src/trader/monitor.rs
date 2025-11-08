#![allow(unused)]

use crate::{
    error::TradingError, storage::{storage_position::PositionStorage, storage_strategy::StrategyStorage}, trader::{
        wallet::Wallet,
    }
};

use std::collections::HashMap;
use sqlx::PgPool;



/// Strategy monitor that checks and closes strategies
pub struct StrategyMonitor {
    position_storage: PositionStorage,
    strategy_storage: StrategyStorage,
    wallets: HashMap<u8, Wallet>,
}

impl StrategyMonitor {
    /// Create a new strategy monitor
    pub async fn new(
        pool: PgPool,
        wallets: Vec<Wallet>,
    ) -> Result<Self, TradingError> {
        let position_storage = PositionStorage::new(pool.clone()).await?;
        let strategy_storage = StrategyStorage::new(pool).await?;
        
        let wallets_map = wallets.into_iter()
            .map(|w| (w.id, w))
            .collect();
        
        Ok(Self {
            position_storage,
            strategy_storage,
            wallets: wallets_map,
        })
    }
}

