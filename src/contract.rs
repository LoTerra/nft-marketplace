use cosmwasm_std::{
    entry_point, from_binary, to_binary, BankMsg, Binary, CanonicalAddr, Coin, ContractResult,
    CosmosMsg, Deps, DepsMut, Env, MessageInfo, Reply, ReplyOn, Response, StdError, StdResult,
    SubMsg, SubMsgExecutionResponse, Uint128, WasmMsg,
};
use cw2::set_contract_version;
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
use cw721::{Cw721ExecuteMsg, Cw721ReceiveMsg};
use std::borrow::Borrow;

use crate::error::ContractError;
use crate::msg::{
    AuctionResponse, BidResponse, CharityResponse, ConfigResponse, ExecuteMsg, InstantiateMsg,
    QueryMsg, ReceiveMsg, StateResponse,
};
use crate::state::{
    BidAmountTimeInfo, BidInfo, CharityInfo, Config, ItemInfo, State, BIDS, CONFIG, ITEMS, STATE,
};
use crate::taxation::deduct_tax;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:marketplace";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
const MIN_TIME_AUCTION: u64 = 600;
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    let config = Config {
        denom: msg.denom,
        bid_margin: msg.bid_margin,
        lota_fee: msg.lota_fee,
        lota_contract: deps.api.addr_canonicalize(&msg.lota_contract)?,
    };

    CONFIG.save(deps.storage, &config)?;

    let state = State {
        counter_items: 0,
        cw20_address: deps.api.addr_canonicalize(&env.contract.address.as_str())?,
    };
    STATE.save(deps.storage, &state)?;
    /*
       Instantiate a cw20, privilege using this cw20 like private sale...
    */
    let msg_init = cw20_base::msg::InstantiateMsg {
        name: "privilege".to_string(),
        symbol: "PRIV".to_string(),
        decimals: 6,
        initial_balances: vec![],
        mint: Some(cw20::MinterResponse {
            minter: env.contract.address.to_string(),
            cap: None,
        }),
        marketing: None,
    };

    let cw20_msg = CosmosMsg::Wasm(WasmMsg::Instantiate {
        admin: Some(env.contract.address.to_string()),
        code_id: msg.cw20_code_id,
        msg: to_binary(&msg_init)?,
        funds: vec![],
        label: msg.cw20_label,
    });

    let cw20_sub_msg = SubMsg::reply_on_success(cw20_msg, 0);
    Ok(Response::new()
        .add_submessage(cw20_sub_msg)
        .add_attribute("method", "instantiate")
        .add_attribute("owner", info.sender))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::InstantBuy { auction_id } => execute_instant_buy(deps, env, info, auction_id),
        ExecuteMsg::WithdrawNft { auction_id } => execute_withdraw_nft(deps, env, info, auction_id),
        ExecuteMsg::PlaceBid { auction_id } => execute_place_bid(deps, env, info, auction_id),
        ExecuteMsg::RetireBids { auction_id } => execute_retire_bids(deps, env, info, auction_id),
        ExecuteMsg::ReceiveNft(msg) => execute_receive_cw721(deps, env, info, msg),
        ExecuteMsg::Receive(msg) => execute_receive_cw20(deps, env, info, msg),
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
}

pub fn execute_register_private_sale(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    sender: String,
    sent: Uint128,
    auction_id: u64,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
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
            Ok(())
        }
    }?;

    // Check if existing bid return error
    match BIDS.may_load(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
    )? {
        None => {}
        Some(_) => return Err(ContractError::Unauthorized {}),
    };

    // Save Privilege
    BIDS.save(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
        &BidInfo {
            bids: vec![],
            bid_counter: 0,
            total_bid: Uint128::zero(),
            privilege_used: Some(sent),
            resolved: false,
        },
    )?;

    let prepare_burn_msg = Cw20ExecuteMsg::Burn { amount: sent };
    let burn_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: deps.api.addr_humanize(&state.cw20_address)?.to_string(),
        msg: to_binary(&prepare_burn_msg)?,
        funds: vec![],
    });

    let res = Response::new()
        .add_message(burn_msg)
        .add_attribute("register_auction", auction_id.to_string())
        .add_attribute("sender", sender.to_string())
        .add_attribute("amount_required", sent.to_string());

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

    let sender_raw = deps.api.addr_canonicalize(sender.as_ref())?;
    let contract_raw = deps.api.addr_canonicalize(info.sender.as_ref())?;

    if env.block.time.seconds() > end_time.checked_add(MIN_TIME_AUCTION).unwrap() {
        return Err(ContractError::EndTimeExpired {});
    }
    match start_time {
        None => {}
        Some(time) => {
            if time <= end_time.checked_add(MIN_TIME_AUCTION).unwrap() {
                return Err(ContractError::EndTimeExpired {});
            }
        }
    }

    // Validate charity data
    let valid_charity = match charity {
        None => None,
        Some(info) => {
            if info.fee_percentage <= 0 || info.fee_percentage > 100 {
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
            start_price,
            start_time,
            end_time,
            highest_bid: None,
            highest_bidder: None,
            nft_contract: contract_raw,
            nft_id: token_id.clone(),
            total_bids: 0,
            charity: valid_charity,
            instant_buy: instant_buy_price,
            reserve_price,
            private_sale_privilege: private_sale_price,
            resolved: false,
        },
    )?;

    state.counter_items += 1;
    STATE.save(deps.storage, &state)?;

    let res = Response::new()
        .add_attribute("create_auction_type", "NFT")
        .add_attribute("token_id", token_id)
        .add_attribute("contract_minter", info.sender)
        .add_attribute("creator", sender)
        .add_attribute("new_temporal_owner", env.contract.address)
        .add_attribute(
            "auction_id",
            state.counter_items.checked_sub(1).unwrap().to_string(),
        );
    Ok(res)
}

pub fn execute_retire_bids(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    auction_id: u64,
) -> Result<Response, ContractError> {
    let sender_raw = deps.api.addr_canonicalize(&info.sender.as_str())?;
    let config = CONFIG.load(deps.storage)?;
    let state = STATE.load(deps.storage)?;

    let item = ITEMS.load(deps.storage, &auction_id.to_be_bytes())?;
    let bid = BIDS.load(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
    )?;

    // Check if the highest bidder is the sender
    match item.highest_bidder {
        None => {}
        Some(highest_bidder) => {
            if highest_bidder == sender_raw {
                return Err(ContractError::Unauthorized {});
            }
        }
    }

    // Check total bid is not 0
    if bid.total_bid.is_zero() {
        return Err(ContractError::Unauthorized {});
    }

    BIDS.update(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
        |bid| -> StdResult<_> {
            let mut update_bid = bid.unwrap();
            update_bid.total_bid = Uint128::zero();
            update_bid.resolved = true;
            Ok(update_bid)
        },
    )?;

    let bank_msg = CosmosMsg::Bank(BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![deduct_tax(
            &deps.querier,
            Coin {
                denom: config.denom.clone(),
                amount: bid.total_bid,
            },
        )?],
    });
    let mut msgs = vec![bank_msg];

    if !bid.resolved {
        let privilege_msg = Cw20ExecuteMsg::Mint {
            recipient: info.sender.to_string(),
            amount: Uint128::from(1_u128),
        };
        let execute_privilege_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: deps.api.addr_humanize(&state.cw20_address)?.to_string(),
            msg: to_binary(&privilege_msg)?,
            funds: vec![],
        });
        msgs.push(execute_privilege_msg);
    }

    let mut res = Response::new()
        .add_messages(msgs)
        .add_attribute("auction_id", auction_id.to_string())
        .add_attribute("refund_amount", bid.total_bid)
        .add_attribute("recipient", info.sender.to_string());

    Ok(res)
}

pub fn execute_withdraw_nft(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    auction_id: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let state = STATE.load(deps.storage)?;
    let item = ITEMS.load(deps.storage, &auction_id.to_be_bytes())?;

    if item.resolved {
        return Err(ContractError::Unauthorized {});
    }

    // check the auction ended
    if env.block.time.seconds() < item.end_time {
        return Err(ContractError::Unauthorized {});
    }

    let mut net_amount_after = Uint128::zero();
    let mut charity_amount = Uint128::zero();
    let mut lota_fee_amount = Uint128::zero();
    let mut charity_address = None;
    let recipient_address_raw = match item.highest_bidder {
        None => item.creator.clone(),
        Some(address) => match item.reserve_price {
            None => address,
            Some(reserve_price) => match item.highest_bid {
                None => item.creator.clone(),
                Some(highest_bid) => {
                    if let Some(charity) = item.charity {
                        charity_amount = highest_bid
                            .multiply_ratio(charity.fee_percentage, Uint128::from(100_u128));
                        net_amount_after = highest_bid.checked_sub(charity_amount).unwrap();
                        charity_address = Some(charity.address);
                    }
                    if reserve_price > highest_bid {
                        item.creator.clone();
                    }
                    address
                }
            },
        },
    };

    if let Some(highest_bid) = item.highest_bid {
        lota_fee_amount = highest_bid.multiply_ratio(config.lota_fee, Uint128::from(100_u128));
        net_amount_after = highest_bid.checked_sub(lota_fee_amount).unwrap();
    }

    ITEMS.update(
        deps.storage,
        &auction_id.to_be_bytes(),
        |item| -> StdResult<_> {
            let mut update_item = item.unwrap();
            update_item.resolved = true;
            Ok(update_item)
        },
    )?;
    /*
       Prepare msg to send the NFT to the new owner
    */
    let new_owner = deps.api.addr_humanize(&recipient_address_raw)?;
    let msg_transfer_nft = Cw721ExecuteMsg::TransferNft {
        recipient: new_owner.to_string(),
        token_id: item.nft_id,
    };
    let msg_execute = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: deps.api.addr_humanize(&item.nft_contract)?.to_string(),
        msg: to_binary(&msg_transfer_nft)?,
        funds: vec![],
    });

    let mut msgs = vec![msg_execute];
    /*
       TODO: Prepare msg to send rewards PRIV token
    */
    // Send to winner and creator if exist
    if recipient_address_raw != item.creator {
        let priv_reward_amount = Uint128::from(5_u128);

        // Send to creator
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: deps.api.addr_humanize(&state.cw20_address)?.to_string(),
            msg: to_binary(&Cw20ExecuteMsg::Transfer {
                recipient: deps.api.addr_humanize(&item.creator)?.to_string(),
                amount: priv_reward_amount,
            })?,
            funds: vec![],
        }));

        // Send to creator
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: deps.api.addr_humanize(&state.cw20_address)?.to_string(),
            msg: to_binary(&Cw20ExecuteMsg::Transfer {
                recipient: deps.api.addr_humanize(&recipient_address_raw)?.to_string(),
                amount: priv_reward_amount,
            })?,
            funds: vec![],
        }));

        /*
            TODO: Prepare msg to send payout to creator
        */
        if !net_amount_after.is_zero() {
            msgs.push(CosmosMsg::Bank(BankMsg::Send {
                to_address: deps.api.addr_humanize(&item.creator)?.to_string(),
                amount: vec![deduct_tax(
                    &deps.querier,
                    Coin {
                        denom: config.denom.clone(),
                        amount: net_amount_after,
                    },
                )?],
            }));
        }

        /*
           TODO: Prepare msg send to lota
        */
        if !lota_fee_amount.is_zero() {
            msgs.push(CosmosMsg::Bank(BankMsg::Send {
                to_address: deps.api.addr_humanize(&config.lota_contract)?.to_string(),
                amount: vec![deduct_tax(
                    &deps.querier,
                    Coin {
                        denom: config.denom.clone(),
                        amount: lota_fee_amount,
                    },
                )?],
            }));
        }
    }

    /*
       TODO: Prepare msg to send charity if some charity
    */
    if let Some(address) = charity_address {
        msgs.push(CosmosMsg::Bank(BankMsg::Send {
            to_address: deps.api.addr_humanize(&address)?.to_string(),
            amount: vec![deduct_tax(
                &deps.querier,
                Coin {
                    denom: config.denom.clone(),
                    amount: charity_amount,
                },
            )?],
        }));
    }

    let mut res = Response::new()
        .add_messages(msgs)
        .add_attribute("auction_type", "NFT")
        .add_attribute("auction_id", auction_id.to_string())
        .add_attribute("sender", info.sender)
        .add_attribute(
            "creator",
            deps.api.addr_humanize(&item.creator)?.to_string(),
        )
        .add_attribute("recipient", new_owner.to_string());

    Ok(res)
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

    let item = ITEMS.load(deps.storage, &auction_id.to_be_bytes())?;

    if item.end_time < env.block.time.seconds() {
        return Err(ContractError::EndTimeExpired {});
    }
    // Handle creator are not bidding
    if item.creator == sender_raw {
        return Err(ContractError::Unauthorized {});
    }

    match item.private_sale_privilege {
        None => Ok(()),
        Some(amount_required) => {
            match BIDS.may_load(
                deps.storage,
                (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
            )? {
                None => Err(ContractError::PrivateSaleRestriction(amount_required)),
                Some(bid) => {
                    if amount_required != bid.privilege_used.unwrap_or_default() {
                        return Err(ContractError::PrivateSaleRestriction(amount_required));
                    }
                    Ok(())
                }
            }
        }
    }?;

    let current_bid = match item.start_price {
        None => item.highest_bid.unwrap_or_default(),
        Some(start_price) => {
            if start_price > item.highest_bid.unwrap_or_default() {
                start_price
            } else {
                item.highest_bid.unwrap_or_default()
            }
        }
    };

    let bid_margin = current_bid.multiply_ratio(config.bid_margin as u128, 100 as u128);
    let min_bid = current_bid.checked_add(bid_margin).unwrap();
    let bid_total_sent = match BIDS.may_load(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
    )? {
        None => Some(sent),
        Some(bid_sent) => Some(bid_sent.total_bid.checked_add(sent).unwrap()),
    }
    .unwrap_or_else(|| sent);

    if bid_total_sent < min_bid {
        return Err(ContractError::MinBid(min_bid, bid_total_sent));
    }

    ITEMS.update(
        deps.storage,
        &auction_id.to_be_bytes(),
        |item| -> StdResult<_> {
            let mut updated_item = item.unwrap();
            // Cannot bid more than the instant buy price
            match updated_item.instant_buy {
                None => None,
                Some(instant_buy) => {
                    if instant_buy <= bid_total_sent {
                        return Err(StdError::generic_err(format!(
                            "Use instant buy price {0}, you are trying to bid higher {1}",
                            instant_buy, bid_total_sent
                        )));
                    }
                    Some(instant_buy)
                }
            };

            updated_item.highest_bid = Some(bid_total_sent);
            updated_item.highest_bidder = Some(sender_raw.clone());
            updated_item.total_bids += 1;

            Ok(updated_item)
        },
    )?;

    match BIDS.may_load(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
    )? {
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
                privilege_used: None,
                resolved: false,
            },
        )?,
        Some(_) => {
            BIDS.update(
                deps.storage,
                (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
                |bid| -> StdResult<_> {
                    let mut updated_bid = bid.unwrap();
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
        .add_attribute("new_bid", sent.to_string())
        .add_attribute("sender", info.sender.to_string())
        .add_attribute("auction_id", auction_id.to_string());
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
    if env.block.time.seconds() > item.end_time {
        return Err(ContractError::EndTimeExpired {});
    }

    let sender_raw = deps.api.addr_canonicalize(&info.sender.as_str())?;

    // Handle creator are not bidding
    if item.creator == sender_raw {
        return Err(ContractError::Unauthorized {});
    }

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
                    Ok(())
                }
            };
        }
    };

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
            let mut updated_item = item.unwrap();
            updated_item.end_time = env.block.time.minus_seconds(MIN_TIME_AUCTION).seconds();
            updated_item.highest_bid = Some(instant_buy_amount);
            updated_item.highest_bidder = Some(sender_raw);
            updated_item.total_bids += 1;

            Ok(updated_item)
        },
    )?;

    let res = Response::new().add_attribute("create_auction_type", "NFT");
    Ok(res)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> Result<Response, ContractError> {
    match msg.id {
        0 => cw20_instance_reply(deps, env, msg.result),
        _ => Err(ContractError::Unauthorized {}),
    }
}
pub fn cw20_instance_reply(
    deps: DepsMut,
    _env: Env,
    msg: ContractResult<SubMsgExecutionResponse>,
) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;
    match msg {
        ContractResult::Ok(subcall) => {
            let contract_address = subcall
                .events
                .into_iter()
                .find(|e| e.ty == "instantiate_contract")
                .and_then(|ev| {
                    ev.attributes
                        .into_iter()
                        .find(|attr| attr.key == "contract_address")
                        .and_then(|addr| Some(addr.value))
                })
                .unwrap();

            state.cw20_address = deps.api.addr_canonicalize(&contract_address.as_str())?;
            STATE.save(deps.storage, &state)?;

            let res = Response::new()
                .add_attribute("cw20-address", contract_address)
                .add_attribute("action", "cw20-instantiate");
            Ok(res)
        }
        ContractResult::Err(_) => Err(ContractError::Unauthorized {}),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Auction { auction_id } => to_binary(&query_auction(deps, env, auction_id)?),
        QueryMsg::Bidder {
            auction_id,
            address,
        } => to_binary(&query_bidder(deps, env, auction_id, address)?),
        QueryMsg::Config {} => to_binary(&query_config(deps, env)?),
        QueryMsg::State {} => to_binary(&query_state(deps, env)?),
    }
}
fn query_config(deps: Deps, _env: Env) -> StdResult<ConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    Ok(ConfigResponse {
        denom: config.denom,
        bid_margin: config.bid_margin,
        lota_fee: config.lota_fee,
        lota_contract: deps.api.addr_humanize(&config.lota_contract)?.to_string(),
    })
}

fn query_state(deps: Deps, _env: Env) -> StdResult<StateResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(StateResponse {
        counter_items: state.counter_items,
        cw20_address: deps.api.addr_humanize(&state.cw20_address)?.to_string(),
    })
}

fn query_auction(deps: Deps, _env: Env, auction_id: u64) -> StdResult<AuctionResponse> {
    let item = match ITEMS.may_load(deps.storage, &auction_id.to_be_bytes())? {
        None => Err(StdError::generic_err("Not found")),
        Some(item) => Ok(item),
    }?;
    let highest_bidder = match item.highest_bidder {
        None => None,
        Some(highest_bidder) => Some(deps.api.addr_humanize(&highest_bidder)?.to_string()),
    };

    let charity = match item.charity {
        None => None,
        Some(charity) => Some(CharityResponse {
            address: deps.api.addr_humanize(&charity.address)?.to_string(),
            fee_percentage: charity.fee_percentage,
        }),
    };

    Ok(AuctionResponse {
        creator: deps.api.addr_humanize(&item.creator)?.to_string(),
        start_price: item.start_price,
        start_time: item.start_time,
        end_time: item.end_time,
        highest_bid: item.highest_bid,
        highest_bidder,
        nft_contract: deps.api.addr_humanize(&item.nft_contract)?.to_string(),
        nft_id: item.nft_id,
        total_bids: item.total_bids,
        charity,
        instant_buy: item.instant_buy,
        reserve_price: item.reserve_price,
        private_sale_privilege: item.private_sale_privilege,
        resolved: item.resolved,
    })
}
fn query_bidder(deps: Deps, _env: Env, auction_id: u64, address: String) -> StdResult<BidResponse> {
    let bid = match BIDS.may_load(
        deps.storage,
        (
            &auction_id.to_be_bytes(),
            deps.api.addr_canonicalize(&address)?.as_slice(),
        ),
    )? {
        None => Err(StdError::generic_err("Not found")),
        Some(bid) => Ok(bid),
    }?;
    Ok(BidResponse {
        bids: bid.bids,
        bid_counter: bid.bid_counter,
        total_bid: bid.total_bid,
        privilege_used: bid.privilege_used,
    })
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::error::ContractError::MinBid;
    use crate::mock_querier::mock_dependencies_custom;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, MOCK_CONTRACT_ADDR};
    use cosmwasm_std::{coins, from_binary, Api, Attribute, ReplyOn, StdError};
    use cw20::Cw20ExecuteMsg;

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies(&[]);
        let info = mock_info("creator", &[]);
        let env = mock_env();

        let msg = InstantiateMsg {
            denom: "uusd".to_string(),
            cw20_code_id: 9,
            cw20_label: "cw20".to_string(),
            bid_margin: 5,
            lota_fee: 5,
            lota_contract: "loterra".to_string(),
        };

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(1, res.messages.len());
    }

    #[test]
    fn create_auction() {
        let mut deps = mock_dependencies(&coins(2, "token"));

        let msg = InstantiateMsg {
            denom: "uusd".to_string(),
            cw20_code_id: 9,
            cw20_label: "cw20".to_string(),
            bid_margin: 5,
            lota_fee: 5,
            lota_contract: "loterra".to_string(),
        };

        let info = mock_info("creator", &vec![]);
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        // ERROR create auction with end_time inferior current time
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: None,
            end_time: 0,
            charity: None,
            instant_buy: None,
            reserve_price: None,
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "market".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("sender", &vec![]),
            execute_msg,
        )
        .unwrap_err();

        // ERROR create auction with time end == time start
        let env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: Some(env.block.time.plus_seconds(1000).seconds()),
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: None,
            instant_buy: None,
            reserve_price: None,
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "sender".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap_err();

        // ERROR create auction with time end superior now but with Option Charity info wrong fee_percentage
        let env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: None,
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: Some(CharityResponse {
                address: "angel".to_string(),
                fee_percentage: 101,
            }),
            instant_buy: None,
            reserve_price: None,
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "sender".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap_err();

        // create auction with time end superior now but without options
        let env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: None,
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: None,
            instant_buy: None,
            reserve_price: None,
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "sender".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();
        let item = ITEMS
            .load(deps.as_ref().storage, &0_u64.to_be_bytes())
            .unwrap();
        assert_eq!(
            item.creator,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("sender").unwrap().to_string())
                .unwrap()
        );
        assert_eq!(item.highest_bid, None);
        assert_eq!(item.nft_id, "test".to_string());
        assert_eq!(item.start_time, None);
        assert_eq!(item.start_price, None);
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, None);
        assert_eq!(item.end_time, env.block.time.plus_seconds(1000).seconds());
        assert_eq!(item.private_sale_privilege, None);
        assert_eq!(item.total_bids, 0);
        assert_eq!(item.instant_buy, None);
        assert_eq!(item.charity, None);
        assert_eq!(
            item.nft_contract,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("market").unwrap().to_string())
                .unwrap()
        );

        assert_eq!(
            res.attributes,
            vec![
                Attribute {
                    key: "create_auction_type".to_string(),
                    value: "NFT".to_string()
                },
                Attribute {
                    key: "token_id".to_string(),
                    value: "test".to_string()
                },
                Attribute {
                    key: "contract_minter".to_string(),
                    value: "market".to_string()
                },
                Attribute {
                    key: "creator".to_string(),
                    value: "sender".to_string()
                },
                Attribute {
                    key: "new_temporal_owner".to_string(),
                    value: "cosmos2contract".to_string()
                },
                Attribute {
                    key: "auction_id".to_string(),
                    value: "0".to_string()
                }
            ]
        );

        // create auction with time end superior now but with Option start price
        let env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: Some(Uint128::from(1000_u128)),
            start_time: None,
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: None,
            instant_buy: None,
            reserve_price: None,
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "sender".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();
        let item = ITEMS
            .load(deps.as_ref().storage, &1_u64.to_be_bytes())
            .unwrap();
        assert_eq!(
            item.creator,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("sender").unwrap().to_string())
                .unwrap()
        );
        assert_eq!(item.highest_bid, None);
        assert_eq!(item.nft_id, "test".to_string());
        assert_eq!(item.start_time, None);
        assert_eq!(item.start_price, Some(Uint128::from(1000_u128)));
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, None);
        assert_eq!(item.end_time, env.block.time.plus_seconds(1000).seconds());
        assert_eq!(item.private_sale_privilege, None);
        assert_eq!(item.total_bids, 0);
        assert_eq!(item.instant_buy, None);
        assert_eq!(item.charity, None);
        assert_eq!(
            item.nft_contract,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("market").unwrap().to_string())
                .unwrap()
        );
        assert_eq!(
            res.attributes,
            vec![
                Attribute {
                    key: "create_auction_type".to_string(),
                    value: "NFT".to_string()
                },
                Attribute {
                    key: "token_id".to_string(),
                    value: "test".to_string()
                },
                Attribute {
                    key: "contract_minter".to_string(),
                    value: "market".to_string()
                },
                Attribute {
                    key: "creator".to_string(),
                    value: "sender".to_string()
                },
                Attribute {
                    key: "new_temporal_owner".to_string(),
                    value: "cosmos2contract".to_string()
                },
                Attribute {
                    key: "auction_id".to_string(),
                    value: "1".to_string()
                }
            ]
        );

        // create auction with time end superior now but with Option start price
        let env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: Some(env.block.time.plus_seconds(2000).seconds()),
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: None,
            instant_buy: None,
            reserve_price: None,
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "sender".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();
        let item = ITEMS
            .load(deps.as_ref().storage, &2_u64.to_be_bytes())
            .unwrap();
        assert_eq!(
            item.creator,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("sender").unwrap().to_string())
                .unwrap()
        );
        assert_eq!(item.highest_bid, None);
        assert_eq!(item.nft_id, "test".to_string());
        assert_eq!(
            item.start_time,
            Some(env.block.time.plus_seconds(2000).seconds())
        );
        assert_eq!(item.start_price, None);
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, None);
        assert_eq!(item.end_time, env.block.time.plus_seconds(1000).seconds());
        assert_eq!(item.private_sale_privilege, None);
        assert_eq!(item.total_bids, 0);
        assert_eq!(item.instant_buy, None);
        assert_eq!(item.charity, None);
        assert_eq!(
            item.nft_contract,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("market").unwrap().to_string())
                .unwrap()
        );
        assert_eq!(
            res.attributes,
            vec![
                Attribute {
                    key: "create_auction_type".to_string(),
                    value: "NFT".to_string()
                },
                Attribute {
                    key: "token_id".to_string(),
                    value: "test".to_string()
                },
                Attribute {
                    key: "contract_minter".to_string(),
                    value: "market".to_string()
                },
                Attribute {
                    key: "creator".to_string(),
                    value: "sender".to_string()
                },
                Attribute {
                    key: "new_temporal_owner".to_string(),
                    value: "cosmos2contract".to_string()
                },
                Attribute {
                    key: "auction_id".to_string(),
                    value: "2".to_string()
                }
            ]
        );

        // create auction with time end superior now but with Option instant buy
        let env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: None,
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: None,
            instant_buy: Some(Uint128::from(1000_u128)),
            reserve_price: None,
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "sender".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();
        let item = ITEMS
            .load(deps.as_ref().storage, &3_u64.to_be_bytes())
            .unwrap();
        assert_eq!(
            item.creator,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("sender").unwrap().to_string())
                .unwrap()
        );
        assert_eq!(item.highest_bid, None);
        assert_eq!(item.nft_id, "test".to_string());
        assert_eq!(item.start_time, None);
        assert_eq!(item.start_price, None);
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, None);
        assert_eq!(item.end_time, env.block.time.plus_seconds(1000).seconds());
        assert_eq!(item.private_sale_privilege, None);
        assert_eq!(item.total_bids, 0);
        assert_eq!(item.instant_buy, Some(Uint128::from(1000_u128)));
        assert_eq!(item.charity, None);
        assert_eq!(
            item.nft_contract,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("market").unwrap().to_string())
                .unwrap()
        );
        assert_eq!(
            res.attributes,
            vec![
                Attribute {
                    key: "create_auction_type".to_string(),
                    value: "NFT".to_string()
                },
                Attribute {
                    key: "token_id".to_string(),
                    value: "test".to_string()
                },
                Attribute {
                    key: "contract_minter".to_string(),
                    value: "market".to_string()
                },
                Attribute {
                    key: "creator".to_string(),
                    value: "sender".to_string()
                },
                Attribute {
                    key: "new_temporal_owner".to_string(),
                    value: "cosmos2contract".to_string()
                },
                Attribute {
                    key: "auction_id".to_string(),
                    value: "3".to_string()
                }
            ]
        );

        // create auction with time end superior now but with Option reserve price
        let env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: None,
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: None,
            instant_buy: None,
            reserve_price: Some(Uint128::from(1000_u128)),
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "sender".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();
        let item = ITEMS
            .load(deps.as_ref().storage, &4_u64.to_be_bytes())
            .unwrap();
        assert_eq!(
            item.creator,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("sender").unwrap().to_string())
                .unwrap()
        );
        assert_eq!(item.highest_bid, None);
        assert_eq!(item.nft_id, "test".to_string());
        assert_eq!(item.start_time, None);
        assert_eq!(item.start_price, None);
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, Some(Uint128::from(1000_u128)));
        assert_eq!(item.end_time, env.block.time.plus_seconds(1000).seconds());
        assert_eq!(item.private_sale_privilege, None);
        assert_eq!(item.total_bids, 0);
        assert_eq!(item.instant_buy, None);
        assert_eq!(item.charity, None);
        assert_eq!(
            item.nft_contract,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("market").unwrap().to_string())
                .unwrap()
        );
        assert_eq!(
            res.attributes,
            vec![
                Attribute {
                    key: "create_auction_type".to_string(),
                    value: "NFT".to_string()
                },
                Attribute {
                    key: "token_id".to_string(),
                    value: "test".to_string()
                },
                Attribute {
                    key: "contract_minter".to_string(),
                    value: "market".to_string()
                },
                Attribute {
                    key: "creator".to_string(),
                    value: "sender".to_string()
                },
                Attribute {
                    key: "new_temporal_owner".to_string(),
                    value: "cosmos2contract".to_string()
                },
                Attribute {
                    key: "auction_id".to_string(),
                    value: "4".to_string()
                }
            ]
        );

        // create auction with time end superior now but with Option privilege sale
        let env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: None,
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: None,
            instant_buy: None,
            reserve_price: None,
            private_sale_privilege: Some(Uint128::from(1000_u128)),
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "sender".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();
        let item = ITEMS
            .load(deps.as_ref().storage, &5_u64.to_be_bytes())
            .unwrap();
        assert_eq!(
            item.creator,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("sender").unwrap().to_string())
                .unwrap()
        );
        assert_eq!(item.highest_bid, None);
        assert_eq!(item.nft_id, "test".to_string());
        assert_eq!(item.start_time, None);
        assert_eq!(item.start_price, None);
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, None);
        assert_eq!(item.end_time, env.block.time.plus_seconds(1000).seconds());
        assert_eq!(item.private_sale_privilege, Some(Uint128::from(1000_u128)));
        assert_eq!(item.total_bids, 0);
        assert_eq!(item.instant_buy, None);
        assert_eq!(item.charity, None);
        assert_eq!(
            item.nft_contract,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("market").unwrap().to_string())
                .unwrap()
        );
        assert_eq!(
            res.attributes,
            vec![
                Attribute {
                    key: "create_auction_type".to_string(),
                    value: "NFT".to_string()
                },
                Attribute {
                    key: "token_id".to_string(),
                    value: "test".to_string()
                },
                Attribute {
                    key: "contract_minter".to_string(),
                    value: "market".to_string()
                },
                Attribute {
                    key: "creator".to_string(),
                    value: "sender".to_string()
                },
                Attribute {
                    key: "new_temporal_owner".to_string(),
                    value: "cosmos2contract".to_string()
                },
                Attribute {
                    key: "auction_id".to_string(),
                    value: "5".to_string()
                }
            ]
        );

        // create auction with time end superior now but with Option Charity info
        let env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: None,
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: Some(CharityResponse {
                address: "angel".to_string(),
                fee_percentage: 10,
            }),
            instant_buy: None,
            reserve_price: None,
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "sender".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();
        let item = ITEMS
            .load(deps.as_ref().storage, &6_u64.to_be_bytes())
            .unwrap();
        assert_eq!(
            item.creator,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("sender").unwrap().to_string())
                .unwrap()
        );
        assert_eq!(item.highest_bid, None);
        assert_eq!(item.nft_id, "test".to_string());
        assert_eq!(item.start_time, None);
        assert_eq!(item.start_price, None);
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, None);
        assert_eq!(item.end_time, env.block.time.plus_seconds(1000).seconds());
        assert_eq!(item.private_sale_privilege, None);
        assert_eq!(item.total_bids, 0);
        assert_eq!(item.instant_buy, None);
        assert_eq!(
            item.charity,
            Some(CharityInfo {
                address: deps
                    .api
                    .addr_canonicalize(&deps.api.addr_validate("angel").unwrap().to_string())
                    .unwrap(),
                fee_percentage: 10
            })
        );
        assert_eq!(
            item.nft_contract,
            deps.api
                .addr_canonicalize(&deps.api.addr_validate("market").unwrap().to_string())
                .unwrap()
        );
        assert_eq!(
            res.attributes,
            vec![
                Attribute {
                    key: "create_auction_type".to_string(),
                    value: "NFT".to_string()
                },
                Attribute {
                    key: "token_id".to_string(),
                    value: "test".to_string()
                },
                Attribute {
                    key: "contract_minter".to_string(),
                    value: "market".to_string()
                },
                Attribute {
                    key: "creator".to_string(),
                    value: "sender".to_string()
                },
                Attribute {
                    key: "new_temporal_owner".to_string(),
                    value: "cosmos2contract".to_string()
                },
                Attribute {
                    key: "auction_id".to_string(),
                    value: "6".to_string()
                }
            ]
        );
    }

    #[test]
    fn place_bid_auction_retire_bid_reserve_price_private_sale() {
        let mut deps = mock_dependencies(&coins(2, "token"));

        let msg = InstantiateMsg {
            denom: "uusd".to_string(),
            cw20_code_id: 9,
            cw20_label: "cw20".to_string(),
            bid_margin: 5,
            lota_fee: 5,
            lota_contract: "loterra".to_string(),
        };

        let info = mock_info("creator", &vec![]);
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        // ERROR create auction with end_time inferior current time
        let env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: None,
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: None,
            instant_buy: None,
            reserve_price: None,
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "market".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);

        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("sender", &vec![]),
            execute_msg,
        )
        .unwrap();

        let execute_msg = ExecuteMsg::PlaceBid { auction_id: 1 };
        // ERROR Wrong auction id
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "sender",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(1000_u128),
                }],
            ),
            execute_msg,
        )
        .unwrap_err();

        // ERROR sender empty funds
        let execute_msg = ExecuteMsg::PlaceBid { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("sender", &vec![]),
            execute_msg.clone(),
        )
        .unwrap_err();

        // Instantiate with start price 1000 ust
        let env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: Some(Uint128::from(1000_u128)),
            start_time: None,
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: None,
            instant_buy: None,
            reserve_price: None,
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "market".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("sender", &vec![]),
            execute_msg,
        )
        .unwrap();

        let execute_msg = ExecuteMsg::PlaceBid { auction_id: 1 };
        // ERROR sent not enough
        let err = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "sender",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(1_u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap_err();

        // Min Bid success Alice
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "alice",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(1050_u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap();
        println!("{:?}", res);
        // Increase bid Bob
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "bob",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(2_000_u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap();
        // Increase bid Sam
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "sam",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(2_100_u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap();
        // ERROR sam retire bids before end because he is the higher bidder
        let msg = ExecuteMsg::RetireBids { auction_id: 1 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("sam", &vec![]),
            msg.clone(),
        )
        .unwrap_err();

        // Min fight bid success Alice
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "alice",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(1155_u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap();

        // SUCCESS sam retire bids before end because Alice is now the highest bidder
        let msg = ExecuteMsg::RetireBids { auction_id: 1 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("sam", &vec![]),
            msg.clone(),
        )
        .unwrap();

        // Bid closed expire
        let mut env = mock_env();
        env.block.time = env.block.time.plus_seconds(2000);
        let item = ITEMS
            .load(deps.as_ref().storage, &1_u64.to_be_bytes())
            .unwrap();

        // Bob fail increasing bid Bob and lose because time end
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "bob",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(3_000_u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap_err();
        let alice_raw = deps.api.addr_canonicalize("alice").unwrap();
        let bid_alice = BIDS
            .load(
                deps.as_ref().storage,
                (&1_u64.to_be_bytes(), &alice_raw.as_slice()),
            )
            .unwrap();
        assert_eq!(bid_alice.total_bid, Uint128::from(2205_u128));
        assert_eq!(bid_alice.bids.len(), 2);
        assert_eq!(bid_alice.bid_counter, 2);
        assert_eq!(bid_alice.privilege_used, None);

        let bob_raw = deps.api.addr_canonicalize("bob").unwrap();
        let bid_bob = BIDS
            .load(
                deps.as_ref().storage,
                (&1_u64.to_be_bytes(), &bob_raw.as_slice()),
            )
            .unwrap();
        assert_eq!(bid_bob.total_bid, Uint128::from(2000_u128));
        assert_eq!(bid_bob.bids.len(), 1);
        assert_eq!(bid_bob.bid_counter, 1);
        assert_eq!(bid_bob.privilege_used, None);

        let item = ITEMS
            .load(deps.as_ref().storage, &1_u64.to_be_bytes())
            .unwrap();
        assert_eq!(item.highest_bidder, Some(alice_raw));
        assert_eq!(item.highest_bid, Some(Uint128::from(2205_u128)));
        assert_eq!(item.total_bids, 4);

        // ERROR Alice try to retire bids
        let msg = ExecuteMsg::RetireBids { auction_id: 1 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("alice", &vec![]),
            msg.clone(),
        )
        .unwrap_err();

        // SUCCESS bob to retire bids
        let msg = ExecuteMsg::RetireBids { auction_id: 1 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("bob", &vec![]),
            msg.clone(),
        )
        .unwrap();

        let bob_raw = deps.api.addr_canonicalize("bob").unwrap();
        let bid_bob = BIDS
            .load(
                deps.as_ref().storage,
                (&1_u64.to_be_bytes(), &bob_raw.as_slice()),
            )
            .unwrap();
        assert_eq!(bid_bob.total_bid, Uint128::zero());
        // ERROR to retire multiple times
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("bob", &vec![]),
            msg.clone(),
        )
        .unwrap_err();

        /*
           Withdraw NFT
        */
        //  Withdraw NFT to winner or creator
        let msg = ExecuteMsg::WithdrawNft { auction_id: 1 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("alice", &vec![]),
            msg.clone(),
        )
        .unwrap();
        println!("{:?}", res);

        // Instantiate with start price 1000 ust
        let env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: None,
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: None,
            instant_buy: Some(Uint128::from(10_000_u128)),
            reserve_price: None,
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "market".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("sender", &vec![]),
            execute_msg,
        )
        .unwrap();

        let execute_msg = ExecuteMsg::PlaceBid { auction_id: 2 };
        // ERROR Alice bidding higher than instant buy
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "alice",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(10_050_u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap_err();

        // Instantiate with start price 1000 ust
        let env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: None,
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: None,
            instant_buy: None,
            reserve_price: None,
            private_sale_privilege: Some(Uint128::from(10_000_u128)),
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "market".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("sender", &vec![]),
            execute_msg,
        )
        .unwrap();

        let execute_msg = ExecuteMsg::PlaceBid { auction_id: 3 };
        // ERROR Private sale registration required
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "alice",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(1_000_u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap_err();
        // Success Alice private sale registered
        let msg = ExecuteMsg::Receive(Cw20ReceiveMsg {
            sender: "alice".to_string(),
            amount: Uint128::from(10_000u128),
            msg: to_binary(&ReceiveMsg::RegisterPrivateSale { auction_id: 3 }).unwrap(),
        });

        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                /*Should be CW20 address but the contract is set to ENV CONTRACT*/
                MOCK_CONTRACT_ADDR,
                &vec![],
            ),
            msg.clone(),
        )
        .unwrap();

        // ERROR Register multiple time
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                /*Should be CW20 address but the contract is set to ENV CONTRACT*/
                MOCK_CONTRACT_ADDR,
                &vec![],
            ),
            msg.clone(),
        )
        .unwrap_err();

        // ERROR Send less
        let msg = ExecuteMsg::Receive(Cw20ReceiveMsg {
            sender: "bob".to_string(),
            amount: Uint128::from(5_000u128),
            msg: to_binary(&ReceiveMsg::RegisterPrivateSale { auction_id: 3 }).unwrap(),
        });

        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                /*Should be CW20 address but the contract is set to ENV CONTRACT*/
                MOCK_CONTRACT_ADDR,
                &vec![],
            ),
            msg.clone(),
        )
        .unwrap_err();

        // ERROR Send more
        let msg = ExecuteMsg::Receive(Cw20ReceiveMsg {
            sender: "bob".to_string(),
            amount: Uint128::from(5_000u128),
            msg: to_binary(&ReceiveMsg::RegisterPrivateSale { auction_id: 3 }).unwrap(),
        });

        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                /*Should be CW20 address but the contract is set to ENV CONTRACT*/
                MOCK_CONTRACT_ADDR,
                &vec![],
            ),
            msg.clone(),
        )
        .unwrap_err();

        let alice_raw = deps.api.addr_canonicalize("alice").unwrap();
        let bid_alice = BIDS
            .update(
                deps.as_mut().storage,
                (&3_u64.to_be_bytes(), &alice_raw.as_slice()),
                |bid| -> StdResult<_> {
                    let mut update_bid = bid.unwrap();
                    update_bid.privilege_used = Some(Uint128::from(10_000u128));
                    Ok(update_bid)
                },
            )
            .unwrap();

        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "alice",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(1_000_u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap();
    }
    #[test]
    fn creator_withdraw_nft() {
        /*
           Withdraw nft creator no bidders
        */
        let mut deps = mock_dependencies(&coins(2, "token"));

        let msg = InstantiateMsg {
            denom: "uusd".to_string(),
            cw20_code_id: 9,
            cw20_label: "cw20".to_string(),
            bid_margin: 5,
            lota_fee: 5,
            lota_contract: "loterra".to_string(),
        };

        let info = mock_info("creator", &vec![]);
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        let mut env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: None,
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: None,
            instant_buy: None,
            reserve_price: None,
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "market".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);

        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("sender", &vec![]),
            execute_msg,
        )
        .unwrap();

        // ERROR to withdraw auction not expired
        let msg = ExecuteMsg::WithdrawNft { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("alice", &vec![]),
            msg.clone(),
        )
        .unwrap_err();
        env.block.time = env.block.time.plus_seconds(2000);
        // SUCCESS
        let msg = ExecuteMsg::WithdrawNft { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("alice", &vec![]),
            msg.clone(),
        )
        .unwrap();
        println!("{:?}", res);
    }

    #[test]
    fn instant_buy() {
        /*
           Withdraw nft creator no bidders
        */
        let mut deps = mock_dependencies_custom(&coins(2, "token"));

        let msg = InstantiateMsg {
            denom: "uusd".to_string(),
            cw20_code_id: 9,
            cw20_label: "cw20".to_string(),
            bid_margin: 5,
            lota_fee: 5,
            lota_contract: "loterra".to_string(),
        };

        let info = mock_info("creator", &vec![]);
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        let mut env = mock_env();
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price: None,
            start_time: None,
            end_time: env.block.time.plus_seconds(1000).seconds(),
            charity: None,
            instant_buy: Some(Uint128::from(1_000u128)),
            reserve_price: None,
            private_sale_privilege: None,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "sender".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        let execute_msg = ExecuteMsg::ReceiveNft(send_msg);

        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();

        // ERROR No enough funds to buy
        let msg = ExecuteMsg::InstantBuy { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("alice", &vec![]),
            msg.clone(),
        )
        .unwrap_err();

        // SUCCESS BUY
        let msg = ExecuteMsg::InstantBuy { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "alice",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(1_000_u128),
                }],
            ),
            msg.clone(),
        )
        .unwrap();

        // ERROR auction expired
        let msg = ExecuteMsg::InstantBuy { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "rico",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(1_000_u128),
                }],
            ),
            msg.clone(),
        )
        .unwrap_err();

        env.block.time = env.block.time.plus_seconds(2000);
        // ERROR auction expired
        let msg = ExecuteMsg::InstantBuy { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("alice", &vec![]),
            msg.clone(),
        )
        .unwrap_err();

        // Alice withdraw winning NFT
        let msg = ExecuteMsg::WithdrawNft { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("alice", &vec![]),
            msg.clone(),
        )
        .unwrap();

        let withdraw_msg = Cw721ExecuteMsg::TransferNft {
            recipient: "alice".to_string(),
            token_id: "test".to_string(),
        };

        assert_eq!(
            res.messages,
            vec![
                SubMsg {
                    id: 0,
                    msg: CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: "market".to_string(),
                        msg: to_binary(&withdraw_msg).unwrap(),
                        funds: vec![]
                    }),
                    gas_limit: None,
                    reply_on: ReplyOn::Never
                },
                SubMsg {
                    id: 1,
                    msg: CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: "market".to_string(),
                        msg: to_binary(&withdraw_msg).unwrap(),
                        funds: vec![]
                    }),
                    gas_limit: None,
                    reply_on: ReplyOn::Never
                },
                SubMsg {
                    id: 2,
                    msg: CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: "market".to_string(),
                        msg: to_binary(&withdraw_msg).unwrap(),
                        funds: vec![]
                    }),
                    gas_limit: None,
                    reply_on: ReplyOn::Never
                },
                SubMsg {
                    id: 3,
                    msg: CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: "market".to_string(),
                        msg: to_binary(&withdraw_msg).unwrap(),
                        funds: vec![]
                    }),
                    gas_limit: None,
                    reply_on: ReplyOn::Never
                },
                SubMsg {
                    id: 4,
                    msg: CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: "market".to_string(),
                        msg: to_binary(&withdraw_msg).unwrap(),
                        funds: vec![]
                    }),
                    gas_limit: None,
                    reply_on: ReplyOn::Never
                }
            ]
        );
        assert_eq!(
            res.attributes,
            vec![
                Attribute {
                    key: "auction_type".to_string(),
                    value: "NFT".to_string()
                },
                Attribute {
                    key: "auction_id".to_string(),
                    value: "0".to_string()
                },
                Attribute {
                    key: "sender".to_string(),
                    value: "alice".to_string()
                },
                Attribute {
                    key: "creator".to_string(),
                    value: "sender".to_string()
                },
                Attribute {
                    key: "recipient".to_string(),
                    value: "alice".to_string()
                }
            ]
        );
        println!("{:?}", res);
    }
    // #[test]
    // fn remove_bid (){
    //     let mut deps = mock_dependencies(&coins(2, "token"));
    //     let msg = InstantiateMsg {
    //         denom: "uusd".to_string(),
    //         cw20_code_id: 9,
    //         cw20_msg: Default::default(),
    //         cw20_label: "cw20".to_string(),
    //         cw721_code_id: 10,
    //         cw721_msg: Default::default(),
    //         cw721_label: "cw721".to_string(),
    //         bid_margin: 5,
    //     };
    //
    //     let info = mock_info("creator", &vec![]);
    //     let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    //
    //     // Instantiate with start price 1000 ust
    //     let env = mock_env();
    //     let msg = ReceiveMsg::CreateAuctionNft {
    //         start_price: Some(Uint128::from(1000_u128)),
    //         start_time: None,
    //         end_time: env.block.time.plus_seconds(1000).seconds(),
    //         charity: None,
    //         instant_buy: None,
    //         reserve_price: None,
    //         private_sale_privilege: None,
    //     };
    //     let send_msg = cw721::Cw721ReceiveMsg {
    //         sender: "market".to_string(),
    //         token_id: "test".to_string(),
    //         msg: to_binary(&msg).unwrap(),
    //     };
    //     let execute_msg = ExecuteMsg::ReceiveNft(send_msg);
    //     let res = execute(
    //         deps.as_mut(),
    //         env.clone(),
    //         mock_info("sender", &vec![]),
    //         execute_msg,
    //     )
    //         .unwrap();
    //
    //     let execute_msg = ExecuteMsg::PlaceBid { auction_id: 1 };
    //
    //     // Alice sent enough
    //     let res = execute(
    //         deps.as_mut(),
    //         env.clone(),
    //         mock_info(
    //             "sender",
    //             &vec![Coin {
    //                 denom: "uusd".to_string(),
    //                 amount: Uint128::from(1_200_u128),
    //             }],
    //         ),
    //         execute_msg.clone(),
    //     )
    //         .unwrap();
    //
    //
    //     // ERROR Alice try to remove bids
    //     let msg = ExecuteMsg::RetireBids { auction_id: 1 };
    //     let res = execute(
    //         deps.as_mut(),
    //         env.clone(),
    //         mock_info(
    //             "alice",
    //             &vec![],
    //         ),
    //         msg.clone(),
    //     )
    //         .unwrap_err();
    //     println!("{:?}", res);
    // }
}
