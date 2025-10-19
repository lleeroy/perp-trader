use chrono::Utc;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::model::{HedgePair, Position, PositionStatus};

/// In-memory storage for hedge pairs and positions
#[derive(Clone)]
pub struct PositionStorage {
    hedge_pairs: Arc<Mutex<HashMap<String, HedgePair>>>,
    positions: Arc<Mutex<HashMap<String, Position>>>,
}

impl PositionStorage {
    pub fn new() -> Self {
        Self {
            hedge_pairs: Arc::new(Mutex::new(HashMap::new())),
            positions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // Hedge Pair methods
    pub fn insert_hedge_pair(&self, pair: HedgePair) {
        let mut pairs = self.hedge_pairs.lock().unwrap();
        pairs.insert(pair.id.clone(), pair);
    }

    pub fn get_hedge_pair(&self, id: &str) -> Option<HedgePair> {
        let pairs = self.hedge_pairs.lock().unwrap();
        pairs.get(id).cloned()
    }

    pub fn get_open_hedge_pairs(&self) -> Vec<HedgePair> {
        let pairs = self.hedge_pairs.lock().unwrap();
        pairs
            .values()
            .filter(|p| p.status == PositionStatus::Open)
            .cloned()
            .collect()
    }

    pub fn get_expiring_hedge_pairs(&self) -> Vec<HedgePair> {
        let now = Utc::now();
        let pairs = self.hedge_pairs.lock().unwrap();
        pairs
            .values()
            .filter(|p| p.status == PositionStatus::Open && p.close_at <= now)
            .cloned()
            .collect()
    }

    pub fn update_hedge_pair(&self, pair: HedgePair) {
        let mut pairs = self.hedge_pairs.lock().unwrap();
        pairs.insert(pair.id.clone(), pair);
    }

    // Position methods
    pub fn insert_position(&self, position: Position) {
        let mut positions = self.positions.lock().unwrap();
        positions.insert(position.id.clone(), position);
    }

    pub fn get_position(&self, id: &str) -> Option<Position> {
        let positions = self.positions.lock().unwrap();
        positions.get(id).cloned()
    }

    pub fn get_open_positions(&self) -> Vec<Position> {
        let positions = self.positions.lock().unwrap();
        positions
            .values()
            .filter(|p| p.status == PositionStatus::Open)
            .cloned()
            .collect()
    }

    pub fn get_positions_by_hedge_pair(&self, hedge_pair_id: &str) -> Vec<Position> {
        let positions = self.positions.lock().unwrap();
        positions
            .values()
            .filter(|p| p.hedge_pair_id == hedge_pair_id)
            .cloned()
            .collect()
    }

    pub fn update_position(&self, position: Position) {
        let mut positions = self.positions.lock().unwrap();
        positions.insert(position.id.clone(), position);
    }
}

