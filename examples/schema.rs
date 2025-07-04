use std::env::current_dir;
use std::fs::create_dir_all;

use cosmwasm_schema::{export_schema, remove_schemas, schema_for};

use marketplace::msg::{
    AllAuctionsResponse, AuctionResponse, BidResponse, CharityResponse, ConfigResponse, ExecuteMsg,
    HistoryBidResponse, HistoryResponse, InstantiateMsg, MigrateMsg, QueryMsg, RoyaltyResponse,
    StateResponse,
};
use marketplace::state::{Config, State};

fn main() {
    let mut out_dir = current_dir().unwrap();
    out_dir.push("schema");
    create_dir_all(&out_dir).unwrap();
    remove_schemas(&out_dir).unwrap();

    export_schema(&schema_for!(InstantiateMsg), &out_dir);
    export_schema(&schema_for!(ExecuteMsg), &out_dir);
    export_schema(&schema_for!(QueryMsg), &out_dir);
    export_schema(&schema_for!(State), &out_dir);
    export_schema(&schema_for!(Config), &out_dir);
    export_schema(&schema_for!(AuctionResponse), &out_dir);
    export_schema(&schema_for!(AllAuctionsResponse), &out_dir);
    export_schema(&schema_for!(BidResponse), &out_dir);
    export_schema(&schema_for!(CharityResponse), &out_dir);
    export_schema(&schema_for!(ConfigResponse), &out_dir);
    export_schema(&schema_for!(StateResponse), &out_dir);
    export_schema(&schema_for!(HistoryBidResponse), &out_dir);
    export_schema(&schema_for!(HistoryResponse), &out_dir);
    export_schema(&schema_for!(RoyaltyResponse), &out_dir);
    export_schema(&schema_for!(MigrateMsg), &out_dir);
}
