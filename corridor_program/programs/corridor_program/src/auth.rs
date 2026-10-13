use anchor_lang::prelude::*;

use crate::error::CorridorError;

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

pub fn require_not_default(key: &Pubkey) -> Result<()> {
    require_keys_neq!(*key, Pubkey::default(), CorridorError::DefaultPubkey);
    Ok(())
}
