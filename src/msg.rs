use crate::state::Charity;
use cosmwasm_std::{Decimal, Uint128};
use cw20::Cw20ReceiveMsg;
use cw721::Cw721ReceiveMsg;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub denom: String,
    pub cw20_code_id: u64,
    pub cw20_label: String,
    pub bid_margin: Uint128,
    pub lota_fee: Uint128,
    pub lota_contract: String,
    pub sity_full_rewards: Uint128,
    pub sity_partial_rewards: Uint128,
    pub sity_fee_registration: Uint128,
    pub sity_min_opening: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    /// Place your bid
    PlaceBid { auction_id: u64 },
    /// Retire all your bids
    RetractBids { auction_id: u64 },
    /// Owner can withdraw the NFT at the end of the sale
    WithdrawNft { auction_id: u64 },
    /// Instant buy if allowed on the sale
    InstantBuy { auction_id: u64 },
    /// This accepts a properly-encoded ReceiveMsg from a cw721 contract
    ReceiveNft(Cw721ReceiveMsg),
    /// This accepts a properly-encoded ReceiveMsg from a cw20 contract
    Receive(Cw20ReceiveMsg),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReceiveMsg {
    /// Create an auction and sell your NFT
    CreateAuctionNft {
        start_price: Option<Uint128>,
        start_time: Option<u64>,
        end_time: u64,
        charity: Option<Charity>,
        instant_buy: Option<Uint128>,
        reserve_price: Option<Uint128>,
        private_sale: bool,
    },
    /// Register private sale
    RegisterPrivateSale { auction_id: u64 },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    /// Get auction by id
    Auction { auction_id: u64 },
    /// Get bid info by auction id and address of the bidder
    Bidder { auction_id: u64, address: String },
    /// Get bids history from an auction id
    HistoryBids { auction_id: u64 },
    /// Get bids history from an auction id
    HistoryBidderBids { auction_id: u64, address: String },
    /// Get config
    Config {},
    /// Get state
    State {},
    /// Get all auctions
    AllAuctions {
        start_after: Option<u64>,
        limit: Option<u32>,
    },
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AuctionResponse {
    pub creator: String,
    pub start_price: Option<Uint128>,
    pub start_time: u64,
    pub end_time: u64,
    pub highest_bid: Option<Uint128>,
    pub highest_bidder: Option<String>,
    pub nft_contract: String,
    pub nft_id: String,
    pub total_bids: u64,
    pub charity: Option<CharityResponse>,
    pub instant_buy: Option<Uint128>,
    pub reserve_price: Option<Uint128>,
    pub private_sale: bool,
    pub resolved: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BidResponse {
    pub bid_counter: u64,
    pub total_bid: Uint128,
    pub sity_used: Option<Uint128>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CharityResponse {
    pub address: String,
    pub fee_percentage: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ConfigResponse {
    pub denom: String,
    pub bid_margin: Decimal,
    pub lota_fee: Decimal,
    pub lota_contract: String,
    pub sity_full_rewards: Decimal,
    pub sity_partial_rewards: Decimal,
    pub sity_fee_registration: Decimal,
    pub sity_min_opening: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct StateResponse {
    pub counter_items: u64,
    pub cw20_address: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct HistoryBidResponse {
    pub bidder: String,
    pub amount: Uint128,
    pub time: u64,
    pub instant_buy: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct HistoryResponse {
    pub bids: Vec<HistoryBidResponse>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AllAuctionsResponse {
    pub auctions: Vec<(u64, AuctionResponse)>,
}
