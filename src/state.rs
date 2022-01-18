use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{CanonicalAddr, Decimal, Uint128};
use cw_storage_plus::{Item, Map};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub denom: String,
    pub bid_margin: Decimal,
    pub lota_fee: Decimal,
    pub lota_fee_low: Decimal,
    pub lota_contract: CanonicalAddr,
    pub sity_full_rewards: Decimal,
    pub sity_partial_rewards: Decimal,
    pub sity_fee_registration: Decimal,
    pub sity_min_opening: Uint128,
}
pub const CONFIG: Item<Config> = Item::new("config");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub counter_items: u64,
    pub cw20_address: CanonicalAddr,
}

pub const STATE: Item<State> = Item::new("state");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CharityInfo {
    pub address: CanonicalAddr,
    pub fee_percentage: Decimal,
}

/*
   TODO: Should we ask for a collateral for selling ? in order to limit spam
*/
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ItemInfo {
    pub creator: CanonicalAddr,
    pub start_price: Option<Uint128>,
    pub start_time: u64,
    pub end_time: u64,
    pub highest_bid: Option<Uint128>,
    pub highest_bidder: Option<CanonicalAddr>,
    pub nft_contract: CanonicalAddr,
    pub nft_id: String,
    pub total_bids: u64,
    pub charity: Option<CharityInfo>,
    pub instant_buy: Option<Uint128>,
    pub reserve_price: Option<Uint128>,
    pub private_sale: bool,
    pub resolved: bool,
}

pub const ITEMS: Map<&[u8], ItemInfo> = Map::new("items");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BidInfo {
    pub bid_counter: u64,
    pub total_bid: Uint128,
    pub sity_used: Option<Uint128>,
    pub resolved: bool,
}

pub const BIDS: Map<(&[u8], &[u8]), BidInfo> = Map::new("bids");

/*
   History bidder info
*/

/*
  Bids stats
*/

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct HistoryBidInfo {
    pub bidder: CanonicalAddr,
    pub amount: Uint128,
    pub time: u64,
    pub instant_buy: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct HistoryBidderInfo {
    pub bids: Vec<HistoryBidInfo>,
}

pub const HISTORIES_BIDDER: Map<(&[u8], &[u8]), HistoryInfo> = Map::new("histories_bidder");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct HistoryInfo {
    pub bids: Vec<HistoryBidInfo>,
}

pub const HISTORIES: Map<&[u8], HistoryInfo> = Map::new("histories");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RoyaltyInfo {
    pub creator: CanonicalAddr,
    pub fee: Decimal,
    pub recipient: Option<CanonicalAddr>,
}
pub const ROYALTY: Map<&[u8], RoyaltyInfo> = Map::new("royalty");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct TalisInfo {
    pub minter: Option<String>,
    pub max_supply: Option<u64>,
}

/*
  User bid stats
*/

// #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
// pub struct UserInfo {
//     pub bidding_stats: u64,
//     pub privilege_used_stats: Uint128,
//     pub winning_auctions_stats: u64,
//     pub created_auctions_stats: u64,
//     pub auctions_stats: u64,
//     pub total_spend_stats: Uint128,
// }
//
// pub const USERS: Map<&[u8], UserInfo> = Map::new("users");
