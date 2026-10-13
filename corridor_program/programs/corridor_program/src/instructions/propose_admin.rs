use anchor_lang::prelude::*;

use crate::{
    auth::require_not_default, constants::*, error::CorridorError, events::AdminProposed,
    state::Config,
};

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

pub fn handle_propose_admin(ctx: Context<ProposeAdmin>, new_admin: Option<Pubkey>) -> Result<()> {
    if let Some(new_admin) = &new_admin {
        require_not_default(new_admin)?;
    }
    ctx.accounts.config.pending_admin = new_admin;

    emit_cpi!(AdminProposed {
        admin: ctx.accounts.admin.key(),
        pending_admin: new_admin,
    });
    Ok(())
}
