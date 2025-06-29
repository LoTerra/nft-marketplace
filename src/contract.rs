use cosmwasm_std::{
    entry_point, from_binary, to_binary, Addr, BankMsg, Binary, Coin, ContractResult, CosmosMsg,
    Decimal, Deps, DepsMut, Env, MessageInfo, Order, Reply, Response, StdError, StdResult, SubMsg,
    SubMsgExecutionResponse, Uint128, WasmMsg, WasmQuery,
};
use cw2::set_contract_version;
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
use cw20_base::state::MinterData;
use cw721::{Cw721ExecuteMsg, Cw721ReceiveMsg};
use cw_storage_plus::Bound;
use std::convert::TryInto;
use std::ops::{Add, Mul};
use std::str::FromStr;

use crate::error::ContractError;
use crate::msg::{
    AllAuctionsResponse, AuctionResponse, BidResponse, CharityResponse, ConfigResponse, ExecuteMsg,
    HistoryBidResponse, HistoryResponse, InstantiateMsg, MigrateMsg, QueryMsg, QueryTalisMsg,
    ReceiveMsg, RoyaltyResponse, StateResponse,
};
use crate::state::{
    BidInfo, Cancellation, CharityInfo, Config, HistoryBidInfo, HistoryInfo, ItemInfo, RoyaltyInfo,
    State, TalisInfo, BIDS, CANCELLATION, CONFIG, HISTORIES, HISTORIES_BIDDER, ITEMS, ROYALTY,
    STATE,
};
use crate::taxation::deduct_tax;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:marketplace";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
const MIN_TIME_AUCTION: u64 = 600; // 10 min
const MAX_TIME_AUCTION: u64 = 15778800; // 6 months max
const LAST_MINUTE_BID_EXTRA_TIME: u64 = 600; // 10 min
const ROYALTY_MAX_FEE: &str = "0.10"; // 10% or 10/100
const DEFAULT_ROYALTY_FEE: &str = "0"; // 1% or 1/100

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
        lota_fee_low: msg.lota_fee_low,
        lota_contract: deps.api.addr_canonicalize(&msg.lota_contract)?,
        sity_full_rewards: msg.sity_full_rewards,
        sity_partial_rewards: msg.sity_partial_rewards,
        sity_fee_registration: msg.sity_fee_registration,
        sity_min_opening: msg.sity_min_opening,
    };

    CONFIG.save(deps.storage, &config)?;

    let state = State {
        counter_items: 0,
        cw20_address: deps.api.addr_canonicalize(&env.contract.address.as_str())?,
    };
    STATE.save(deps.storage, &state)?;

    let cancellation = Cancellation {
        cancellation_fee: Default::default(),
    };
    CANCELLATION.save(deps.storage, &cancellation)?;
    /*
       Instantiate a cw20, privilege using this cw20 like private sale...
    */
    let msg_init = cw20_base::msg::InstantiateMsg {
        name: "curio".to_string(),
        symbol: "SITY".to_string(),
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
        ExecuteMsg::RetractBids { auction_id } => execute_retract_bids(deps, env, info, auction_id),
        ExecuteMsg::UpdateRoyalty { fee, recipient } => {
            execute_update_royalty(deps, env, info, fee, recipient)
        }
        ExecuteMsg::ReceiveNft(msg) => execute_receive_cw721(deps, env, info, msg),
        ExecuteMsg::Receive(msg) => execute_receive_cw20(deps, env, info, msg),
        ExecuteMsg::CancelAuction { auction_id } => {
            execute_cancel_auction(deps, env, info, auction_id)
        }
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
            private_sale,
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
            private_sale,
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
    env: Env,
    _info: MessageInfo,
    sender: String,
    sent: Uint128,
    auction_id: u64,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let config = CONFIG.load(deps.storage)?;
    let sender_raw = deps.api.addr_canonicalize(sender.as_ref())?;

    // Verify if the auction id exist
    let item = match ITEMS.may_load(deps.storage, &auction_id.to_be_bytes())? {
        None => Err(ContractError::Unauthorized {}),
        Some(item) => Ok(item),
    }?;
    // Verify if auction ended
    if item.end_time < env.block.time.seconds() {
        return Err(ContractError::EndTimeExpired {});
    }
    // Verify if auction is scheduled to start
    if item.start_time > env.block.time.seconds() {
        return Err(ContractError::AuctionNotStarted {});
    }

    // Handle creator are not bidding
    if item.creator == sender_raw {
        return Err(ContractError::Unauthorized {});
    }

    // Check if the auction have private sale and check if the amount sent is valid
    if item.private_sale {
        // Private sale detected
        // Calculate SITY requirement
        let sity_required = match item.highest_bid {
            None => config.sity_min_opening,
            Some(highest_bid) => config
                .sity_min_opening
                .add(highest_bid.mul(config.sity_fee_registration)),
        };

        // Verify the amount is correct
        if sity_required != sent {
            return Err(ContractError::PrivateSaleRestriction(sity_required));
        }
    } else {
        // No private sale detected
        return Err(ContractError::Unauthorized {});
    }

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
            bid_counter: 0,
            total_bid: Uint128::zero(),
            sity_used: Some(sent),
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
        .add_attribute("sender", sender)
        .add_attribute("amount_required", sent.to_string());

    Ok(res)
}

#[allow(clippy::too_many_arguments)]
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
    private_sale: bool,
) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;

    let sender_raw = deps.api.addr_canonicalize(sender.as_ref())?;
    let contract_raw = deps.api.addr_canonicalize(info.sender.as_ref())?;

    // Handle user are not creating auction inferior limit min time auction
    if env.block.time.plus_seconds(MIN_TIME_AUCTION).seconds() > end_time {
        return Err(ContractError::EndTimeExpired {});
    }
    // Handle user are not creating auction superior limit max time auction
    if env.block.time.plus_seconds(MAX_TIME_AUCTION).seconds() < end_time {
        return Err(ContractError::AuctionLimitReached {});
    }

    let start = match start_time {
        None => env.block.time.seconds(),
        Some(time) => time,
    };

    if start.checked_add(MIN_TIME_AUCTION).unwrap() >= end_time {
        return Err(ContractError::EndTimeExpired {});
    }

    /*
       Query NFT'S
    */
    // let prepare_query_msg = Cw721QueryMsg::NftInfo { token_id: token_id.clone() };
    // let execute_query_msg = WasmQuery::Smart { contract_addr: info.sender.to_string(), msg: to_binary(&prepare_query_msg)? };
    // let query_msg: NftInfoResponse<T>  = deps.querier.query(&execute_query_msg.into())?;

    /*
       check if start_price is less than reserve_price and instant_buy
    */
    if let Some(start_price_amount) = start_price {
        if let Some(instant_buy_amount) = instant_buy {
            if start_price_amount >= instant_buy_amount {
                return Err(ContractError::StartPriceHigherThan(
                    "instant buy".to_string(),
                ));
            }
        }
        if let Some(reserve_price_amount) = reserve_price {
            if start_price_amount > reserve_price_amount {
                return Err(ContractError::StartPriceHigherThan(
                    "reserve price".to_string(),
                ));
            }
        }
    }

    // Validate charity data
    let valid_charity = match charity {
        None => None,
        Some(info) => {
            if info.fee_percentage.is_zero() || info.fee_percentage > Decimal::one() {
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
        Some(instant_buy_price) => {
            if instant_buy_price.is_zero() {
                return Err(ContractError::ZeroNotValid {});
            }
            if let Some(reserve_price_amount) = reserve_price {
                if instant_buy_price < reserve_price_amount {
                    return Err(ContractError::InstantBuyPriceLowerThan(
                        "reserve price".to_string(),
                    ));
                }
            }
            Some(instant_buy_price)
        }
    };

    ITEMS.save(
        deps.storage,
        &state.counter_items.to_be_bytes(),
        &ItemInfo {
            creator: sender_raw,
            start_price,
            start_time: start,
            end_time,
            highest_bid: None,
            highest_bidder: None,
            nft_contract: contract_raw,
            nft_id: token_id.clone(),
            total_bids: 0,
            charity: valid_charity,
            instant_buy: instant_buy_price,
            reserve_price,
            private_sale,
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
        )
        .add_attribute("private_sale", private_sale.to_string());
    Ok(res)
}

pub fn execute_retract_bids(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    auction_id: u64,
) -> Result<Response, ContractError> {
    let sender_raw = deps.api.addr_canonicalize(&info.sender.as_str())?;
    let config = CONFIG.load(deps.storage)?;
    let state = STATE.load(deps.storage)?;

    let item = ITEMS.load(deps.storage, &auction_id.to_be_bytes())?;
    let reserve_price = item.reserve_price.unwrap_or_default();
    let highest_bid = item.highest_bid.unwrap_or_default();

    let bid = BIDS.load(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
    )?;

    // Check if the highest bidder is the sender
    if let Some(highest_bidder) = item.highest_bidder {
        if highest_bidder == sender_raw {
            // Verify if the highest bid is higher or equal the reserve price
            // Meaning the highest bidder is the future owner and unauthorized to retract
            if highest_bid >= reserve_price {
                return Err(ContractError::Unauthorized {});
            }
        }
    }

    // Check total bid is not 0
    if bid.total_bid.is_zero() {
        return Err(ContractError::Unauthorized {});
    }

    let bank_msg = CosmosMsg::Bank(BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![deduct_tax(
            &deps.querier,
            Coin {
                denom: config.denom,
                amount: bid.total_bid,
            },
        )?],
    });
    let mut msgs = vec![bank_msg];

    if !bid.resolved && reserve_price < highest_bid {
        let priv_reward_amount = bid.total_bid.mul(config.sity_partial_rewards);
        let privilege_msg = Cw20ExecuteMsg::Mint {
            recipient: info.sender.to_string(),
            amount: priv_reward_amount,
        };
        let execute_privilege_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: deps.api.addr_humanize(&state.cw20_address)?.to_string(),
            msg: to_binary(&privilege_msg)?,
            funds: vec![],
        });
        msgs.push(execute_privilege_msg);
    }

    BIDS.update(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
        |bid| -> StdResult<BidInfo> {
            let mut update_bid = bid.unwrap();
            update_bid.total_bid = Uint128::zero();
            update_bid.resolved = true;
            Ok(update_bid)
        },
    )?;

    let res = Response::new()
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

    let contract_address = deps.api.addr_humanize(&item.nft_contract)?;
    let minter_msg = cw20_base::msg::QueryMsg::Minter {};
    let wasm = WasmQuery::Smart {
        contract_addr: contract_address.to_string(),
        msg: to_binary(&minter_msg)?,
    };
    let res: cw20_base::state::MinterData =
        deps.querier.query(&wasm.into()).unwrap_or(MinterData {
            minter: Addr::unchecked("talis"),
            cap: None,
        });

    // let minter = if res.unwrap_err() {
    //     let minter_msg = QueryTalisMsg::MintingInfo {};
    //     let wasm = WasmQuery::Smart {
    //         contract_addr: contract_address.to_string(),
    //         msg: to_binary(&minter_msg)?,
    //     };
    //
    //     let res: TalisInfo = deps.querier.query(&wasm.into()).unwrap_or(TalisInfo {
    //         minter: Some("undefined".to_string()),
    //         max_supply: None,
    //     });
    //
    //     if let Some(minter) = res.minter.clone() {
    //         if minter == "undefined" {
    //             None
    //         } else {
    //             Some(deps.api.addr_canonicalize(&res.minter.unwrap())?)
    //         }
    //     } else {
    //         None
    //     }
    // }else {
    //
    //     //Some(deps.api.addr_canonicalize(&res.minter.to_string())?)
    // };

    let minter = if res.minter == "talis" {
        let minter_msg = QueryTalisMsg::MintingInfo {};
        let wasm = WasmQuery::Smart {
            contract_addr: contract_address.to_string(),
            msg: to_binary(&minter_msg)?,
        };

        let res: TalisInfo = deps.querier.query(&wasm.into()).unwrap_or(TalisInfo {
            minter: Some("undefined".to_string()),
            max_supply: None,
        });

        if let Some(minter) = res.minter.clone() {
            if minter == "undefined" {
                None
            } else {
                Some(deps.api.addr_canonicalize(&res.minter.unwrap())?)
            }
        } else {
            None
        }
    } else {
        Some(deps.api.addr_canonicalize(&res.minter.to_string())?)
    };

    // Set the recipient
    let royalty = if let Some(minter) = minter {
        let royalty_info = ROYALTY
            .load(deps.storage, &minter.as_slice())
            .unwrap_or(RoyaltyInfo {
                creator: minter.clone(),
                fee: Decimal::from_str(DEFAULT_ROYALTY_FEE).unwrap(),
                recipient: None,
            });

        let raw_royalty_recipient = if let Some(recipient) = royalty_info.clone().recipient {
            recipient
        } else {
            minter
        };
        Some((raw_royalty_recipient, royalty_info))
    } else {
        None
    };

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
    let mut royalty_fee_amount = Uint128::zero();
    let mut charity_address = None;
    let recipient_address_raw = match item.highest_bidder {
        None => item.creator.clone(),
        Some(address) => match item.reserve_price {
            None => address,
            Some(reserve_price) => match item.highest_bid {
                None => item.creator.clone(),
                Some(highest_bid) => {
                    if reserve_price > highest_bid {
                        item.creator.clone()
                    } else {
                        address
                    }
                }
            },
        },
    };
    let mut highest_bid_amount = Uint128::zero();
    if let Some(highest_bid) = item.highest_bid {
        highest_bid_amount = highest_bid;
        net_amount_after = highest_bid;

        if let Some(royalty) = royalty.clone() {
            // Apply Royalty fee
            royalty_fee_amount = net_amount_after.mul(royalty.1.fee);
        }

        // Apply fee if it is not a private sale or lower fee if it is a private sale
        if !item.private_sale {
            lota_fee_amount = net_amount_after.mul(config.lota_fee);
        } else {
            lota_fee_amount = net_amount_after.mul(config.lota_fee_low);
        }
        net_amount_after = net_amount_after.checked_sub(lota_fee_amount).unwrap();
        net_amount_after = net_amount_after.checked_sub(royalty_fee_amount).unwrap();

        if let Some(charity) = item.charity {
            charity_amount = net_amount_after.mul(charity.fee_percentage);
            net_amount_after = net_amount_after.checked_sub(charity_amount).unwrap();
            charity_address = Some(charity.address);
        }
    }

    ITEMS.update(
        deps.storage,
        &auction_id.to_be_bytes(),
        |item| -> StdResult<ItemInfo> {
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
       Prepare msg to send rewards PRIV token
    */
    // Send to winner and creator if exist
    if recipient_address_raw != item.creator {
        if !highest_bid_amount.is_zero() {
            let priv_reward_amount = highest_bid_amount.mul(config.sity_full_rewards);
            /*
                Prepare msg to mint rewards
            */

            // Send to creator
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: deps.api.addr_humanize(&state.cw20_address)?.to_string(),
                msg: to_binary(&Cw20ExecuteMsg::Mint {
                    recipient: deps.api.addr_humanize(&item.creator)?.to_string(),
                    amount: priv_reward_amount,
                })?,
                funds: vec![],
            }));

            // Send to highest bidder
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: deps.api.addr_humanize(&state.cw20_address)?.to_string(),
                msg: to_binary(&Cw20ExecuteMsg::Mint {
                    recipient: deps.api.addr_humanize(&recipient_address_raw)?.to_string(),
                    amount: priv_reward_amount,
                })?,
                funds: vec![],
            }));
        }

        if !net_amount_after.is_zero() {
            /*
                Prepare msg to send payout to creator
            */
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
           Prepare msg send Royalty to minter
        */
        if !royalty_fee_amount.is_zero() {
            if let Some(royalty) = royalty {
                msgs.push(CosmosMsg::Bank(BankMsg::Send {
                    to_address: deps.api.addr_humanize(&royalty.0)?.to_string(),
                    amount: vec![deduct_tax(
                        &deps.querier,
                        Coin {
                            denom: config.denom.clone(),
                            amount: royalty_fee_amount,
                        },
                    )?],
                }));
            }
        }

        /*
           Prepare msg send to lota
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
        /*
            Prepare msg to send charity if some charity
        */
        if let Some(address) = charity_address {
            if !charity_amount.is_zero() {
                msgs.push(CosmosMsg::Bank(BankMsg::Send {
                    to_address: deps.api.addr_humanize(&address)?.to_string(),
                    amount: vec![deduct_tax(
                        &deps.querier,
                        Coin {
                            denom: config.denom,
                            amount: charity_amount,
                        },
                    )?],
                }));
            }
        }
    }

    let res = Response::new()
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

    // Verify if auction ended
    if item.end_time < env.block.time.seconds() {
        return Err(ContractError::EndTimeExpired {});
    }
    // Verify if auction is started
    if item.start_time > env.block.time.seconds() {
        return Err(ContractError::AuctionNotStarted {});
    }

    // Handle creator are not bidding
    if item.creator == sender_raw {
        return Err(ContractError::Unauthorized {});
    }

    if item.private_sale {
        // Calculate SITY requirement
        let sity_required = match item.highest_bid {
            None => config.sity_min_opening,
            Some(highest_bid) => highest_bid.mul(config.sity_fee_registration),
        };

        // Check if already registered
        if BIDS.may_load(
            deps.storage,
            (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
        )? == None
        {
            return Err(ContractError::PrivateSaleRestriction(sity_required));
        };
    }

    let min_bid = match item.start_price {
        None => {
            let current_bid = item.highest_bid.unwrap_or_default();
            let bid_margin = current_bid.mul(config.bid_margin);
            current_bid.checked_add(bid_margin).unwrap()
        }
        Some(start_price) => {
            if start_price > item.highest_bid.unwrap_or_default() {
                start_price
            } else {
                let current_bid = item.highest_bid.unwrap_or_default();
                let bid_margin = current_bid.mul(config.bid_margin);
                current_bid.checked_add(bid_margin).unwrap()
            }
        }
    };

    let bid_total_sent = match BIDS.may_load(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
    )? {
        None => Some(sent),
        Some(bid_sent) => Some(bid_sent.total_bid.checked_add(sent).unwrap()),
    }
    .unwrap_or(sent);

    if bid_total_sent < min_bid {
        return Err(ContractError::MinBid(min_bid, bid_total_sent));
    }

    ITEMS.update(
        deps.storage,
        &auction_id.to_be_bytes(),
        |item| -> StdResult<ItemInfo> {
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
            // New bid incoming
            updated_item.total_bids += 1;

            // Any bids made in the last 10 minutes of an auction will extend each auction by 10 more minutes.
            if env
                .block
                .time
                .plus_seconds(LAST_MINUTE_BID_EXTRA_TIME)
                .seconds()
                > updated_item.end_time
            {
                updated_item.end_time = updated_item.end_time.add(LAST_MINUTE_BID_EXTRA_TIME);
            }

            Ok(updated_item)
        },
    )?;

    let mut history_sent = sent;
    match BIDS.may_load(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
    )? {
        None => BIDS.save(
            deps.storage,
            (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
            &BidInfo {
                bid_counter: 1,
                total_bid: sent,
                sity_used: None,
                resolved: false,
            },
        )?,
        Some(_) => {
            BIDS.update(
                deps.storage,
                (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
                |bid| -> StdResult<BidInfo> {
                    let mut updated_bid = bid.unwrap();
                    // Update history with sent compounded
                    history_sent = updated_bid.total_bid.checked_add(sent)?;

                    updated_bid.bid_counter += 1;
                    updated_bid.total_bid = updated_bid.total_bid.checked_add(sent)?;
                    Ok(updated_bid)
                },
            )?;
        }
    }

    match HISTORIES_BIDDER.may_load(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
    )? {
        None => HISTORIES_BIDDER.save(
            deps.storage,
            (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
            &HistoryInfo {
                bids: vec![HistoryBidInfo {
                    bidder: sender_raw.clone(),
                    amount: history_sent,
                    time: env.block.time.seconds(),
                    instant_buy: false,
                }],
            },
        )?,
        Some(_) => {
            HISTORIES_BIDDER.update(
                deps.storage,
                (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
                |hist| -> StdResult<HistoryInfo> {
                    let mut updated_hist = hist.unwrap();
                    updated_hist.bids.push(HistoryBidInfo {
                        bidder: sender_raw.clone(),
                        amount: history_sent,
                        time: env.block.time.seconds(),
                        instant_buy: false,
                    });
                    Ok(updated_hist)
                },
            )?;
        }
    }

    match HISTORIES.may_load(deps.storage, &auction_id.to_be_bytes())? {
        None => HISTORIES.save(
            deps.storage,
            &auction_id.to_be_bytes(),
            &HistoryInfo {
                bids: vec![HistoryBidInfo {
                    bidder: sender_raw,
                    amount: history_sent,
                    time: env.block.time.seconds(),
                    instant_buy: false,
                }],
            },
        )?,
        Some(_) => {
            HISTORIES.update(
                deps.storage,
                &auction_id.to_be_bytes(),
                |hist| -> StdResult<HistoryInfo> {
                    let mut updated_hist = hist.unwrap();
                    updated_hist.bids.push(HistoryBidInfo {
                        bidder: sender_raw,
                        amount: history_sent,
                        time: env.block.time.seconds(),
                        instant_buy: false,
                    });
                    Ok(updated_hist)
                },
            )?;
        }
    }

    let res = Response::new()
        .add_attribute("new_bid", history_sent.to_string())
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

    if item.private_sale {
        // Calculate SITY requirement
        let sity_required = match item.highest_bid {
            None => config.sity_min_opening,
            Some(highest_bid) => highest_bid.mul(config.sity_fee_registration),
        };
        if BIDS.may_load(
            deps.storage,
            (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
        )? == None
        {
            return Err(ContractError::PrivateSaleRestriction(sity_required));
        };
    }

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

    let mut history_sent = sent;
    match BIDS.may_load(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
    )? {
        None => BIDS.save(
            deps.storage,
            (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
            &BidInfo {
                bid_counter: 1,
                total_bid: sent,
                sity_used: None,
                resolved: false,
            },
        )?,
        Some(_) => {
            BIDS.update(
                deps.storage,
                (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
                |bid| -> StdResult<BidInfo> {
                    let mut updated_bid = bid.unwrap();
                    // Update history with sent compounded
                    history_sent = updated_bid.total_bid.checked_add(sent)?;

                    updated_bid.bid_counter += 1;
                    updated_bid.total_bid = updated_bid.total_bid.checked_add(sent)?;
                    Ok(updated_bid)
                },
            )?;
        }
    }

    let instant_buy_amount = match item.instant_buy {
        None => Err(ContractError::Unauthorized {}),
        Some(amount) => {
            if amount != history_sent {
                return Err(ContractError::InaccurateFunds(amount, history_sent));
            }
            Ok(amount)
        }
    }?;

    ITEMS.update(
        deps.storage,
        &auction_id.to_be_bytes(),
        |item_info| -> StdResult<ItemInfo> {
            let mut updated_item = item_info.unwrap();
            updated_item.end_time = env.block.time.minus_seconds(MIN_TIME_AUCTION).seconds();
            updated_item.highest_bid = Some(instant_buy_amount);
            updated_item.highest_bidder = Some(sender_raw.clone());
            updated_item.total_bids += 1;

            Ok(updated_item)
        },
    )?;

    match HISTORIES_BIDDER.may_load(
        deps.storage,
        (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
    )? {
        None => HISTORIES_BIDDER.save(
            deps.storage,
            (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
            &HistoryInfo {
                bids: vec![HistoryBidInfo {
                    bidder: sender_raw.clone(),
                    amount: history_sent,
                    time: env.block.time.seconds(),
                    instant_buy: true,
                }],
            },
        )?,
        Some(_) => {
            HISTORIES_BIDDER.update(
                deps.storage,
                (&auction_id.to_be_bytes(), &sender_raw.as_slice()),
                |hist| -> StdResult<HistoryInfo> {
                    let mut updated_hist = hist.unwrap();
                    updated_hist.bids.push(HistoryBidInfo {
                        bidder: sender_raw.clone(),
                        amount: history_sent,
                        time: env.block.time.seconds(),
                        instant_buy: true,
                    });
                    Ok(updated_hist)
                },
            )?;
        }
    }

    match HISTORIES.may_load(deps.storage, &auction_id.to_be_bytes())? {
        None => HISTORIES.save(
            deps.storage,
            &auction_id.to_be_bytes(),
            &HistoryInfo {
                bids: vec![HistoryBidInfo {
                    bidder: sender_raw,
                    amount: sent,
                    time: env.block.time.seconds(),
                    instant_buy: true,
                }],
            },
        )?,
        Some(_) => {
            HISTORIES.update(
                deps.storage,
                &auction_id.to_be_bytes(),
                |hist| -> StdResult<HistoryInfo> {
                    let mut updated_hist = hist.unwrap();
                    updated_hist.bids.push(HistoryBidInfo {
                        bidder: sender_raw,
                        amount: history_sent,
                        time: env.block.time.seconds(),
                        instant_buy: true,
                    });
                    Ok(updated_hist)
                },
            )?;
        }
    }

    let res = Response::new()
        .add_attribute("instant_buy", "NFT")
        .add_attribute("nft_id", item.nft_id)
        .add_attribute("auction_id", auction_id.to_string());
    Ok(res)
}

pub fn execute_update_royalty(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    fee: Decimal,
    recipient: Option<String>,
) -> Result<Response, ContractError> {
    let raw_sender = deps.api.addr_canonicalize(info.sender.as_str())?;
    // Handle not abusive Royalty
    if fee > Decimal::from_str(ROYALTY_MAX_FEE).unwrap() {
        return Err(ContractError::MaxRoyaltyReached {});
    }
    // Handle recipient
    let set_recipient = if let Some(address) = recipient.clone() {
        deps.api
            .addr_canonicalize(deps.api.addr_validate(&address)?.as_str())?
    } else {
        raw_sender.clone()
    };

    match ROYALTY.may_load(deps.storage, &raw_sender.as_ref())? {
        None => {
            ROYALTY.save(
                deps.storage,
                &raw_sender.as_ref(),
                &RoyaltyInfo {
                    creator: raw_sender.clone(),
                    fee,
                    recipient: Some(set_recipient.clone()),
                },
            )?;
        }
        Some(_) => {
            ROYALTY.update(
                deps.storage,
                &raw_sender.as_ref(),
                |royalty| -> StdResult<RoyaltyInfo> {
                    let mut updated_royalty = royalty.unwrap();

                    if recipient.is_some() {
                        updated_royalty.recipient = Some(set_recipient.clone());
                    }
                    updated_royalty.fee = fee;

                    Ok(updated_royalty)
                },
            )?;
        }
    };

    let recipient = deps.api.addr_humanize(&set_recipient).unwrap();
    let res = Response::new()
        .add_attribute("update_royalty", info.sender.to_string())
        .add_attribute("royalty_fee", fee.to_string())
        .add_attribute("recipient", recipient.to_string());
    Ok(res)
}

pub fn execute_cancel_auction(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    auction_id: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let cancellation = CANCELLATION.load(deps.storage)?;

    let item = match ITEMS.may_load(deps.storage, &auction_id.to_be_bytes())? {
        None => return Err(ContractError::Unauthorized {}),
        Some(auction) => auction,
    };
    // Verify if auction ended
    if env.block.time.seconds() > item.end_time {
        return Err(ContractError::EndTimeExpired {});
    }

    let raw_sender = deps.api.addr_canonicalize(info.sender.as_str())?;
    if raw_sender != item.creator {
        return Err(ContractError::Unauthorized {});
    }
    let mut msgs = vec![];
    // Check if this auction need fees
    let fee_indicator = if let Some(highest_bid) = item.highest_bid {
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

        let cancellation_fee = highest_bid.mul(cancellation.cancellation_fee);

        if sent != cancellation_fee {
            return Err(ContractError::CancelAuctionFee(
                cancellation_fee.to_string(),
                config.denom,
            ));
        }

        // Fee for highest bidder
        if let Some(highest_bidder) = item.highest_bidder {
            // send split amount
            let split_amount = Decimal::from_str("0.5").unwrap();
            // prepare message for highest bidder
            msgs.push(CosmosMsg::Bank(BankMsg::Send {
                to_address: deps.api.addr_humanize(&highest_bidder)?.to_string(),
                amount: vec![deduct_tax(
                    &deps.querier,
                    Coin {
                        denom: config.denom.clone(),
                        amount: cancellation_fee.mul(split_amount),
                    },
                )?],
            }));
            // prepare message for fee recipient
            msgs.push(CosmosMsg::Bank(BankMsg::Send {
                to_address: deps.api.addr_humanize(&config.lota_contract)?.to_string(),
                amount: vec![deduct_tax(
                    &deps.querier,
                    Coin {
                        denom: config.denom.clone(),
                        amount: cancellation_fee.mul(split_amount),
                    },
                )?],
            }));
        } else {
            // Send the full amount
            msgs.push(CosmosMsg::Bank(BankMsg::Send {
                to_address: deps.api.addr_humanize(&config.lota_contract)?.to_string(),
                amount: vec![deduct_tax(
                    &deps.querier,
                    Coin {
                        denom: config.denom.clone(),
                        amount: cancellation_fee,
                    },
                )?],
            }));
        }
        // Return cancellation_fee
        cancellation_fee
    } else {
        Uint128::zero()
    };

    ITEMS.update(
        deps.storage,
        &auction_id.to_be_bytes(),
        |item| -> StdResult<ItemInfo> {
            let mut updated_item = item.unwrap();
            updated_item.end_time = env.block.time.minus_seconds(MIN_TIME_AUCTION).seconds();
            //updated_item.highest_bid = None;
            updated_item.highest_bidder = None;

            Ok(updated_item)
        },
    )?;

    let res = Response::new()
        .add_messages(msgs)
        .add_attribute("action", "cancel_auction".to_string())
        .add_attribute("auction_id", auction_id.to_string())
        .add_attribute("cancellation_fee", fee_indicator.to_string());
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
                        .map(|addr| addr.value)
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
        QueryMsg::HistoryBids { auction_id } => to_binary(&query_bids(deps, env, auction_id)?),
        QueryMsg::HistoryBidderBids {
            auction_id,
            address,
        } => to_binary(&query_bidder_bids(deps, env, auction_id, address)?),
        QueryMsg::AllAuctions { start_after, limit } => {
            to_binary(&query_all_auctions(deps, start_after, limit)?)
        }
        QueryMsg::Royalty { address } => to_binary(&query_royalty(deps, env, address)?),
    }
}

const DEFAULT_LIMIT: u32 = 10;
const MAX_LIMIT: u32 = 30;
fn query_all_auctions(
    deps: Deps,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> StdResult<AllAuctionsResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;
    let start = start_after.map(|d| Bound::Exclusive(d.to_be_bytes().to_vec()));

    let items = ITEMS
        .range(deps.storage, start, None, Order::Ascending)
        .take(limit)
        .map(|pair| {
            pair.and_then(|(k, item)| {
                let highest_bidder = match item.highest_bidder {
                    None => None,
                    Some(highest_bidder) => {
                        Some(deps.api.addr_humanize(&highest_bidder)?.to_string())
                    }
                };
                let charity = match item.charity {
                    None => None,
                    Some(charity) => Some(CharityResponse {
                        address: deps.api.addr_humanize(&charity.address)?.to_string(),
                        fee_percentage: charity.fee_percentage,
                    }),
                };

                Ok((
                    u64::from_be_bytes(k.try_into().unwrap()),
                    AuctionResponse {
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
                        private_sale: item.private_sale,
                        resolved: item.resolved,
                    },
                ))
            })
        })
        .collect::<StdResult<Vec<(u64, AuctionResponse)>>>();

    Ok(AllAuctionsResponse { auctions: items? })
}

fn query_bids(deps: Deps, _env: Env, auction_id: u64) -> StdResult<HistoryResponse> {
    let history_info = match HISTORIES.may_load(deps.storage, &auction_id.to_be_bytes())? {
        None => None,
        Some(history) => Some(history),
    };
    let mut hist = vec![];
    if let Some(history) = history_info {
        hist = history
            .bids
            .into_iter()
            .map(|hist| HistoryBidResponse {
                bidder: deps.api.addr_humanize(&hist.bidder).unwrap().to_string(),
                amount: hist.amount,
                time: hist.time,
                instant_buy: hist.instant_buy,
            })
            .collect::<Vec<HistoryBidResponse>>();
    }

    Ok(HistoryResponse { bids: hist })
}

fn query_bidder_bids(
    deps: Deps,
    _env: Env,
    auction_id: u64,
    address: String,
) -> StdResult<HistoryResponse> {
    let addr_raw = deps.api.addr_canonicalize(&address)?;
    let history_info = match HISTORIES_BIDDER.may_load(
        deps.storage,
        (&auction_id.to_be_bytes(), &addr_raw.as_slice()),
    )? {
        None => None,
        Some(history) => Some(history),
    };
    let mut hist = vec![];
    if let Some(history) = history_info {
        hist = history
            .bids
            .into_iter()
            .map(|hist| HistoryBidResponse {
                bidder: deps.api.addr_humanize(&hist.bidder).unwrap().to_string(),
                amount: hist.amount,
                time: hist.time,
                instant_buy: hist.instant_buy,
            })
            .collect::<Vec<HistoryBidResponse>>();
    }

    Ok(HistoryResponse { bids: hist })
}

fn query_config(deps: Deps, _env: Env) -> StdResult<ConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    let cancellation = CANCELLATION.load(deps.storage)?;
    Ok(ConfigResponse {
        denom: config.denom,
        bid_margin: config.bid_margin,
        lota_fee: config.lota_fee,
        lota_fee_low: config.lota_fee_low,
        lota_contract: deps.api.addr_humanize(&config.lota_contract)?.to_string(),
        sity_full_rewards: config.sity_full_rewards,
        sity_partial_rewards: config.sity_partial_rewards,
        sity_fee_registration: config.sity_fee_registration,
        sity_min_opening: config.sity_min_opening,
        cancellation_fee: cancellation.cancellation_fee,
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
        private_sale: item.private_sale,
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
        None => BidResponse {
            bid_counter: 0,
            total_bid: Uint128::zero(),
            sity_used: None,
        },
        Some(bid) => BidResponse {
            bid_counter: bid.bid_counter,
            total_bid: bid.total_bid,
            sity_used: bid.sity_used,
        },
    };

    Ok(bid)
}

fn query_royalty(deps: Deps, _env: Env, address: String) -> StdResult<RoyaltyResponse> {
    let raw_address = deps.api.addr_canonicalize(&address.as_str())?;
    let store = ROYALTY
        .load(deps.storage, &raw_address.as_slice())
        .unwrap_or(RoyaltyInfo {
            creator: raw_address,
            fee: Decimal::from_str(DEFAULT_ROYALTY_FEE).unwrap(),
            recipient: None,
        });
    let recipient = match store.recipient {
        None => None,
        Some(raw_recipient) => Some(deps.api.addr_humanize(&raw_recipient)?.to_string()),
    };

    Ok(RoyaltyResponse {
        creator: deps.api.addr_humanize(&store.creator)?.to_string(),
        fee: store.fee,
        recipient,
    })
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> StdResult<Response> {
    // let cancellation = Cancellation {
    //     cancellation_fee: Decimal::from_str("0.1").unwrap(),
    // };
    // CANCELLATION.save(deps.storage, &cancellation)?;
    // store.lota_contract = deps.api.addr_canonicalize(Addr::unchecked("terra1suetgdll2ra65hzdp3yfzafkq8zwdktht6aqdq").as_ref())?;
    // CONFIG.save(deps.storage, &store)?;
    // let highest_bidder = deps.api.addr_canonicalize(
    //     Addr::unchecked("terra1suetgdll2ra65hzdp3yfzafkq8zwdktht6aqdq").as_ref(),
    // )?;
    // ITEMS.update(
    //     deps.storage,
    //     &94_u64.to_be_bytes(),
    //     |item| -> StdResult<ItemInfo> {
    //         let mut updated_item = item.unwrap();
    //         updated_item.highest_bidder = Some(highest_bidder);
    //         Ok(updated_item)
    //     },
    // )?;
    Ok(Response::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ContractError::MinBid;
    use crate::mock_querier::mock_dependencies_custom;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, MOCK_CONTRACT_ADDR};
    use cosmwasm_std::{coins, from_binary, Api, Attribute, Decimal, ReplyOn, StdError};
    use cw20::Cw20ExecuteMsg;
    use std::str::FromStr;

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies(&[]);
        let info = mock_info("creator", &[]);
        let env = mock_env();

        let msg = InstantiateMsg {
            denom: "uusd".to_string(),
            cw20_code_id: 9,
            cw20_label: "cw20".to_string(),
            bid_margin: Decimal::from_str("0.05").unwrap(),
            lota_fee: Decimal::from_str("0.05").unwrap(),
            lota_fee_low: Decimal::from_str("0.0175").unwrap(),
            lota_contract: "loterra".to_string(),
            sity_full_rewards: Decimal::from_str("0.10").unwrap(),
            sity_partial_rewards: Decimal::from_str("0.01").unwrap(),
            sity_fee_registration: Decimal::from_str("0.02").unwrap(),
            sity_min_opening: Uint128::from(1_000_000u128),
        };

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), env, info, msg).unwrap();
        assert_eq!(1, res.messages.len());
    }

    fn init_default(deps: DepsMut) {
        let msg = InstantiateMsg {
            denom: "uusd".to_string(),
            cw20_code_id: 9,
            cw20_label: "cw20".to_string(),
            bid_margin: Decimal::from_str("0.05").unwrap(),
            lota_fee: Decimal::from_str("0.05").unwrap(),
            lota_fee_low: Decimal::from_str("0.0175").unwrap(),
            lota_contract: "loterra".to_string(),
            sity_full_rewards: Decimal::from_str("0.10").unwrap(),
            sity_partial_rewards: Decimal::from_str("0.01").unwrap(),
            sity_fee_registration: Decimal::from_str("0.02").unwrap(),
            sity_min_opening: Uint128::from(1_000_000u128),
        };

        let info = mock_info("creator", &vec![]);
        let _res = instantiate(deps, mock_env(), info, msg).unwrap();
    }

    fn create_msg_nft(
        start_price: Option<Uint128>,
        start_time: Option<u64>,
        end_time: u64,
        charity: Option<CharityResponse>,
        instant_buy: Option<Uint128>,
        reserve_price: Option<Uint128>,
        private_sale: bool,
    ) -> Result<ExecuteMsg, ContractError> {
        let msg = ReceiveMsg::CreateAuctionNft {
            start_price,
            start_time,
            end_time,
            charity,
            instant_buy,
            reserve_price,
            private_sale,
        };
        let send_msg = cw721::Cw721ReceiveMsg {
            sender: "sender".to_string(),
            token_id: "test".to_string(),
            msg: to_binary(&msg).unwrap(),
        };
        Ok(ExecuteMsg::ReceiveNft(send_msg))
    }

    #[test]
    fn create_auction() {
        let mut deps = mock_dependencies(&coins(2, "token"));
        init_default(deps.as_mut());

        // ERROR create auction with end_time inferior current time
        let execute_msg = create_msg_nft(None, None, 0, None, None, None, false).unwrap();
        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap_err();

        // ERROR create auction with end_time superior 6 month current time
        let execute_msg =
            create_msg_nft(None, None, 1000000000000000, None, None, None, false).unwrap();
        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap_err();

        // ERROR create auction with time end == time start
        let env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            Some(env.block.time.plus_seconds(1000).seconds()),
            env.block.time.plus_seconds(1000).seconds(),
            None,
            None,
            None,
            false,
        )
        .unwrap();
        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap_err();

        // ERROR create auction with time end superior now but with Option Charity info wrong fee_percentage
        let env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            Some(CharityResponse {
                address: "angel".to_string(),
                fee_percentage: Decimal::from_str("1.1").unwrap(),
            }),
            None,
            None,
            false,
        )
        .unwrap();

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap_err();

        // create auction with time end superior now but without options
        let env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            None,
            None,
            false,
        )
        .unwrap();
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
        assert_eq!(item.start_time, mock_env().block.time.seconds());
        assert_eq!(item.start_price, None);
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, None);
        assert_eq!(item.end_time, env.block.time.plus_seconds(1000).seconds());
        assert_eq!(item.private_sale, false);
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
                },
                Attribute {
                    key: "private_sale".to_string(),
                    value: false.to_string()
                }
            ]
        );

        // create auction with time end superior now but with Option start price
        let env = mock_env();
        let execute_msg = create_msg_nft(
            Some(Uint128::from(1000_u128)),
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            None,
            None,
            false,
        )
        .unwrap();
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
        assert_eq!(item.start_time, mock_env().block.time.seconds());
        assert_eq!(item.start_price, Some(Uint128::from(1000_u128)));
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, None);
        assert_eq!(item.end_time, env.block.time.plus_seconds(1000).seconds());
        assert_eq!(item.private_sale, false);
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
                },
                Attribute {
                    key: "private_sale".to_string(),
                    value: false.to_string()
                }
            ]
        );

        // create auction with time end superior now but with Option start price
        let env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            Some(env.block.time.plus_seconds(2000).seconds()),
            env.block.time.plus_seconds(5000).seconds(),
            None,
            None,
            None,
            false,
        )
        .unwrap();
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
        assert_eq!(item.start_time, env.block.time.plus_seconds(2000).seconds());
        assert_eq!(item.start_price, None);
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, None);
        assert_eq!(item.end_time, env.block.time.plus_seconds(5000).seconds());
        assert_eq!(item.private_sale, false);
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
                },
                Attribute {
                    key: "private_sale".to_string(),
                    value: false.to_string()
                }
            ]
        );

        // create auction with time end superior now but with Option instant buy
        let env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            Some(Uint128::from(1000_u128)),
            None,
            false,
        )
        .unwrap();
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
        assert_eq!(item.start_time, mock_env().block.time.seconds());
        assert_eq!(item.start_price, None);
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, None);
        assert_eq!(item.end_time, env.block.time.plus_seconds(1000).seconds());
        assert_eq!(item.private_sale, false);
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
                },
                Attribute {
                    key: "private_sale".to_string(),
                    value: false.to_string()
                }
            ]
        );

        // create auction with time end superior now but with Option reserve price
        let env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            None,
            Some(Uint128::from(1000_u128)),
            false,
        )
        .unwrap();
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
        assert_eq!(item.start_time, mock_env().block.time.seconds());
        assert_eq!(item.start_price, None);
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, Some(Uint128::from(1000_u128)));
        assert_eq!(item.end_time, env.block.time.plus_seconds(1000).seconds());
        assert_eq!(item.private_sale, false);
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
                },
                Attribute {
                    key: "private_sale".to_string(),
                    value: false.to_string()
                }
            ]
        );

        // create auction with time end superior now but with Option privilege sale
        let env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            None,
            None,
            true,
        )
        .unwrap();
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
        assert_eq!(item.start_time, mock_env().block.time.seconds());
        assert_eq!(item.start_price, None);
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, None);
        assert_eq!(item.end_time, env.block.time.plus_seconds(1000).seconds());
        assert_eq!(item.private_sale, true);
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
                },
                Attribute {
                    key: "private_sale".to_string(),
                    value: true.to_string()
                }
            ]
        );

        // create auction with time end superior now but with Option Charity info
        let env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            Some(CharityResponse {
                address: "angel".to_string(),
                fee_percentage: Decimal::from_str("0.10").unwrap(),
            }),
            None,
            None,
            false,
        )
        .unwrap();

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
        assert_eq!(item.start_time, mock_env().block.time.seconds());
        assert_eq!(item.start_price, None);
        assert_eq!(item.highest_bidder, None);
        assert_eq!(item.reserve_price, None);
        assert_eq!(item.end_time, env.block.time.plus_seconds(1000).seconds());
        assert_eq!(item.private_sale, false);
        assert_eq!(item.total_bids, 0);
        assert_eq!(item.instant_buy, None);
        assert_eq!(
            item.charity,
            Some(CharityInfo {
                address: deps
                    .api
                    .addr_canonicalize(&deps.api.addr_validate("angel").unwrap().to_string())
                    .unwrap(),
                fee_percentage: Decimal::from_str("0.10").unwrap()
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
                },
                Attribute {
                    key: "private_sale".to_string(),
                    value: false.to_string()
                }
            ]
        );

        /*
           TODO: Get current auction with limit
        */
        let msg = query_all_auctions(deps.as_ref(), Some(5), None);
        println!("Query auctions");
        println!("{:?}", msg);
    }

    #[test]
    fn place_bid_auction() {
        let mut deps = mock_dependencies_custom(&coins(2, "token"));
        init_default(deps.as_mut());

        // Create auction with end_time inferior current time
        let env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            None,
            None,
            false,
        )
        .unwrap();
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("market", &vec![]),
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
        let execute_msg = create_msg_nft(
            Some(Uint128::from(1000_u128)),
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            None,
            None,
            false,
        )
        .unwrap();
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("market", &vec![]),
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
        let msg = ExecuteMsg::RetractBids { auction_id: 1 };
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
        let msg = ExecuteMsg::RetractBids { auction_id: 1 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("sam", &vec![]),
            msg.clone(),
        )
        .unwrap();
        let mint_msg = Cw20ExecuteMsg::Mint {
            recipient: "sam".to_string(),
            amount: Uint128::from(21u128),
        };
        let cosmos_mint_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: "cosmos2contract".to_string(),
            msg: to_binary(&mint_msg).unwrap(),
            funds: vec![],
        });
        let cosmos_bank_msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: "sam".to_string(),
            amount: vec![Coin {
                denom: "uusd".to_string(),
                amount: Uint128::from(2079u128),
            }],
        });
        assert_eq!(
            res.messages,
            vec![SubMsg::new(cosmos_bank_msg), SubMsg::new(cosmos_mint_msg)]
        );

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
        assert_eq!(bid_alice.bid_counter, 2);
        assert_eq!(bid_alice.sity_used, None);

        let bob_raw = deps.api.addr_canonicalize("bob").unwrap();
        let bid_bob = BIDS
            .load(
                deps.as_ref().storage,
                (&1_u64.to_be_bytes(), &bob_raw.as_slice()),
            )
            .unwrap();
        assert_eq!(bid_bob.total_bid, Uint128::from(2000_u128));
        assert_eq!(bid_bob.bid_counter, 1);
        assert_eq!(bid_bob.sity_used, None);

        let item = ITEMS
            .load(deps.as_ref().storage, &1_u64.to_be_bytes())
            .unwrap();
        assert_eq!(item.highest_bidder, Some(alice_raw));
        assert_eq!(item.highest_bid, Some(Uint128::from(2205_u128)));
        assert_eq!(item.total_bids, 4);

        // ERROR Alice try to retire bids because she is the winner
        let msg = ExecuteMsg::RetractBids { auction_id: 1 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("alice", &vec![]),
            msg.clone(),
        )
        .unwrap_err();

        // SUCCESS bob to retire bids
        let msg = ExecuteMsg::RetractBids { auction_id: 1 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("bob", &vec![]),
            msg.clone(),
        )
        .unwrap();

        let mint_msg = Cw20ExecuteMsg::Mint {
            recipient: "bob".to_string(),
            amount: Uint128::from(20u128),
        };
        let cosmos_mint_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: "cosmos2contract".to_string(),
            msg: to_binary(&mint_msg).unwrap(),
            funds: vec![],
        });
        let cosmos_bank_msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: "bob".to_string(),
            amount: vec![Coin {
                denom: "uusd".to_string(),
                amount: Uint128::from(1980u128),
            }],
        });
        assert_eq!(
            res.messages,
            vec![SubMsg::new(cosmos_bank_msg), SubMsg::new(cosmos_mint_msg)]
        );

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
    }

    #[test]
    fn extend_bid_last_minute() {
        //Any bids made in the last 10 minutes of an auction will extend each auction by 10 more minutes.
        let mut deps = mock_dependencies_custom(&coins(2, "token"));
        init_default(deps.as_mut());

        // Create auction with end_time
        let mut env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            Some(CharityResponse {
                address: "charity".to_string(),
                fee_percentage: Decimal::from_str("0.10").unwrap(),
            }),
            None,
            Some(Uint128::from(1000u128)),
            false,
        )
        .unwrap();

        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();
        let prev_item_info = ITEMS
            .load(deps.as_ref().storage, &0_u64.to_be_bytes())
            .unwrap();

        /*
           Place bid
        */
        let execute_msg = ExecuteMsg::PlaceBid { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "bob",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(100u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap();
        let item_info = ITEMS
            .load(deps.as_ref().storage, &0_u64.to_be_bytes())
            .unwrap();
        assert_eq!(prev_item_info.end_time, item_info.end_time);

        //Last minute bid incoming will extend 10 min more the sell ending
        env.block.time = env.block.time.plus_seconds(401);

        /*
           Place bid
        */
        let execute_msg = ExecuteMsg::PlaceBid { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "alice",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(105u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap();

        let item_info = ITEMS
            .load(deps.as_ref().storage, &0_u64.to_be_bytes())
            .unwrap();
        assert_ne!(prev_item_info.end_time, item_info.end_time);
    }
    #[test]
    fn retract_bid_reserve_not_met() {
        let mut deps = mock_dependencies_custom(&coins(2, "token"));
        init_default(deps.as_mut());

        // Create auction with end_time
        let mut env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            Some(CharityResponse {
                address: "charity".to_string(),
                fee_percentage: Decimal::from_str("0.10").unwrap(),
            }),
            None,
            Some(Uint128::from(1000u128)),
            false,
        )
        .unwrap();
        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();
        /*
           Place bid
        */
        let execute_msg = ExecuteMsg::PlaceBid { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "alice",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(100u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap();

        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "bob",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(500u128),
                }],
            ),
            execute_msg,
        )
        .unwrap();
        //expire the auction
        env.block.time = env.block.time.plus_seconds(2000);
        /*
           Max bidder retract bid
        */

        let execute_msg = ExecuteMsg::RetractBids { auction_id: 0 };

        // Alice retract his bid and no SITY minted since retract are not met
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("alice", &vec![]),
            execute_msg.clone(),
        )
        .unwrap();
        let cosmos_msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: "alice".to_string(),
            amount: vec![Coin {
                denom: "uusd".to_string(),
                amount: Uint128::from(99u128),
            }],
        });
        assert_eq!(res.messages, vec![SubMsg::new(cosmos_msg)]);

        // Bob the highest bidder also can retract the bid
        // reserve price was not met
        // and no SITY minted since retract are not met
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("bob", &vec![]),
            execute_msg.clone(),
        )
        .unwrap();
        let cosmos_msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: "bob".to_string(),
            amount: vec![Coin {
                denom: "uusd".to_string(),
                amount: Uint128::from(495u128),
            }],
        });
        assert_eq!(res.messages, vec![SubMsg::new(cosmos_msg)]);

        /*
           Creator withdraw the nft since reserve price not met
        */
        let execute_msg = ExecuteMsg::WithdrawNft { auction_id: 0 };

        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("bob", &vec![]),
            execute_msg.clone(),
        )
        .unwrap();

        let transfer_msg = cw721::Cw721ExecuteMsg::TransferNft {
            recipient: "sender".to_string(),
            token_id: "test".to_string(),
        };
        let cosmos_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: "market".to_string(),
            msg: to_binary(&transfer_msg).unwrap(),
            funds: vec![],
        });
        assert_eq!(res.messages, vec![SubMsg::new(cosmos_msg)])
    }

    #[test]
    fn withdraw_nft() {
        let mut deps = mock_dependencies_custom(&coins(2, "token"));
        init_default(deps.as_mut());

        // Create auction with end_time
        let mut env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            None,
            None,
            false,
        )
        .unwrap();
        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();
        // Update Royalty
        let royalty_msg = ExecuteMsg::UpdateRoyalty {
            fee: Decimal::from_str("0.1").unwrap(),
            recipient: None,
        };
        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("terrans", &vec![]),
            royalty_msg,
        )
        .unwrap();
        /*
           Place bid
        */
        let execute_msg = ExecuteMsg::PlaceBid { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "alice",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(100_000_000u128),
                }],
            ),
            execute_msg,
        )
        .unwrap();
        //expire the auction
        env.block.time = env.block.time.plus_seconds(2000);
        /*
           Withdraw NFT
        */

        //  Withdraw NFT to winner
        let msg = ExecuteMsg::WithdrawNft { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("alice", &vec![]),
            msg.clone(),
        )
        .unwrap();

        let prepare_msg = cw721::Cw721ExecuteMsg::TransferNft {
            recipient: "alice".to_string(),
            token_id: "test".to_string(),
        };
        let message_one = WasmMsg::Execute {
            contract_addr: "market".to_string(),
            msg: to_binary(&prepare_msg).unwrap(),
            funds: vec![],
        };
        let prepare_msg = cw20::Cw20ExecuteMsg::Mint {
            recipient: "sender".to_string(),
            amount: Uint128::from(10_000_000u128),
        };
        let message_two = WasmMsg::Execute {
            contract_addr: "cosmos2contract".to_string(),
            msg: to_binary(&prepare_msg).unwrap(),
            funds: vec![],
        };
        let prepare_msg = cw20::Cw20ExecuteMsg::Mint {
            recipient: "alice".to_string(),
            amount: Uint128::from(10_000_000u128),
        };
        let message_three = WasmMsg::Execute {
            contract_addr: "cosmos2contract".to_string(),
            msg: to_binary(&prepare_msg).unwrap(),
            funds: vec![],
        };
        let message_four = CosmosMsg::Bank(BankMsg::Send {
            to_address: "sender".to_string(),
            amount: vec![Coin {
                denom: "uusd".to_string(),
                amount: Uint128::from(84_158_415u128),
            }],
        });
        let message_five = CosmosMsg::Bank(BankMsg::Send {
            to_address: "terrans".to_string(),
            amount: vec![Coin {
                denom: "uusd".to_string(),
                amount: Uint128::from(9_900_990u128),
            }],
        });
        let message_six = CosmosMsg::Bank(BankMsg::Send {
            to_address: "loterra".to_string(),
            amount: vec![Coin {
                denom: "uusd".to_string(),
                amount: Uint128::from(4_950_495u128),
            }],
        });

        let all_msg = vec![
            SubMsg::new(message_one),
            SubMsg::new(message_two),
            SubMsg::new(message_three),
            SubMsg::new(message_four),
            SubMsg::new(message_five),
            SubMsg::new(message_six),
        ];
        assert_eq!(res.messages, all_msg);

        // Instantiate with start price 1000 ust
        let env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            Some(Uint128::from(10_000_u128)),
            None,
            false,
        )
        .unwrap();
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();

        let execute_msg = ExecuteMsg::PlaceBid { auction_id: 1 };
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
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            None,
            None,
            true,
        )
        .unwrap();
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();

        let execute_msg = ExecuteMsg::PlaceBid { auction_id: 2 };
        // ERROR Private sale registration required
        let err = execute(
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
        assert_eq!(
            err,
            ContractError::PrivateSaleRestriction(Uint128::from(1_000_000u128))
        );

        // Success Alice private sale registered
        let msg = ExecuteMsg::Receive(Cw20ReceiveMsg {
            sender: "alice".to_string(),
            amount: Uint128::from(1_000_000u128),
            msg: to_binary(&ReceiveMsg::RegisterPrivateSale { auction_id: 2 }).unwrap(),
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

        let mut all_vecs = vec![];
        let prepare_burn = cw20::Cw20ExecuteMsg::Burn {
            amount: Uint128::from(1_000_000u128),
        };
        let exec_burn = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: "cosmos2contract".to_string(),
            msg: to_binary(&prepare_burn).unwrap(),
            funds: vec![],
        });
        all_vecs.push(SubMsg::new(exec_burn));
        assert_eq!(res.messages, all_vecs);

        // ERROR Register multiple time
        let err = execute(
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
        assert_eq!(err, ContractError::Unauthorized {});

        // ALICE place a bid
        let message_bid = ExecuteMsg::PlaceBid { auction_id: 2 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "alice",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(1_545_000_000u128),
                }],
            ),
            message_bid,
        )
        .unwrap();

        // ERROR Send less
        let msg = ExecuteMsg::Receive(Cw20ReceiveMsg {
            sender: "bob".to_string(),
            amount: Uint128::from(5_000u128),
            msg: to_binary(&ReceiveMsg::RegisterPrivateSale { auction_id: 2 }).unwrap(),
        });

        let err = execute(
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
        assert_eq!(
            err,
            ContractError::PrivateSaleRestriction(Uint128::from(31_900_000u128))
        );

        // ERROR Send more
        let msg = ExecuteMsg::Receive(Cw20ReceiveMsg {
            sender: "bob".to_string(),
            amount: Uint128::from(33_900_000u128),
            msg: to_binary(&ReceiveMsg::RegisterPrivateSale { auction_id: 2 }).unwrap(),
        });

        let err = execute(
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
        assert_eq!(
            err,
            ContractError::PrivateSaleRestriction(Uint128::from(31_900_000u128))
        );

        // 1_545_000_000 + 145_000_000 = 299_500_000
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "alice",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(145_000_000u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap();

        //expire the auction
        let mut env = mock_env();
        env.block.time = env.block.time.plus_seconds(200000);
        /*
           Withdraw NFT
        */

        //  Withdraw NFT to winner
        let msg = ExecuteMsg::WithdrawNft { auction_id: 2 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("bob", &vec![]),
            msg.clone(),
        )
        .unwrap();
        let prepare_msg = cw721::Cw721ExecuteMsg::TransferNft {
            recipient: "alice".to_string(),
            token_id: "test".to_string(),
        };
        let message_one = WasmMsg::Execute {
            contract_addr: "market".to_string(),
            msg: to_binary(&prepare_msg).unwrap(),
            funds: vec![],
        };
        let prepare_msg = cw20::Cw20ExecuteMsg::Mint {
            recipient: "sender".to_string(),
            amount: Uint128::from(169_000_000u128),
        };
        let message_two = WasmMsg::Execute {
            contract_addr: "cosmos2contract".to_string(),
            msg: to_binary(&prepare_msg).unwrap(),
            funds: vec![],
        };
        let prepare_msg = cw20::Cw20ExecuteMsg::Mint {
            recipient: "alice".to_string(),
            amount: Uint128::from(169_000_000u128),
        };
        let message_three = WasmMsg::Execute {
            contract_addr: "cosmos2contract".to_string(),
            msg: to_binary(&prepare_msg).unwrap(),
            funds: vec![],
        };
        let message_four = CosmosMsg::Bank(BankMsg::Send {
            to_address: "sender".to_string(),
            amount: vec![Coin {
                denom: "uusd".to_string(),
                amount: Uint128::from(1_490_425_000u128),
            }],
        });
        let message_five = CosmosMsg::Bank(BankMsg::Send {
            to_address: "terrans".to_string(),
            amount: vec![Coin {
                denom: "uusd".to_string(),
                amount: Uint128::from(1_68_000_000u128),
            }],
        });
        let message_six = CosmosMsg::Bank(BankMsg::Send {
            to_address: "loterra".to_string(),
            amount: vec![Coin {
                denom: "uusd".to_string(),
                amount: Uint128::from(29_282_178u128),
            }],
        });

        let all_msg = vec![
            SubMsg::new(message_one),
            SubMsg::new(message_two),
            SubMsg::new(message_three),
            SubMsg::new(message_four),
            SubMsg::new(message_five),
            SubMsg::new(message_six),
        ];
        assert_eq!(res.messages, all_msg);

        println!("{:?}", res);
        // Create auction with end_time
        let mut env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            Some(CharityResponse {
                address: "charity".to_string(),
                fee_percentage: Decimal::from_str("0.025").unwrap(),
            }),
            None,
            None,
            false,
        )
        .unwrap();
        let res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();
        /*
           Place bid
        */
        let execute_msg = ExecuteMsg::PlaceBid { auction_id: 3 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "alice",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(100u128),
                }],
            ),
            execute_msg,
        )
        .unwrap();
        //expire the auction
        env.block.time = env.block.time.plus_seconds(2000);
        /*
           Withdraw NFT
        */

        //  Withdraw NFT to winner
        let msg = ExecuteMsg::WithdrawNft { auction_id: 3 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("alice", &vec![]),
            msg.clone(),
        )
        .unwrap();
    }
    #[test]
    fn creator_withdraw_nft() {
        /*
           Withdraw nft creator no bidders
        */
        let mut deps = mock_dependencies_custom(&coins(2, "token"));
        init_default(deps.as_mut());

        let mut env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            None,
            None,
            false,
        )
        .unwrap();
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("market", &vec![]),
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
        let prepare_msg = cw721::Cw721ExecuteMsg::TransferNft {
            recipient: "sender".to_string(),
            token_id: "test".to_string(),
        };
        let message = WasmMsg::Execute {
            contract_addr: "market".to_string(),
            msg: to_binary(&prepare_msg).unwrap(),
            funds: vec![],
        };

        let all_msg = vec![SubMsg::new(message)];
        assert_eq!(res.messages, all_msg);
        println!("{:?}", res);
    }
    #[test]
    fn update_royalty() {
        /*
           Withdraw nft creator no bidders
        */
        let mut deps = mock_dependencies_custom(&coins(2, "token"));
        init_default(deps.as_mut());

        let mut env = mock_env();
        let execute_msg = ExecuteMsg::UpdateRoyalty {
            fee: Decimal::from_str("0.12").unwrap(),
            recipient: None,
        };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("creator_1", &vec![]),
            execute_msg,
        )
        .unwrap_err();

        let execute_msg = ExecuteMsg::UpdateRoyalty {
            fee: Decimal::from_str("0.1").unwrap(),
            recipient: None,
        };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("creator_1", &vec![]),
            execute_msg,
        )
        .unwrap();

        let execute_msg = ExecuteMsg::UpdateRoyalty {
            fee: Decimal::from_str("0.05").unwrap(),
            recipient: None,
        };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("creator_1", &vec![]),
            execute_msg,
        )
        .unwrap();

        println!("{:?}", res);
    }
    #[test]
    fn instant_buy() {
        /*
           Instant buy creator no bidders
        */
        let mut deps = mock_dependencies_custom(&coins(2, "token"));
        init_default(deps.as_mut());

        let mut env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            Some(Uint128::from(1_000u128)),
            None,
            false,
        )
        .unwrap();
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

        // assert_eq!(
        //     res.messages,
        //     vec![
        //         SubMsg {
        //             id: 0,
        //             msg: CosmosMsg::Wasm(WasmMsg::Execute {
        //                 contract_addr: "market".to_string(),
        //                 msg: to_binary(&withdraw_msg).unwrap(),
        //                 funds: vec![]
        //             }),
        //             gas_limit: None,
        //             reply_on: ReplyOn::Never
        //         },
        //         SubMsg {
        //             id: 1,
        //             msg: CosmosMsg::Wasm(WasmMsg::Execute {
        //                 contract_addr: "market".to_string(),
        //                 msg: to_binary(&withdraw_msg).unwrap(),
        //                 funds: vec![]
        //             }),
        //             gas_limit: None,
        //             reply_on: ReplyOn::Never
        //         },
        //         SubMsg {
        //             id: 2,
        //             msg: CosmosMsg::Wasm(WasmMsg::Execute {
        //                 contract_addr: "market".to_string(),
        //                 msg: to_binary(&withdraw_msg).unwrap(),
        //                 funds: vec![]
        //             }),
        //             gas_limit: None,
        //             reply_on: ReplyOn::Never
        //         },
        //         SubMsg {
        //             id: 3,
        //             msg: CosmosMsg::Wasm(WasmMsg::Execute {
        //                 contract_addr: "market".to_string(),
        //                 msg: to_binary(&withdraw_msg).unwrap(),
        //                 funds: vec![]
        //             }),
        //             gas_limit: None,
        //             reply_on: ReplyOn::Never
        //         },
        //         SubMsg {
        //             id: 4,
        //             msg: CosmosMsg::Wasm(WasmMsg::Execute {
        //                 contract_addr: "market".to_string(),
        //                 msg: to_binary(&withdraw_msg).unwrap(),
        //                 funds: vec![]
        //             }),
        //             gas_limit: None,
        //             reply_on: ReplyOn::Never
        //         }
        //     ]
        // );
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

    #[test]
    fn cancel_auction() {
        let mut deps = mock_dependencies_custom(&coins(2, "token"));
        init_default(deps.as_mut());
        let mut config = CONFIG.load(deps.as_ref().storage).unwrap();
        let mut cancellation = CANCELLATION.load(deps.as_ref().storage).unwrap();
        cancellation.cancellation_fee = Decimal::from_str("0.1").unwrap();
        CANCELLATION.save(deps.as_mut().storage, &cancellation);

        // Create auction with end_time inferior current time
        let env = mock_env();
        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            None,
            None,
            false,
        )
        .unwrap();
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();
        let execute_msg = ExecuteMsg::CancelAuction { auction_id: 0 };

        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("not_sender", &vec![]),
            execute_msg.clone(),
        )
        .unwrap_err();

        let execute_msg = ExecuteMsg::CancelAuction { auction_id: 0 };

        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("sender", &vec![]),
            execute_msg.clone(),
        )
        .unwrap();
        //println!("{:?}", res);
        assert_eq!(res.messages.len(), 0);
        assert_eq!(
            res.attributes,
            vec![
                Attribute {
                    key: "action".to_string(),
                    value: "cancel_auction".to_string()
                },
                Attribute {
                    key: "auction_id".to_string(),
                    value: "0".to_string()
                },
                Attribute {
                    key: "cancellation_fee".to_string(),
                    value: "0".to_string()
                },
            ]
        );
        // Handle cancelling multiple times
        let err = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("sender", &vec![]),
            execute_msg,
        )
        .unwrap_err();
        assert_eq!(err, ContractError::EndTimeExpired {});

        // Withdraw the NFT
        let execute_msg = ExecuteMsg::WithdrawNft { auction_id: 0 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("sender", &vec![]),
            execute_msg,
        )
        .unwrap();
        let cw721_msg = cw721::Cw721ExecuteMsg::TransferNft {
            recipient: "sender".to_string(),
            token_id: "test".to_string(),
        };
        let cosmwasm_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: "market".to_string(),
            msg: to_binary(&cw721_msg).unwrap(),
            funds: vec![],
        });
        assert_eq!(res.messages, vec![SubMsg::new(cosmwasm_msg)]);

        let execute_msg = create_msg_nft(
            None,
            None,
            env.block.time.plus_seconds(1000).seconds(),
            None,
            None,
            None,
            false,
        )
        .unwrap();
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("market", &vec![]),
            execute_msg,
        )
        .unwrap();

        let execute_msg = ExecuteMsg::PlaceBid { auction_id: 1 };
        let bid_amount = Uint128::from(100_000_000u128);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "alice",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: bid_amount,
                }],
            ),
            execute_msg,
        )
        .unwrap();

        let execute_msg = ExecuteMsg::CancelAuction { auction_id: 1 };
        let err = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "sender",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::from(1_000_000u128),
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap_err();
        let cancellation_fee = bid_amount.mul(cancellation.cancellation_fee);
        assert_eq!(
            err,
            ContractError::CancelAuctionFee(cancellation_fee.to_string(), "uusd".to_string())
        );

        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info(
                "sender",
                &vec![Coin {
                    denom: "uusd".to_string(),
                    amount: cancellation_fee,
                }],
            ),
            execute_msg.clone(),
        )
        .unwrap();
        // Split the amount between the highest bidder and fee contract collector here LoTerra staking contract
        let bank_msg_one = CosmosMsg::Bank(BankMsg::Send {
            to_address: "alice".to_string(),
            amount: vec![Coin {
                denom: "uusd".to_string(),
                amount: Uint128::from(4_950_495u128),
            }],
        });
        let bank_msg_two = CosmosMsg::Bank(BankMsg::Send {
            to_address: "loterra".to_string(),
            amount: vec![Coin {
                denom: "uusd".to_string(),
                amount: Uint128::from(4_950_495u128),
            }],
        });

        assert_eq!(res.messages.len(), 2);
        assert_eq!(
            res.messages,
            vec![SubMsg::new(bank_msg_one), SubMsg::new(bank_msg_two)]
        );
        assert_eq!(
            res.attributes,
            vec![
                Attribute {
                    key: "action".to_string(),
                    value: "cancel_auction".to_string()
                },
                Attribute {
                    key: "auction_id".to_string(),
                    value: "1".to_string()
                },
                Attribute {
                    key: "cancellation_fee".to_string(),
                    value: cancellation_fee.to_string()
                },
            ]
        );
        // ALICE retract bid success since auction was cancelled
        let execute_msg = ExecuteMsg::RetractBids { auction_id: 1 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("alice", &vec![]),
            execute_msg,
        )
        .unwrap();
        let bank_msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: "alice".to_string(),
            amount: vec![Coin {
                denom: "uusd".to_string(),
                amount: Uint128::from(99_009_900u128),
            }],
        });
        let mint_sity_msg = cw20::Cw20ExecuteMsg::Mint {
            recipient: "alice".to_string(),
            amount: Uint128::from(1_000_000u128),
        };
        let wasm_msg = WasmMsg::Execute {
            contract_addr: "cosmos2contract".to_string(),
            msg: to_binary(&mint_sity_msg).unwrap(),
            funds: vec![],
        };
        assert_eq!(res.messages.len(), 2);
        assert_eq!(
            res.messages,
            vec![SubMsg::new(bank_msg), SubMsg::new(wasm_msg)]
        );
        assert_eq!(
            res.attributes,
            vec![
                Attribute {
                    key: "auction_id".to_string(),
                    value: "1".to_string()
                },
                Attribute {
                    key: "refund_amount".to_string(),
                    value: "100000000".to_string()
                },
                Attribute {
                    key: "recipient".to_string(),
                    value: "alice".to_string()
                }
            ]
        );
        // Withdraw the NFT sender get back his NFT
        let execute_msg = ExecuteMsg::WithdrawNft { auction_id: 1 };
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("sender", &vec![]),
            execute_msg,
        )
        .unwrap();
        assert_eq!(res.messages.len(), 1);
        let cw721_msg = cw721::Cw721ExecuteMsg::TransferNft {
            recipient: "sender".to_string(),
            token_id: "test".to_string(),
        };
        let cosmwasm_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: "market".to_string(),
            msg: to_binary(&cw721_msg).unwrap(),
            funds: vec![],
        });
        assert_eq!(res.messages, vec![SubMsg::new(cosmwasm_msg)]);
        assert_eq!(
            res.attributes,
            vec![
                Attribute {
                    key: "auction_type".to_string(),
                    value: "NFT".to_string()
                },
                Attribute {
                    key: "auction_id".to_string(),
                    value: "1".to_string()
                },
                Attribute {
                    key: "sender".to_string(),
                    value: "sender".to_string()
                },
                Attribute {
                    key: "creator".to_string(),
                    value: "sender".to_string()
                },
                Attribute {
                    key: "recipient".to_string(),
                    value: "sender".to_string()
                },
            ]
        );
        println!("{:?}", res);
    }
}
