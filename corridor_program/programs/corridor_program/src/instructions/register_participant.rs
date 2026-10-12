use anchor_lang::prelude::*;

use crate::{
    constants::*,
    error::CorridorError,
    events::ParticipantRegistered,
    state::{Config, Participant, Role, Status},
};

#[event_cpi]
#[derive(Accounts)]
#[instruction(wallet: Pubkey)]
pub struct RegisterParticipant<'info> {
    #[account(mut)]
    pub compliance: Signer<'info>,
    #[account(
        seeds = [CONFIG_SEED],
        bump = config.bump,
        constraint = config.compliance_authority == compliance.key() @ CorridorError::Unauthorized
    )]
    pub config: Account<'info, Config>,
    #[account(
        init,
        payer = compliance,
        space = 8 + Participant::INIT_SPACE,
        seeds = [PARTICIPANT_SEED, wallet.as_ref()],
        bump
    )]
    pub participant: Account<'info, Participant>,
    pub system_program: Program<'info, System>,
}

/// KYB happens off-chain; the compliance multisig attests to it by signing
/// (F8). `quote_signer` applies to makers only and defaults to the wallet.
pub fn handle_register_participant(
    ctx: Context<RegisterParticipant>,
    wallet: Pubkey,
    role: Role,
    kyb_ref_hash: [u8; 32],
    max_trade_usdc: u64,
    daily_limit_usdc: u64,
    quote_signer: Option<Pubkey>,
) -> Result<()> {
    let quote_signer = match role {
        Role::Maker => quote_signer.unwrap_or(wallet),
        _ => {
            require!(quote_signer.is_none(), CorridorError::WrongRole);
            Pubkey::default()
        }
    };

    let now = Clock::get()?.unix_timestamp;
    ctx.accounts.participant.set_inner(Participant {
        wallet,
        role,
        kyb_ref_hash,
        quote_signer,
        max_trade_usdc,
        daily_limit_usdc,
        day_index: now / SECONDS_PER_DAY,
        day_volume_usdc: 0,
        status: Status::Active,
        bump: ctx.bumps.participant,
    });
    ctx.accounts.participant.validate_limits()?;

    emit_cpi!(ParticipantRegistered {
        wallet,
        role,
        kyb_ref_hash,
    });
    Ok(())
}
