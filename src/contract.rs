#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_binary, to_binary, Binary, CanonicalAddr, Coin, CosmosMsg, Deps, DepsMut, Env,
    MessageInfo, Reply, Response, StdResult, SubMsg, Uint128, WasmMsg,
};
use cw2::set_contract_version;
use cw20::Cw20ReceiveMsg;
use cw721::{Cw721ExecuteMsg, Cw721ReceiveMsg};

use crate::error::ContractError;
use crate::msg::{
    CharityResponse, CountResponse, ExecuteMsg, InstantiateMsg, QueryMsg, ReceiveMsg,
};
use crate::state::{
    BidAmountTimeInfo, BidInfo, CharityInfo, ItemInfo, State, BIDDER, BIDS, CONFIG, ITEMS, STATE,
};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:marketplace";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    config.denom = msg.denom;
    config.bid_margin = msg.bid_margin;
    CONFIG.save(deps.storage, &config)?;

    /*
       Instantiate a cw20, privilege using this cw20 like private sale...
    */
    let cw20_msg = CosmosMsg::Wasm(WasmMsg::Instantiate {
        admin: Some(env.contract.address.to_string()),
        code_id: msg.cw20_code_id,
        msg: msg.cw20_msg,
        funds: vec![],
        label: msg.cw20_label,
    });

    /*
       Instantiate a cw721 for minting nft's directly from creation as option
    */
    let cw721_msg = CosmosMsg::Wasm(WasmMsg::Instantiate {
        admin: Some(env.contract.address.to_string()),
        code_id: msg.cw721_code_id,
        msg: msg.cw721_msg,
        funds: vec![],
        label: msg.cw721_label,
    });

    let cw20_sub_msg = SubMsg::reply_on_success(cw20_msg, 0);
    let cw721_sub_msg = SubMsg::reply_on_success(cw721_msg, 1);
    Ok(Response::new()
        .add_submessage(cw20_sub_msg)
        .add_submessage(cw721_sub_msg)
        .add_attribute("method", "instantiate")
        .add_attribute("owner", info.sender))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::InstantBuy { auction_id } => execute_instant_buy(deps, env, info, auction_id),
        ExecuteMsg::WithdrawNft { auction_id } => execute_withdraw_nft(deps, env, info, auction_id),
        ExecuteMsg::PlaceBid { auction_id } => execute_place_bid(deps, env, info, auction_id),
        ExecuteMsg::RetireBids { auction_id } => execute_retire_bids(deps, env, info, auction_id),
        ExecuteMsg::ReceiveCw721(msg) => execute_receive_cw721(deps, env, info, msg),
        ExecuteMsg::ReceiveCw20(msg) => execute_receive_cw20(deps, env, info, msg),
    }
}

pub fn execute_receive_cw721(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    wrapper: Cw721ReceiveMsg,
) -> Result<Response, ContractError> {
    // let state = STATE.load(deps.storage)?;

    // All cw721 can send nft and trigger this receive msg
    /*if info.sender != deps.api.addr_humanize(&state.cw721_address)? {
        return Err(ContractError::Unauthorized {});
    }*/

    let msg: ReceiveMsg = from_binary(&wrapper.msg)?;
    match msg {
        ReceiveMsg::CreateAuctionNft {
            start_price,
            start_time,
            end_time,
            charity,
            instant_buy,
            reserve_price,
            private_sale_privilege,
        } => execute_create_auction(
            deps,
            env,
            info,
            wrapper.sender,
            wrapper.token_id,
            start_price,
            start_time,
            end_time,
            charity,
            instant_buy,
            reserve_price,
            private_sale_privilege,
        ),
        _ => Err(ContractError::Unauthorized {}),
    }
    Ok(Response::default())
}

pub fn execute_receive_cw20(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    wrapper: Cw20ReceiveMsg,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;

    // Only cw20 PRIV can trigger this receive msg
    if info.sender != deps.api.addr_humanize(&state.cw20_address)? {
        return Err(ContractError::Unauthorized {});
    }

    let msg: ReceiveMsg = from_binary(&wrapper.msg)?;
    match msg {
        ReceiveMsg::RegisterPrivateSale { auction_id } => execute_register_private_sale(
            deps,
            env,
            info,
            wrapper.sender,
            wrapper.amount,
            auction_id,
        ),
        _ => Err(ContractError::Unauthorized {}),
    }
    Ok(Response::default())
}

pub fn execute_register_private_sale(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    sender: String,
    sent: Uint128,
    auction_id: u64,
) -> Result<Response, ContractError> {
    let sender_raw = deps.api.addr_canonicalize(sender.as_ref())?;

    let item = match ITEMS.may_load(deps.storage, &auction_id.to_be_bytes())? {
        None => Err(ContractError::Unauthorized {}),
        Some(item) => Ok(item),
    }?;

    // Check if the auction have private sale
    match item.private_sale_privilege {
        None => Err(ContractError::Unauthorized {}),
        Some(amount) => {
            if amount != sent {
                return Err(ContractError::Unauthorized {});
            }
        }
    }

    // Check if existing bid return error
    match BIDS.may_load(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
    )? {
        None => None,
        Some(_) => Err(ContractError::Unauthorized {}),
    };

    // Save Privilege
    BIDS.save(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
        &BidInfo {
            bids: vec![],
            bid_counter: 0,
            total_bid: Uint128::zero(),
            refunded: false,
            privilege_used: Some(sent),
        },
    )?;

    let res = Response::new()
        .add_attribute("register_auction", auction_id)
        .add_attribute("sender", sender.to_string())
        .add_attribute("amount_required", sent);

    Ok(res)
}

pub fn execute_create_auction(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    sender: String,
    token_id: String,
    start_price: Option<Uint128>,
    start_time: Option<u64>,
    end_time: u64,
    charity: Option<CharityResponse>,
    instant_buy: Option<Uint128>,
    reserve_price: Option<Uint128>,
    private_sale_privilege: Option<Uint128>,
) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;
    state.counter_items += 1;
    STATE.save(deps.storage, &state)?;

    let sender_raw = deps.api.addr_canonicalize(sender.as_ref())?;
    let contract_raw = deps.api.addr_canonicalize(info.sender.as_ref())?;

    if env.block.time.seconds() > end_time {
        return Err(ContractError::EndTimeExpired {});
    }

    // Validate charity data
    let valid_charity = match charity {
        None => None,
        Some(info) => {
            if info.fee_percentage < 0 || info.fee_percentage > 100 {
                return Err(ContractError::PercentageFormat {});
            }
            let addr_validate = deps.api.addr_validate(info.address.as_str())?;
            let addr_raw = deps.api.addr_canonicalize(addr_validate.as_str())?;
            Some(CharityInfo {
                address: addr_raw,
                fee_percentage: info.fee_percentage,
            })
        }
    };

    // Validate instant buy
    let instant_buy_price = match instant_buy {
        None => None,
        Some(price) => {
            if price.is_zero() {
                return Err(ContractError::ZeroNotValid {});
            }
            Some(price)
        }
    };

    // Validate private price
    let private_sale_price = match private_sale_privilege {
        None => None,
        Some(price) => {
            if price.is_zero() {
                return Err(ContractError::ZeroNotValid {});
            }
            Some(price)
        }
    };

    ITEMS.save(
        deps.storage,
        &state.counter_items.to_be_bytes(),
        &ItemInfo {
            creator: sender_raw,
            start_price: Some(start_price.unwrap_or_default()),
            start_time: Some(start_time?),
            end_time,
            highest_bid: Uint128::zero(),
            highest_bidder: Default::default(),
            nft_contract: contract_raw,
            nft_id: token_id.clone(),
            total_bids: 0,
            charity: valid_charity,
            instant_buy: instant_buy_price,
            reserve_price: Some(reserve_price?),
            private_sale_privilege: private_sale_price,
        },
    )?;

    let res = Response::new()
        .add_attribute("create_auction_type", "NFT")
        .add_attribute("token_id", token_id)
        .add_attribute("contract_minter", info.sender)
        .add_attribute("creator", sender)
        .add_attribute("new_temporal_owner", env.contract.address)
        .add_attribute("auction_id", state.counter_items);
    Ok(res)
}

pub fn execute_retire_bids(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    auction_id: u64,
) -> Result<Response, ContractError> {
    Ok(Response::default())
}

pub fn execute_place_bid(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    auction_id: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let sender_raw = deps.api.addr_canonicalize(&info.sender.as_str())?;
    let sent = match info.funds.len() {
        0 => Err(ContractError::EmptyFunds {}),
        1 => {
            if info.funds[0].denom != config.denom {
                return Err(ContractError::WrongDenom {});
            }
            Ok(info.funds[0].amount)
        }
        _ => Err(ContractError::MultipleDenoms {}),
    }?;

    let item = match ITEMS.may_load(deps.storage, &auction_id.to_be_bytes())? {
        None => Err(ContractError::Unauthorized {}),
        Some(item) => Ok(item),
    }?;

    let bid = match item.private_sale_privilege {
        None => None,
        Some(amount_required) => {
            let bid = match BIDS.may_load(
                deps.storage,
                (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
            )? {
                None => Err(ContractError::PrivateSaleRestriction(amount_required)),
                Some(bid) => {
                    if amount_required != bid.privilege_used {
                        ContractError::PrivateSaleRestriction(amount_required)
                    }
                    Ok(bid)
                }
            }?;
            Some(bid)
        }
    };

    let bid_margin = item.highest_bid.multiply_ratio(config.bid_margin, 100);
    let min_bid = item.highest_bid.checked_add(bid_margin)?;
    if sent < min_bid {
        return Err(ContractError::MinBid(min_bid));
    }

    ITEMS.update(
        deps.storage,
        &auction_id.to_be_bytes(),
        |item| -> StdResult<_> {
            let mut updated_item = item?;
            updated_item.highest_bid = sent;
            updated_item.highest_bidder = sender_raw;
            updated_item.total_bids += 1;

            Ok(updated_item)
        },
    )?;

    match bid {
        None => BIDS.save(
            deps.storage,
            (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
            &BidInfo {
                bids: vec![BidAmountTimeInfo {
                    amount: sent,
                    time: env.block.time.seconds(),
                }],
                bid_counter: 1,
                total_bid: sent,
                refunded: false,
                privilege_used: None,
            },
        ),
        Some(_) => {
            BIDS.update(
                deps.storage,
                (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
                |bid| -> StdResult<_> {
                    let mut updated_bid = bid?;
                    updated_bid.bid_counter += 1;
                    updated_bid.total_bid = updated_bid.total_bid.checked_add(sent)?;
                    updated_bid.bids.push(BidAmountTimeInfo {
                        amount: sent,
                        time: env.block.time.seconds(),
                    });

                    Ok(updated_bid)
                },
            )?;
        }
    }

    let res = Response::new()
        .add_attribute("new_bid", sent)
        .add_attribute("sender", info.sender.to_string())
        .add_attribute("auction_id", auction_id);
    Ok(res)
}

pub fn execute_instant_buy(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    auction_id: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let item = match ITEMS.may_load(deps.storage, &auction_id.to_be_bytes())? {
        None => Err(ContractError::Unauthorized {}),
        Some(item) => Ok(item),
    }?;
    let sender_raw = deps.api.addr_canonicalize(&info.sender.as_str())?;

    match item.private_sale_privilege {
        None => {}
        Some(prive_amount) => {
            match BIDS.may_load(
                deps.storage,
                (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
            )? {
                None => Err(ContractError::Unauthorized {}),
                Some(bids) => {
                    if bids.privilege_used.unwrap_or_default() != prive_amount {
                        return Err(ContractError::Unauthorized {});
                    }
                }
            }?;
        }
    }?;

    let sent = match info.funds.len() {
        0 => Err(ContractError::EmptyFunds {}),
        1 => {
            if info.funds[0].denom != config.denom {
                return Err(ContractError::WrongDenom {});
            }
            Ok(info.funds[0].amount)
        }
        _ => Err(ContractError::MultipleDenoms {}),
    }?;

    let instant_buy_amount = match item.instant_buy {
        None => Err(ContractError::Unauthorized {}),
        Some(amount) => {
            if amount != sent {
                return Err(ContractError::InaccurateFunds {});
            }
            Ok(amount)
        }
    }?;

    ITEMS.update(
        deps.storage,
        &auction_id.to_be_bytes(),
        |item| -> StdResult<_> {
            let mut updated_item = item?;
            updated_item.end_time = env.block.time.seconds();
            updated_item.highest_bid = instant_buy_amount;
            updated_item.highest_bidder = sender_raw;
            updated_item.total_bids += 1;

            Ok(updated_item)
        },
    )?;

    /*
       Prepare msg to send the NFT to the new owner
    */
    let msg_transfer_nft = Cw721ExecuteMsg::TransferNft {
        recipient: info.sender.to_string(),
        token_id: item.nft_id,
    };
    let msg_execute = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: deps.api.addr_humanize(&item.nft_contract)?.to_string(),
        msg: to_binary(&msg_transfer_nft)?,
        funds: vec![],
    });

    let res = Response::new()
        .add_message(msg_execute)
        .add_attribute("create_auction_type", "NFT")
        .add_attribute("token_id", token_id)
        .add_attribute("contract_minter", info.sender)
        .add_attribute("creator", sender)
        .add_attribute("new_temporal_owner", env.contract.address);
    Ok(res)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        0 => cw20_instance_reply(deps, env, msg.result),
        1 => cw721_instance_reply(deps, env, msg.result),
        _ => Err(ContractError::Unauthorized {}),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetCount {} => to_binary(&query_count(deps)?),
    }
}

fn query_count(deps: Deps) -> StdResult<CountResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(CountResponse { count: state.count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coins, from_binary};

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies(&[]);

        let msg = InstantiateMsg { count: 17 };
        let info = mock_info("creator", &coins(1000, "earth"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // it worked, let's query the state
        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(17, value.count);
    }

    #[test]
    fn increment() {
        let mut deps = mock_dependencies(&coins(2, "token"));

        let msg = InstantiateMsg { count: 17 };
        let info = mock_info("creator", &coins(2, "token"));
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        // beneficiary can release it
        let info = mock_info("anyone", &coins(2, "token"));
        let msg = ExecuteMsg::Increment {};
        let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        // should increase counter by 1
        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(18, value.count);
    }

    #[test]
    fn reset() {
        let mut deps = mock_dependencies(&coins(2, "token"));

        let msg = InstantiateMsg { count: 17 };
        let info = mock_info("creator", &coins(2, "token"));
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        // beneficiary can release it
        let unauth_info = mock_info("anyone", &coins(2, "token"));
        let msg = ExecuteMsg::Reset { count: 5 };
        let res = execute(deps.as_mut(), mock_env(), unauth_info, msg);
        match res {
            Err(ContractError::Unauthorized {}) => {}
            _ => panic!("Must return unauthorized error"),
        }

        // only the original creator can reset the counter
        let auth_info = mock_info("creator", &coins(2, "token"));
        let msg = ExecuteMsg::Reset { count: 5 };
        let _res = execute(deps.as_mut(), mock_env(), auth_info, msg).unwrap();

        // should now be 5
        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(5, value.count);
    }
}
