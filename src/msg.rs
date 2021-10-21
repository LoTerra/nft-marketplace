use crate::state::{CharityInfo, NftCreatorInfo};
use cosmwasm_std::{Coin, Uint128};
use cw20::Cw20ReceiveMsg;
use cw721::Cw721ReceiveMsg;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::Binary;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub cw20_code_id: u64,
    pub cw20_msg: Binary,
    pub cw20_label: String,
    pub cw721_code_id: u64,
    pub cw721_msg: Binary,
    pub cw721_label: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    /// Create an NFT and sell
    CreateMintAuction {},
    /// Place your bid
    PlaceBid {},
    /// Retire all your bids
    RetireBids {},
    /// Owner can withdraw the NFT at the end of the sale
    WithdrawNft {},
    /// Instant buy if allowed on the sale
    InstantBuy { auction_id: u64},
    /// This accepts a properly-encoded ReceiveMsg from a cw721 contract
    ReceiveCw721(Cw721ReceiveMsg),
    /// This accepts a properly-encoded ReceiveMsg from a cw20 contract
    ReceiveCw20(Cw20ReceiveMsg),
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
    RegisterPrivateSale {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    // GetCount returns the current count as a json-encoded number
    GetCount {},
    /// Get on sale NFT's
    OnSale {},
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CountResponse {
    pub count: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CharityResponse {
    pub address: String,
    pub fee_percentage: u8,
}
