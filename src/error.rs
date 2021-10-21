use cosmwasm_std::StdError;
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
    // Add any other custom errors you like here.
    // Look at https://docs.rs/thiserror/1.0.21/thiserror/ for details.
}
