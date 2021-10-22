use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::ops::Add;

use cosmwasm_std::{Addr, CanonicalAddr, Uint128};
use cw_storage_plus::{Item, Map};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub denom: String,
    pub bid_margin: u8,
}
pub const CONFIG: Item<Config> = Item::new("config");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub counter_items: u64,
    pub cw20_address: CanonicalAddr,
    pub cw721_address: CanonicalAddr,
}

pub const STATE: Item<State> = Item::new("state");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CharityInfo {
    pub address: CanonicalAddr,
    pub fee_percentage: u8,
}

/*
   TODO: Should we limit with spread bid like percentage max to increase the current bid
   E.g
   Current bid 100
   Alice want to bid 1000 the percentage increase is 1000%
   it's probably better to limit
*/

/*
   TODO: Should we ask for a collateral for selling ? in order to limit spam
*/
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ItemInfo {
    pub creator: CanonicalAddr,
    pub start_price: Option<Uint128>,
    pub start_time: Option<u64>,
    pub end_time: u64,
    pub highest_bid: Option<Uint128>,
    pub highest_bidder: Option<CanonicalAddr>,
    pub nft_contract: CanonicalAddr,
    pub nft_id: String,
    pub total_bids: u64,
    pub charity: Option<CharityInfo>,
    pub instant_buy: Option<Uint128>,
    pub reserve_price: Option<Uint128>,
    pub private_sale_privilege: Option<Uint128>,
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
    pub privilege_used: Option<Uint128>,
}

pub const BIDS: Map<(&[u8], &[u8]), BidInfo> = Map::new("bids");


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UserInfo {
    pub bidding_stats: u64,
    pub privilege_used_stats: Uint128,
    pub winning_auctions_stats: u64,
    pub created_auctions_stats: u64,
    pub auctions_stats: u64,
    pub total_spend_stats: Uint128
}

pub const USERS: Map<&[u8], UserInfo> = Map::new("users");
