//! PlayCraft Error Handling Module
//! Author: Rashmina Fernando (https://www.rashmina.dev)
//! Copyright (c) 2026 Rashmina Fernando (www.rashmina.dev). All Rights Reserved.

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlayCraftError {
    #[error("I/O operation failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Game not found in library: {0}")]
    GameNotFound(String),

    #[error("Executable launch failure: {0}")]
    LaunchFailed(String),

    #[error("Scanner error: {0}")]
    ScannerError(String),

    #[error("Optimizer error: {0}")]
    OptimizerError(String),

    #[error("Mobile controller server error: {0}")]
    MobileServerError(String),

    #[error("Internal error: {0}")]
    Other(#[from] anyhow::Error),
}

impl Serialize for PlayCraftError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type PlayCraftResult<T> = Result<T, PlayCraftError>;
