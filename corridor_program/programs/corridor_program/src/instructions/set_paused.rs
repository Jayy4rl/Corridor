use anchor_lang::prelude::*;

use crate::{
    auth::require_authority_or_guardian_stop, constants::*, events::PauseChanged, state::Config,
};

#[event_cpi]
#[derive(Accounts)]
pub struct SetPaused<'info> {
    pub authority: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
}

pub fn handle_set_paused(ctx: Context<SetPaused>, paused: bool) -> Result<()> {
    let config = &mut ctx.accounts.config;
    let authority = ctx.accounts.authority.key();

    require_authority_or_guardian_stop(authority, config.admin, config.guardian, paused)?;

    config.paused = paused;

    emit_cpi!(PauseChanged {
        paused,
        by: authority,
    });
    Ok(())
}
