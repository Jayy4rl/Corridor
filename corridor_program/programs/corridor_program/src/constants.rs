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

#[constant]
pub const DELEGATE_SEED: &[u8] = b"delegate";

// Protocol limits
#[constant]
pub const MAX_FEE_BPS: u16 = 50;

#[constant]
pub const BPS_DENOMINATOR: u64 = 10_000;

#[constant]
pub const MAX_QUOTE_TTL: i64 = 120;

#[constant]
pub const SECONDS_PER_DAY: i64 = 86_400;

#[constant]
pub const DOMAIN_TAG: &[u8] = b"CORRIDOR_QUOTE_V1";
