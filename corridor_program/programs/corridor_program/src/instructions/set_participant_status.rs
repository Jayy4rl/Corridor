use anchor_lang::prelude::*;

use crate::{
    auth::require_authority_or_guardian_stop,
    constants::*,
    events::ParticipantStatusChanged,
    state::{Config, Participant, Status},
};

#[event_cpi]
#[derive(Accounts)]
pub struct SetParticipantStatus<'info> {
    pub authority: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub participant: Account<'info, Participant>,
}

/// Revoking a suspended maker's SPL allowance is client-side (F11): the maker
/// tooling does it, the program only blocks the participant in settle_quote.
pub fn handle_set_participant_status(
    ctx: Context<SetParticipantStatus>,
    status: Status,
) -> Result<()> {
    let config = &ctx.accounts.config;
    let authority = ctx.accounts.authority.key();
    require_authority_or_guardian_stop(
        authority,
        config.compliance_authority,
        config.guardian,
        status == Status::Suspended,
    )?;

    let participant = &mut ctx.accounts.participant;
    participant.status = status;

    emit_cpi!(ParticipantStatusChanged {
        wallet: participant.wallet,
        status,
        by: authority,
    });
    Ok(())
}
