use anchor_lang::prelude::*;

use crate::{
    error::CorridorError,
    events::QuoteSignerRotated,
    state::{Participant, Role},
};

#[event_cpi]
#[derive(Accounts)]
pub struct SetQuoteSigner<'info> {
    pub maker: Signer<'info>,
    /// Binding by stored wallet rather than by seeds gives the spec's
    /// Unauthorized error; both are equally strict. Status is deliberately not
    /// checked: a maker suspended over a leaked quote key must be able to
    /// rotate it before reactivation, or the leaked key goes live again.
    #[account(
        mut,
        constraint = maker_participant.wallet == maker.key() @ CorridorError::Unauthorized,
        constraint = maker_participant.role == Role::Maker @ CorridorError::WrongRole
    )]
    pub maker_participant: Account<'info, Participant>,
}

/// Rotating the key invalidates every outstanding quote signed by the old one,
/// because settle_quote verifies against the current quote_signer.
pub fn handle_set_quote_signer(ctx: Context<SetQuoteSigner>, new_quote_signer: Pubkey) -> Result<()> {
    let participant = &mut ctx.accounts.maker_participant;
    let old_quote_signer = participant.quote_signer;
    require_keys_neq!(old_quote_signer, new_quote_signer, CorridorError::SameSigner);

    participant.quote_signer = new_quote_signer;

    emit_cpi!(QuoteSignerRotated {
        maker: participant.wallet,
        old_quote_signer,
        new_quote_signer,
    });
    Ok(())
}
