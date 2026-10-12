use anchor_lang::prelude::*;

use crate::{
    constants::*,
    error::CorridorError,
    events::ParticipantUpdated,
    state::{Config, Participant},
};

#[event_cpi]
#[derive(Accounts)]
pub struct UpdateParticipant<'info> {
    pub compliance: Signer<'info>,
    #[account(
        seeds = [CONFIG_SEED],
        bump = config.bump,
        constraint = config.compliance_authority == compliance.key() @ CorridorError::Unauthorized
    )]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub participant: Account<'info, Participant>,
}

/// Role is not changeable; today's day_volume_usdc is kept, so a lowered
/// daily limit takes effect against volume already traded today.
pub fn handle_update_participant(
    ctx: Context<UpdateParticipant>,
    kyb_ref_hash: Option<[u8; 32]>,
    max_trade_usdc: Option<u64>,
    daily_limit_usdc: Option<u64>,
) -> Result<()> {
    let participant = &mut ctx.accounts.participant;
    let (old_max_trade_usdc, old_daily_limit_usdc) =
        (participant.max_trade_usdc, participant.daily_limit_usdc);

    if let Some(kyb_ref_hash) = kyb_ref_hash {
        participant.kyb_ref_hash = kyb_ref_hash;
    }
    if let Some(max_trade_usdc) = max_trade_usdc {
        participant.max_trade_usdc = max_trade_usdc;
    }
    if let Some(daily_limit_usdc) = daily_limit_usdc {
        participant.daily_limit_usdc = daily_limit_usdc;
    }
    participant.validate_limits()?;

    emit_cpi!(ParticipantUpdated {
        wallet: participant.wallet,
        kyb_ref_hash: participant.kyb_ref_hash,
        old_max_trade_usdc,
        new_max_trade_usdc: participant.max_trade_usdc,
        old_daily_limit_usdc,
        new_daily_limit_usdc: participant.daily_limit_usdc,
    });
    Ok(())
}
