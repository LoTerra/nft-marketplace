use crate::msg::QueryTalisMsg;
use crate::state::TalisInfo;
use cosmwasm_bignumber::{Decimal256, Uint256};
use cosmwasm_std::testing::{MockApi, MockQuerier, MockStorage, MOCK_CONTRACT_ADDR};
use cosmwasm_std::{
    from_slice, to_binary, Addr, Api, BankQuery, Binary, Coin, ContractResult, Decimal, OwnedDeps,
    Querier, QuerierResult, QueryRequest, Response, StdError, StdResult, SystemError, SystemResult,
    Uint128, WasmQuery,
};
use cw20::BalanceResponse;
use serde::Serialize;
use std::str::FromStr;
use terra_cosmwasm::{
    ExchangeRateItem, ExchangeRatesResponse, TaxCapResponse, TaxRateResponse, TerraQuery,
    TerraQueryWrapper, TerraRoute,
};
//pub const MOCK_HUB_CONTRACT_ADDR: &str = "hub";
//pub const MOCK_CW20_CONTRACT_ADDR: &str = "lottery";
//pub const MOCK_REWARD_CONTRACT_ADDR: &str = "reward";
//pub const MOCK_TOKEN_CONTRACT_ADDR: &str = "token";

pub fn mock_dependencies_custom(
    contract_balance: &[Coin],
) -> OwnedDeps<MockStorage, MockApi, WasmMockQuerier> {
    let custom_querier =
        WasmMockQuerier::new(MockQuerier::new(&[(MOCK_CONTRACT_ADDR, contract_balance)]));
    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
        querier: custom_querier,
    }
}

pub struct WasmMockQuerier {
    base: MockQuerier<TerraQueryWrapper>,
}

impl Querier for WasmMockQuerier {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        let request: QueryRequest<TerraQueryWrapper> = match from_slice(bin_request) {
            Ok(v) => v,
            Err(e) => {
                return SystemResult::Err(SystemError::InvalidRequest {
                    error: format!("Parsing query request: {}", e),
                    request: bin_request.into(),
                })
            }
        };
        self.handle_query(&request)
    }
}

impl WasmMockQuerier {
    pub fn handle_query(&self, request: &QueryRequest<TerraQueryWrapper>) -> QuerierResult {
        match &request {
            QueryRequest::Wasm(WasmQuery::Smart { contract_addr, msg }) => {
                if contract_addr == &"market".to_string() {
                    println!("{:?}", request);
                    // CW-721 standard
                    // let msg_minter = cw20_base::state::MinterData {
                    //     minter: Addr::unchecked("terrans"),
                    //     cap: None,
                    // };
                    // Talis
                    let msg_minter = TalisInfo {
                        minter: Some("terrans".to_string()),
                        max_supply: None,
                    };
                    //let msg_minter = cw20::BalanceResponse{ balance: Uint128::from(100_u128) };
                    return SystemResult::Ok(ContractResult::from(to_binary(&msg_minter)));
                }
                panic!("DO NOT ENTER HERE")
            }
            QueryRequest::Custom(TerraQueryWrapper { route, query_data }) => match query_data {
                TerraQuery::TaxRate {} => {
                    let res = TaxRateResponse {
                        rate: Decimal::percent(1),
                    };
                    SystemResult::Ok(ContractResult::from(to_binary(&res)))
                }
                TerraQuery::TaxCap { denom: _ } => {
                    let cap = Uint128::from(1000000u128);
                    let res = TaxCapResponse { cap };
                    SystemResult::Ok(ContractResult::from(to_binary(&res)))
                }
                _ => panic!("DO NOT ENTER HERE"),
            },
            _ => self.base.handle_query(request),
        }
    }
}

impl WasmMockQuerier {
    pub fn new(base: MockQuerier<TerraQueryWrapper>) -> Self {
        WasmMockQuerier { base }
    }
}
