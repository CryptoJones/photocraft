//! Optional, app-owned Sign in with ChatGPT. No document or UI types cross this seam.
//! Default and web builds neither read credentials nor contact an external service.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use photocraft_raster::Interrupt;
use serde::{Deserialize, Serialize};

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
mod native;
#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub use native::NativeAccounts;

pub const MANAGE_USAGE: &str = "https://chatgpt.com/settings/usage";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cancelled")]
    Cancelled,
    #[error("ChatGPT sign-in: {0}")]
    Auth(String),
    #[error("private account storage: {0}")]
    Storage(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub(crate) fn check(ctl: &Interrupt<'_>) -> Result<()> {
    ctl.check().map_err(|_| Error::Cancelled)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub label: String,
    pub email: Option<String>,
    pub connected: bool,
    pub plan_usage: bool,
    pub welcomed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub active: Option<String>,
    pub accounts: Vec<Account>,
    pub sign_in_available: bool,
}

/// Public state excludes tokens, authorization URLs and PKCE secrets. Native workers own
/// account operations; they never mutate documents or reuse another app's credentials.
pub trait Accounts: Send + Sync {
    fn status(&self) -> AccountStatus;
    fn sign_in(&self, account: Option<&str>, ctl: &Interrupt<'_>) -> Result<AccountStatus>;
    fn select(&self, account: &str, ctl: &Interrupt<'_>) -> Result<AccountStatus>;
    fn refresh(&self, ctl: &Interrupt<'_>) -> Result<AccountStatus>;
    /// False means local sign-out succeeded but remote token revocation was not confirmed.
    fn sign_out(&self, ctl: &Interrupt<'_>) -> Result<bool>;
    fn acknowledge_welcome(&self, account: &str) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_status_has_no_account_or_credentials() {
        let status = AccountStatus::default();
        assert!(!status.sign_in_available);
        assert!(status.active.is_none() && status.accounts.is_empty());
        assert_eq!(serde_json::to_value(status).unwrap(), serde_json::json!({"active":null,"accounts":[],"signInAvailable":false}));
    }
}
