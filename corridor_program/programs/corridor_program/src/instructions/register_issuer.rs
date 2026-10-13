use anchor_lang::prelude::*;
use anchor_spl::{
    token_2022::spl_token_2022::{
        self,
        extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensions},
    },
    token_interface::Mint,
};

use crate::{
    constants::*,
    error::CorridorError,
    events::IssuerRegistered,
    state::{Config, IssuerEntry, Status},
};

pub const UNSUPPORTED_MINT_EXTENSIONS: [ExtensionType; 4] = [
    ExtensionType::TransferFeeConfig,
    ExtensionType::TransferHook,
    ExtensionType::PermanentDelegate,
    ExtensionType::MintCloseAuthority,
];

#[event_cpi]
#[derive(Accounts)]
pub struct RegisterIssuer<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = admin @ CorridorError::Unauthorized
    )]
    pub config: Account<'info, Config>,
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(
        init,
        payer = admin,
        space = 8 + IssuerEntry::INIT_SPACE,
        seeds = [ISSUER_SEED, mint.key().as_ref()],
        bump
    )]
    pub issuer_entry: Account<'info, IssuerEntry>,
    pub system_program: Program<'info, System>,
}

pub fn handle_register_issuer(
    ctx: Context<RegisterIssuer>,
    attestation_hash: [u8; 32],
    attestation_expires_at: i64,
    max_trade_amount: u64,
    daily_cap: u64,
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    require!(attestation_expires_at > now, CorridorError::AttestationExpired);

    let mint_info = ctx.accounts.mint.to_account_info();
    let token_program = *mint_info.owner;
    if token_program == spl_token_2022::ID {
        let data = mint_info.try_borrow_data()?;
        let mint = StateWithExtensions::<spl_token_2022::state::Mint>::unpack(&data)?;
        for extension in mint.get_extension_types()? {
            require!(
                !UNSUPPORTED_MINT_EXTENSIONS.contains(&extension),
                CorridorError::UnsupportedMintExtension
            );
        }
    }

    let mint = ctx.accounts.mint.key();
    let decimals = ctx.accounts.mint.decimals;
    ctx.accounts.issuer_entry.set_inner(IssuerEntry {
        mint,
        token_program,
        decimals,
        attestation_hash,
        attestation_expires_at,
        max_trade_amount,
        daily_cap,
        day_index: now / SECONDS_PER_DAY,
        day_volume: 0,
        status: Status::Active,
        bump: ctx.bumps.issuer_entry,
    });

    emit_cpi!(IssuerRegistered {
        mint,
        token_program,
        decimals,
        attestation_hash,
        attestation_expires_at,
        max_trade_amount,
        daily_cap,
    });
    Ok(())
}
