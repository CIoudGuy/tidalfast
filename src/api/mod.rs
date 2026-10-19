//! The TIDAL catalogue client (`tidal`) and what the backend shares with it.

use std::fmt;

pub mod bio;
pub mod client;
pub mod gateway;
pub mod models;
pub mod tidal;
pub mod tidal_catalog;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ApiSource {
    Shared,
    Personal,
}

impl fmt::Display for ApiSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shared => formatter.write_str("shared"),
            Self::Personal => formatter.write_str("personal"),
        }
    }
}

pub use client::{ApiError, NetActivity, PlayRequest};
pub use gateway::{AccountId, ApiGateway, SessionState};
pub use tidal::TidalClient;
pub use tidal_catalog::{Catalog, Quality, StreamInfo, TidalCatalog};
