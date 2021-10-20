use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, CanonicalAddr, Uint128};
use cw_storage_plus::{Item, Map};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub count: i32,
    pub owner: Addr,
    pub count_bet: u64
}

pub const STATE: Item<State> = Item::new("state");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BetInfo {
    pub bet_id: u64,
    pub owner: CanonicalAddr,
    pub description: String,
    pub start_price: Uint128,
    pub reserve_price: Uint128,
    pub start_time: u64,
    pub end_time: u64,
    pub highest_bid: Uint128,
    pub highest_bidder: CanonicalAddr
}

pub const BET: Map<&[u8], BetInfo> = Map::new("bet");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BidAmountTimeInfo {
    pub amount: Uint128,
    pub time: u64
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BidInfo {
    pub bid: Vec<BidAmountTimeInfo>,
}

pub const BIDDER: Map<(&[u8], &[u8]), BidInfo> = Map::new("bet");
