use anchor_lang::prelude::*;

// PDA seeds
#[constant]
pub const CONFIG_SEED: &[u8] = b"config";

#[constant]
pub const ISSUER_SEED: &[u8] = b"issuer";

#[constant]
pub const PARTICIPANT_SEED: &[u8] = b"participant";

#[constant]
pub const RECEIPT_SEED: &[u8] = b"receipt";

/// Per-maker signer-only PDA: ["delegate", maker] (F9).
#[constant]
pub const DELEGATE_SEED: &[u8] = b"delegate";

// Protocol limits
#[constant]
pub const MAX_FEE_BPS: u16 = 50;

#[constant]
pub const BPS_DENOMINATOR: u64 = 10_000;

/// Longest allowed gap between settlement time and quote expiry (F10d).
#[constant]
pub const MAX_QUOTE_TTL: i64 = 120;

/// Daily counters roll over on the UTC day: unix_timestamp / SECONDS_PER_DAY.
#[constant]
pub const SECONDS_PER_DAY: i64 = 86_400;

/// Prefix of every signed quote message (F10b).
#[constant]
pub const DOMAIN_TAG: &[u8] = b"CORRIDOR_QUOTE_V1";
