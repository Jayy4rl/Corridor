use anchor_lang::prelude::*;

use crate::{
    auth::require_authority_or_guardian_stop,
    constants::*,
    events::IssuerStatusChanged,
    state::{Config, IssuerEntry, Status},
};

#[event_cpi]
#[derive(Accounts)]
pub struct SetIssuerStatus<'info> {
    pub authority: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub issuer_entry: Account<'info, IssuerEntry>,
}

pub fn handle_set_issuer_status(ctx: Context<SetIssuerStatus>, status: Status) -> Result<()> {
    let config = &ctx.accounts.config;
    let authority = ctx.accounts.authority.key();
    require_authority_or_guardian_stop(
        authority,
        config.admin,
        config.guardian,
        status == Status::Suspended,
    )?;

    let issuer = &mut ctx.accounts.issuer_entry;
    issuer.status = status;

    emit_cpi!(IssuerStatusChanged {
        mint: issuer.mint,
        status,
        by: authority,
    });
    Ok(())
}
