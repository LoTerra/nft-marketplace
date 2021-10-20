use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, CanonicalAddr, Uint128};
use cw_storage_plus::{Item, Map};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub count: i32,
    pub owner: Addr,
    pub count_bets: u64,
}

pub const STATE: Item<State> = Item::new("state");


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CharityInfo {
    pub address: CanonicalAddr,
    pub fee_percentage: u8,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ItemInfo {
    pub bet_id: u64,
    pub owner: CanonicalAddr,
    pub start_price: Uint128,
    pub reserve_price: Uint128,
    pub start_time: u64,
    pub end_time: u64,
    pub highest_bid: Uint128,
    pub highest_bidder: CanonicalAddr,
    pub nft_contract: CanonicalAddr,
    pub nft_id: u64,
    pub private_sale_privilege: Option<Uint128>,
    pub total_bids: u64,
    pub charity: Option<CharityInfo>
}

pub const ITEMS: Map<&[u8], ItemInfo> = Map::new("items");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BidAmountTimeInfo {
    pub amount: Uint128,
    pub time: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BidInfo {
    pub bids: Vec<BidAmountTimeInfo>,
    pub bid_counter: u64,
    pub total_bid: Uint128,
    pub refunded: bool,
    pub privilege_used: Uint128
}

pub const BIDS: Map<(&[u8], &[u8]), BidInfo> = Map::new("bids");
