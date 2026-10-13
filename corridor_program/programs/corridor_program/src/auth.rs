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

/// Rejects Pubkey::default() wherever a key grants a role or receives funds:
/// nobody holds its private key, so setting it locks the role out for good.
pub fn require_not_default(key: &Pubkey) -> Result<()> {
    require_keys_neq!(*key, Pubkey::default(), CorridorError::DefaultPubkey);
    Ok(())
}
