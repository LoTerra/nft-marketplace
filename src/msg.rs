use crate::state::{BidAmountTimeInfo, CharityInfo};
use cosmwasm_std::{Addr, Binary, Coin, Uint128};
use cw20::Cw20ReceiveMsg;
use cw721::Cw721ReceiveMsg;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub denom: String,
    pub cw20_code_id: u64,
    pub cw20_label: String,
    pub bid_margin: u8,
    pub lota_fee: u8,
    pub lota_contract: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    /// Place your bid
    PlaceBid { auction_id: u64 },
    /// Retire all your bids
    RetireBids { auction_id: u64 },
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
        charity: Option<CharityResponse>,
        instant_buy: Option<Uint128>,
        reserve_price: Option<Uint128>,
        private_sale_privilege: Option<Uint128>,
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
    /// Get config
    Config {},
    /// Get state
    State {},
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AuctionResponse {
    pub creator: String,
    pub start_price: Option<Uint128>,
    pub start_time: Option<u64>,
    pub end_time: u64,
    pub highest_bid: Option<Uint128>,
    pub highest_bidder: Option<String>,
    pub nft_contract: String,
    pub nft_id: String,
    pub total_bids: u64,
    pub charity: Option<CharityResponse>,
    pub instant_buy: Option<Uint128>,
    pub reserve_price: Option<Uint128>,
    pub private_sale_privilege: Option<Uint128>,
    pub resolved: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct BidResponse {
    pub bids: Vec<BidAmountTimeInfo>,
    pub bid_counter: u64,
    pub total_bid: Uint128,
    pub privilege_used: Option<Uint128>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CharityResponse {
    pub address: String,
    pub fee_percentage: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ConfigResponse {
    pub denom: String,
    pub bid_margin: u8,
    pub lota_fee: u8,
    pub lota_contract: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct StateResponse {
    pub counter_items: u64,
    pub cw20_address: String,
}
