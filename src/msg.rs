use std::fmt::Binary;
use cosmwasm_std::Coin;
use cw721::Cw721ReceiveMsg;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub cw20_code_id: u64,
    pub cw20_msg: Binary,
    pub cw20_label: String,
    pub cw721_code_id: u64,
    pub cw721_msg: Binary,
    pub cw721_label: String
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    /// Create an nft and sell
    CreateSell {},
    /// Place your bid
    PlaceBid{},
    /// Retire all your bids
    RetireBids{},
    /// Owner can withdraw the nft at the end of the sale
    OwnerWithdrawNft{},
    /// This accepts a properly-encoded ReceiveMsg from a cw721 contract
    Receive(Cw721ReceiveMsg),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReceiveMsg {
    /// Sell your nft
    SellNft {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    // GetCount returns the current count as a json-encoded number
    GetCount {},
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CountResponse {
    pub count: i32,
}
