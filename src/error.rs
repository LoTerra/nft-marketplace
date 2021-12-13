use cosmwasm_std::{StdError, Uint128};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("End time already expired")]
    EndTimeExpired {},

    #[error("Percentage error")]
    PercentageFormat {},

    #[error("Cannot be zero")]
    ZeroNotValid {},

    #[error("Empty funds")]
    EmptyFunds {},

    #[error("Wrong denom")]
    WrongDenom {},

    #[error("Multiple denom not allowed")]
    MultipleDenoms {},

    #[error("Wait until auction start")]
    AuctionNotStarted {},

    #[error("Inaccurate funds for instant buying price is {0} your total bids is {1} ")]
    InaccurateFunds(Uint128, Uint128),

    #[error("Min bid amount is {0}, your total sent with this current amount is {1}")]
    MinBid(Uint128, Uint128),

    #[error("Registration amount required {0} PRIV token, please register first")]
    PrivateSaleRestriction(Uint128),

    #[error("Use instant buy price {0}, you are trying to bid higher {1}")]
    UseInstantBuy(Uint128, Uint128),

    #[error("Start price cannot be higher than {0}")]
    StartPriceHigherThan(String),
    #[error("Instant buy price cannot be lower than {0}")]
    InstantBuyPriceLowerThan(String),
    // Add any other custom errors you like here.
    // Look at https://docs.rs/thiserror/1.0.21/thiserror/ for details.
}
