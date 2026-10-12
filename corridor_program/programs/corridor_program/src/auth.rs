use anchor_lang::prelude::*;

use crate::error::CorridorError;

/// C3: `authority` (admin or compliance) may move a switch either way; the
/// guardian may only move it towards "stopped". Checking `authority` first
/// means a key holding both roles keeps the stronger power.
pub fn require_authority_or_guardian_stop(
    signer: Pubkey,
    authority: Pubkey,
    guardian: Pubkey,
    is_stop: bool,
) -> Result<()> {
    if signer != authority {
        require_keys_eq!(signer, guardian, CorridorError::Unauthorized);
        require!(is_stop, CorridorError::GuardianCannotResume);
    }
    Ok(())
}
