use anchor_lang::prelude::*;

use crate::constants::DOMAIN_TAG;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    LocalToUsdc,
    UsdcToLocal,
}

pub const QUOTE_MESSAGE_LEN: usize = DOMAIN_TAG.len() + 32 * 2 + 8 + 32 * 4 + 8 * 3;

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

pub fn split_fee(amount_out: u64, fee_bps: u16) -> Result<(u64, u64)> {
    let fee = u128::from(amount_out) * u128::from(fee_bps) / u128::from(crate::BPS_DENOMINATOR);
    let fee = u64::try_from(fee).map_err(|_| crate::error::CorridorError::MathOverflow)?;
    let net = amount_out
        .checked_sub(fee)
        .ok_or(crate::error::CorridorError::MathOverflow)?;
    Ok((net, fee))
}
