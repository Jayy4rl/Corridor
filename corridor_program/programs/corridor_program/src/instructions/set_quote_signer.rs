use anchor_lang::prelude::*;

use crate::{
    auth::require_not_default,
    error::CorridorError,
    events::QuoteSignerRotated,
    state::{Participant, Role},
};

#[event_cpi]
#[derive(Accounts)]
pub struct SetQuoteSigner<'info> {
    pub maker: Signer<'info>,
   #[account(
        mut,
        constraint = maker_participant.wallet == maker.key() @ CorridorError::Unauthorized,
        constraint = maker_participant.role == Role::Maker @ CorridorError::WrongRole
    )]
    pub maker_participant: Account<'info, Participant>,
}

pub fn handle_set_quote_signer(ctx: Context<SetQuoteSigner>, new_quote_signer: Pubkey) -> Result<()> {
    let participant = &mut ctx.accounts.maker_participant;
    let old_quote_signer = participant.quote_signer;
    require_not_default(&new_quote_signer)?;
    require_keys_neq!(old_quote_signer, new_quote_signer, CorridorError::SameSigner);

    participant.quote_signer = new_quote_signer;

    emit_cpi!(QuoteSignerRotated {
        maker: participant.wallet,
        old_quote_signer,
        new_quote_signer,
    });
    Ok(())
}
