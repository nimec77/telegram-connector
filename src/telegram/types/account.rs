//! Successful account observations, independently timestamped by RPC.
use crate::UtcTimestamp;

/// Last observed account state; absent timestamps mean not yet observed.
/// A transport failure never changes authorization to false.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AccountStatus {
    pub authorized: bool,
    pub premium: Option<bool>,
    pub authorization_checked_at: Option<UtcTimestamp>,
    pub premium_checked_at: Option<UtcTimestamp>,
}
