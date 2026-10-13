use anchor_lang::prelude::*;

use crate::{
    auth::require_not_default, constants::*, error::CorridorError, events::ParamsUpdated,
    state::Config,
};

#[event_cpi]
#[derive(Accounts)]
pub struct UpdateParams<'info> {
    pub admin: Signer<'info>,
    #[account(
        mut,
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = admin @ CorridorError::Unauthorized
    )]
    pub config: Account<'info, Config>,
}

pub fn handle_update_params(
    ctx: Context<UpdateParams>,
    fee_bps: Option<u16>,
    treasury_owner: Option<Pubkey>,
    receipt_retention_secs: Option<u64>,
) -> Result<()> {
    let config = &mut ctx.accounts.config;
    let (old_fee_bps, old_treasury_owner, old_retention) =
        (config.fee_bps, config.treasury_owner, config.receipt_retention_secs);

    if let Some(fee_bps) = fee_bps {
        require!(fee_bps <= MAX_FEE_BPS, CorridorError::FeeTooHigh);
        config.fee_bps = fee_bps;
    }
    if let Some(treasury_owner) = treasury_owner {
        require_not_default(&treasury_owner)?;
        config.treasury_owner = treasury_owner;
    }
    if let Some(retention) = receipt_retention_secs {
        config.receipt_retention_secs = retention;
    }

    emit_cpi!(ParamsUpdated {
        old_fee_bps,
        new_fee_bps: config.fee_bps,
        old_treasury_owner,
        new_treasury_owner: config.treasury_owner,
        old_receipt_retention_secs: old_retention,
        new_receipt_retention_secs: config.receipt_retention_secs,
    });
    Ok(())
}
