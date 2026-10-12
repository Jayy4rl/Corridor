use anchor_lang::prelude::*;

use crate::{constants::*, error::CorridorError, events::AdminProposed, state::Config};

#[event_cpi]
#[derive(Accounts)]
pub struct ProposeAdmin<'info> {
    pub admin: Signer<'info>,
    #[account(
        mut,
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = admin @ CorridorError::Unauthorized
    )]
    pub config: Account<'info, Config>,
}

/// Step 1 of the two-step admin transfer (F1, F6). Passing None cancels a
/// pending proposal; a new proposal replaces the old one.
pub fn handle_propose_admin(ctx: Context<ProposeAdmin>, new_admin: Option<Pubkey>) -> Result<()> {
    ctx.accounts.config.pending_admin = new_admin;

    emit_cpi!(AdminProposed {
        admin: ctx.accounts.admin.key(),
        pending_admin: new_admin,
    });
    Ok(())
}
