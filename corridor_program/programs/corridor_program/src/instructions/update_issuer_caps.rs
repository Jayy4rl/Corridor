use anchor_lang::prelude::*;

use crate::{
    constants::*,
    error::CorridorError,
    events::IssuerCapsUpdated,
    state::{Config, IssuerEntry},
};

#[event_cpi]
#[derive(Accounts)]
pub struct UpdateIssuerCaps<'info> {
    pub admin: Signer<'info>,
    #[account(
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = admin @ CorridorError::Unauthorized
    )]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub issuer_entry: Account<'info, IssuerEntry>,
}

pub fn handle_update_issuer_caps(
    ctx: Context<UpdateIssuerCaps>,
    max_trade_amount: Option<u64>,
    daily_cap: Option<u64>,
) -> Result<()> {
    let issuer = &mut ctx.accounts.issuer_entry;
    let (old_max_trade_amount, old_daily_cap) = (issuer.max_trade_amount, issuer.daily_cap);

    if let Some(max_trade_amount) = max_trade_amount {
        issuer.max_trade_amount = max_trade_amount;
    }
    if let Some(daily_cap) = daily_cap {
        issuer.daily_cap = daily_cap;
    }

    emit_cpi!(IssuerCapsUpdated {
        mint: issuer.mint,
        old_max_trade_amount,
        new_max_trade_amount: issuer.max_trade_amount,
        old_daily_cap,
        new_daily_cap: issuer.daily_cap,
    });
    Ok(())
}
