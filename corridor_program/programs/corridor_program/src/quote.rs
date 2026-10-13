use anchor_lang::prelude::*;

use crate::constants::DOMAIN_TAG;

/// Which way the taker trades. Accounts are named by role (local vs USDC), so
/// this argument decides which mint is `mint_in` (taker pays) and `mint_out`
/// (maker pays) in the signed quote.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// Taker pays the local stablecoin and receives USDC.
    LocalToUsdc,
    /// Taker pays USDC and receives the local stablecoin.
    UsdcToLocal,
}

pub const QUOTE_MESSAGE_LEN: usize = DOMAIN_TAG.len() + 32 * 2 + 8 + 32 * 4 + 8 * 3;

/// The exact bytes a maker's quote key signs (C15). Encoding is fixed: raw
/// 32-byte keys and hashes, little-endian fixed-width integers, no length
/// prefixes. The off-chain RFQ service must produce identical bytes.
///
/// DOMAIN_TAG | genesis_hash | program_id | quote_id | maker | taker |
/// mint_in | mint_out | amount_in | amount_out | expires_at
#[allow(clippy::too_many_arguments)]
pub fn quote_message(
    genesis_hash: &[u8; 32],
    quote_id: u64,
    maker: &Pubkey,
    taker: &Pubkey,
    mint_in: &Pubkey,
    mint_out: &Pubkey,
    amount_in: u64,
    amount_out: u64,
    expires_at: i64,
) -> Vec<u8> {
    let mut message = Vec::with_capacity(QUOTE_MESSAGE_LEN);
    message.extend_from_slice(DOMAIN_TAG);
    message.extend_from_slice(genesis_hash);
    message.extend_from_slice(crate::ID.as_ref());
    message.extend_from_slice(&quote_id.to_le_bytes());
    message.extend_from_slice(maker.as_ref());
    message.extend_from_slice(taker.as_ref());
    message.extend_from_slice(mint_in.as_ref());
    message.extend_from_slice(mint_out.as_ref());
    message.extend_from_slice(&amount_in.to_le_bytes());
    message.extend_from_slice(&amount_out.to_le_bytes());
    message.extend_from_slice(&expires_at.to_le_bytes());
    message
}

/// C17: splits `amount_out` into (net to recipient, fee to treasury). The fee
/// rounds down, so it never exceeds amount_out and net + fee == amount_out.
pub fn split_fee(amount_out: u64, fee_bps: u16) -> Result<(u64, u64)> {
    let fee = u128::from(amount_out) * u128::from(fee_bps) / u128::from(crate::BPS_DENOMINATOR);
    let fee = u64::try_from(fee).map_err(|_| crate::error::CorridorError::MathOverflow)?;
    let net = amount_out
        .checked_sub(fee)
        .ok_or(crate::error::CorridorError::MathOverflow)?;
    Ok((net, fee))
}
