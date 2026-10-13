use anchor_lang::prelude::*;
use anchor_lang::solana_program::bpf_loader_upgradeable;

use crate::{
    auth::require_not_default, constants::*, error::CorridorError, events::ConfigInitialized,
    state::Config,
};

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct InitializeConfigArgs {
    pub admin: Pubkey,
    pub compliance_authority: Pubkey,
    pub guardian: Pubkey,
    pub usdc_mint: Pubkey,
    pub fee_bps: u16,
    pub treasury_owner: Pubkey,
    pub receipt_retention_secs: u64,
    pub genesis_hash: [u8; 32],
}

#[event_cpi]
#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    #[account(mut)]
    pub deployer: Signer<'info>,
    #[account(
        init,
        payer = deployer,
        space = 8 + Config::INIT_SPACE,
        seeds = [CONFIG_SEED],
        bump
    )]
    pub config: Account<'info, Config>,
    /// C1: the loader-derived ProgramData of *this* program, so only the
    /// upgrade authority can initialise (prevents front-running).
    #[account(
        seeds = [crate::ID.as_ref()],
        bump,
        seeds::program = bpf_loader_upgradeable::ID,
        constraint = program_data.upgrade_authority_address == Some(deployer.key())
            @ CorridorError::Unauthorized
    )]
    pub program_data: Account<'info, ProgramData>,
    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_config(
    ctx: Context<InitializeConfig>,
    args: InitializeConfigArgs,
) -> Result<()> {
    require!(args.fee_bps <= MAX_FEE_BPS, CorridorError::FeeTooHigh);
    for key in [
        &args.admin,
        &args.compliance_authority,
        &args.guardian,
        &args.usdc_mint,
        &args.treasury_owner,
    ] {
        require_not_default(key)?;
    }

    ctx.accounts.config.set_inner(Config {
        admin: args.admin,
        compliance_authority: args.compliance_authority,
        guardian: args.guardian,
        usdc_mint: args.usdc_mint,
        fee_bps: args.fee_bps,
        treasury_owner: args.treasury_owner,
        receipt_retention_secs: args.receipt_retention_secs,
        paused: false,
        pending_admin: None,
        genesis_hash: args.genesis_hash,
        bump: ctx.bumps.config,
    });

    emit_cpi!(ConfigInitialized {
        admin: args.admin,
        compliance_authority: args.compliance_authority,
        guardian: args.guardian,
        usdc_mint: args.usdc_mint,
        fee_bps: args.fee_bps,
        treasury_owner: args.treasury_owner,
        receipt_retention_secs: args.receipt_retention_secs,
        genesis_hash: args.genesis_hash,
    });
    Ok(())
}
