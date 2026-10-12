use anchor_lang::prelude::*;

use crate::{
    constants::*,
    error::CorridorError,
    events::AttestationUpdated,
    state::{Config, IssuerEntry},
};

#[event_cpi]
#[derive(Accounts)]
pub struct UpdateIssuerAttestation<'info> {
    pub compliance: Signer<'info>,
    #[account(
        seeds = [CONFIG_SEED],
        bump = config.bump,
        constraint = config.compliance_authority == compliance.key() @ CorridorError::Unauthorized
    )]
    pub config: Account<'info, Config>,
    /// Program ownership and the account discriminator already prove this is a
    /// genuine IssuerEntry; no mint is passed here, so there is nothing to bind.
    #[account(mut)]
    pub issuer_entry: Account<'info, IssuerEntry>,
}

pub fn handle_update_issuer_attestation(
    ctx: Context<UpdateIssuerAttestation>,
    attestation_hash: [u8; 32],
    attestation_expires_at: i64,
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    require!(attestation_expires_at > now, CorridorError::AttestationExpired);

    let issuer = &mut ctx.accounts.issuer_entry;
    let (old_attestation_hash, old_expires_at) =
        (issuer.attestation_hash, issuer.attestation_expires_at);
    issuer.attestation_hash = attestation_hash;
    issuer.attestation_expires_at = attestation_expires_at;

    emit_cpi!(AttestationUpdated {
        mint: issuer.mint,
        old_attestation_hash,
        new_attestation_hash: attestation_hash,
        old_expires_at,
        new_expires_at: attestation_expires_at,
    });
    Ok(())
}
