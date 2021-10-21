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

    #[error("Inaccurate funds for instant buying")]
    InaccurateFunds {},

    #[error("Min bid amount is {0}")]
    MinBid (Uint128),

    #[error("Registration amount required {0} PRIV token")]
    PrivateSaleRestriction (Uint128),
    // Add any other custom errors you like here.
    // Look at https://docs.rs/thiserror/1.0.21/thiserror/ for details.
}
