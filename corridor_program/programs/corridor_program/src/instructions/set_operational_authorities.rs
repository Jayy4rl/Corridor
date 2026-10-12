use anchor_lang::prelude::*;

use crate::{
    constants::*, error::CorridorError, events::OperationalAuthoritiesUpdated, state::Config,
};

#[event_cpi]
#[derive(Accounts)]
pub struct SetOperationalAuthorities<'info> {
    pub admin: Signer<'info>,
    #[account(
        mut,
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = admin @ CorridorError::Unauthorized
    )]
    pub config: Account<'info, Config>,
}

pub fn handle_set_operational_authorities(
    ctx: Context<SetOperationalAuthorities>,
    compliance_authority: Option<Pubkey>,
    guardian: Option<Pubkey>,
) -> Result<()> {
    let config = &mut ctx.accounts.config;
    let (old_compliance_authority, old_guardian) = (config.compliance_authority, config.guardian);

    if let Some(compliance_authority) = compliance_authority {
        config.compliance_authority = compliance_authority;
    }
    if let Some(guardian) = guardian {
        config.guardian = guardian;
    }

    emit_cpi!(OperationalAuthoritiesUpdated {
        old_compliance_authority,
        new_compliance_authority: config.compliance_authority,
        old_guardian,
        new_guardian: config.guardian,
    });
    Ok(())
}
