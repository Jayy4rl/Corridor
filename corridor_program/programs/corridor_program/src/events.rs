use anchor_lang::prelude::*;

use crate::state::{Role, Status};

// All events are emitted with emit_cpi! (F4) so they land in instruction data,
// where the Corridor indexer reads them, rather than in truncatable logs.

#[event]
pub struct ConfigInitialized {
    pub admin: Pubkey,
    pub compliance_authority: Pubkey,
    pub guardian: Pubkey,
    pub usdc_mint: Pubkey,
    pub fee_bps: u16,
    pub treasury_owner: Pubkey,
    pub receipt_retention_secs: u64,
    pub genesis_hash: [u8; 32],
}

#[event]
pub struct ParamsUpdated {
    pub old_fee_bps: u16,
    pub new_fee_bps: u16,
    pub old_treasury_owner: Pubkey,
    pub new_treasury_owner: Pubkey,
    pub old_receipt_retention_secs: u64,
    pub new_receipt_retention_secs: u64,
}

#[event]
pub struct OperationalAuthoritiesUpdated {
    pub old_compliance_authority: Pubkey,
    pub new_compliance_authority: Pubkey,
    pub old_guardian: Pubkey,
    pub new_guardian: Pubkey,
}

#[event]
pub struct AdminProposed {
    pub admin: Pubkey,
    pub pending_admin: Option<Pubkey>,
}

#[event]
pub struct AdminAccepted {
    pub old_admin: Pubkey,
    pub new_admin: Pubkey,
}

#[event]
pub struct PauseChanged {
    pub paused: bool,
    pub by: Pubkey,
}

#[event]
pub struct IssuerRegistered {
    pub mint: Pubkey,
    pub token_program: Pubkey,
    pub decimals: u8,
    pub attestation_hash: [u8; 32],
    pub attestation_expires_at: i64,
    pub max_trade_amount: u64,
    pub daily_cap: u64,
}

#[event]
pub struct AttestationUpdated {
    pub mint: Pubkey,
    pub old_attestation_hash: [u8; 32],
    pub new_attestation_hash: [u8; 32],
    pub old_expires_at: i64,
    pub new_expires_at: i64,
}

#[event]
pub struct IssuerCapsUpdated {
    pub mint: Pubkey,
    pub old_max_trade_amount: u64,
    pub new_max_trade_amount: u64,
    pub old_daily_cap: u64,
    pub new_daily_cap: u64,
}

#[event]
pub struct IssuerStatusChanged {
    pub mint: Pubkey,
    pub status: Status,
    pub by: Pubkey,
}

#[event]
pub struct ParticipantRegistered {
    pub wallet: Pubkey,
    pub role: Role,
    pub kyb_ref_hash: [u8; 32],
}

#[event]
pub struct ParticipantUpdated {
    pub wallet: Pubkey,
    pub kyb_ref_hash: [u8; 32],
    pub old_max_trade_usdc: u64,
    pub new_max_trade_usdc: u64,
    pub old_daily_limit_usdc: u64,
    pub new_daily_limit_usdc: u64,
}

#[event]
pub struct ParticipantStatusChanged {
    pub wallet: Pubkey,
    pub status: Status,
    pub by: Pubkey,
}

#[event]
pub struct QuoteSignerRotated {
    pub maker: Pubkey,
    pub old_quote_signer: Pubkey,
    pub new_quote_signer: Pubkey,
}

#[event]
pub struct QuoteSettled {
    pub quote_id: u64,
    pub maker: Pubkey,
    pub taker: Pubkey,
    pub recipient: Pubkey,
    pub mint_in: Pubkey,
    pub mint_out: Pubkey,
    pub amount_in: u64,
    pub amount_out: u64,
    pub fee: u64,
    pub compliance_hash: [u8; 32],
    pub settled_at: i64,
}

#[event]
pub struct ReceiptClosed {
    pub receipt: Pubkey,
    pub rent_payer: Pubkey,
    pub closed_by: Pubkey,
}
