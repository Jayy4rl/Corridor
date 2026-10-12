use anchor_lang::prelude::*;

use crate::{constants::*, error::CorridorError, events::AdminAccepted, state::Config};

#[event_cpi]
#[derive(Accounts)]
pub struct AcceptAdmin<'info> {
    pub new_admin: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
}

/// Step 2 of the two-step admin transfer: only the proposed key can complete
/// it, so a mistyped address can never take control (F6).
pub fn handle_accept_admin(ctx: Context<AcceptAdmin>) -> Result<()> {
    let config = &mut ctx.accounts.config;
    let pending_admin = config.pending_admin.ok_or(CorridorError::NoPendingAdmin)?;
    require_keys_eq!(
        pending_admin,
        ctx.accounts.new_admin.key(),
        CorridorError::Unauthorized
    );

    let old_admin = config.admin;
    config.admin = pending_admin;
    config.pending_admin = None;

    emit_cpi!(AdminAccepted {
        old_admin,
        new_admin: pending_admin,
    });
    Ok(())
}
