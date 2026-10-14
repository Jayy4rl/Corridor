use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::{
    constants::*,
    ed25519::verify_quote_signature,
    error::CorridorError,
    events::QuoteSettled,
    quote::{quote_message, split_fee, Direction},
    state::{Config, IssuerEntry, Participant, QuoteReceipt, Role, Status},
};

// Accounts are named by role, not by trade direction: `usdc_*` and `local_*`
// stay fixed whichever way the taker trades, so the USDC IssuerEntry can be
// read-only in every settlement (F12) and C13 holds by construction. Token
// accounts are named by flow (in = taker pays, out = maker pays) and checked
// against the direction in the handler. Large accounts are boxed to keep
// try_accounts within the 4 KB stack frame.
#[event_cpi]
#[derive(Accounts)]
#[instruction(quote_id: u64)]
pub struct SettleQuote<'info> {
    #[account(mut)]
    pub taker: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump, constraint = !config.paused @ CorridorError::Paused)]
    pub config: Box<Account<'info, Config>>,

    // C8
    #[account(
        mut,
        constraint = taker_participant.wallet == taker.key() @ CorridorError::Unauthorized,
        constraint = taker_participant.role == Role::Taker @ CorridorError::WrongRole,
        constraint = taker_participant.status == Status::Active @ CorridorError::ParticipantSuspended
    )]
    pub taker_participant: Box<Account<'info, Participant>>,
    #[account(
        mut,
        constraint = maker_participant.role == Role::Maker @ CorridorError::WrongRole,
        constraint = maker_participant.status == Status::Active @ CorridorError::ParticipantSuspended
    )]
    pub maker_participant: Box<Account<'info, Participant>>,
    pub recipient_participant: Box<Account<'info, Participant>>,

    // C13, C6
    #[account(address = config.usdc_mint @ CorridorError::MintMismatch)]
    pub usdc_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(constraint = local_mint.key() != config.usdc_mint @ CorridorError::PairMustIncludeUsdc)]
    pub local_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(seeds = [ISSUER_SEED, usdc_mint.key().as_ref()], bump = usdc_issuer.bump)]
    pub usdc_issuer: Box<Account<'info, IssuerEntry>>,
    #[account(mut, seeds = [ISSUER_SEED, local_mint.key().as_ref()], bump = local_issuer.bump)]
    pub local_issuer: Box<Account<'info, IssuerEntry>>,

    // C9-C12, checked against the direction in the handler
    #[account(mut)]
    pub taker_ata_in: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)]
    pub maker_ata_in: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)]
    pub maker_ata_out: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)]
    pub recipient_ata_out: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)]
    pub treasury_ata_out: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: signer-only PDA that holds no data. Per-maker seeds (F9) mean a
    /// maker's SPL approval can only be spent on that maker's own quotes.
    #[account(seeds = [DELEGATE_SEED, maker_participant.wallet.as_ref()], bump)]
    pub delegate_authority: UncheckedAccount<'info>,

    // C21: init, never init_if_needed, so a second settlement fails.
    #[account(
        init,
        payer = taker,
        space = 8 + QuoteReceipt::INIT_SPACE,
        seeds = [RECEIPT_SEED, maker_participant.wallet.as_ref(), &quote_id.to_le_bytes()],
        bump
    )]
    pub quote_receipt: Box<Account<'info, QuoteReceipt>>,

    /// CHECK: address-checked; read through the sysvar helpers.
    #[account(address = solana_instructions_sysvar::ID)]
    pub instructions_sysvar: UncheckedAccount<'info>,
    #[account(address = usdc_issuer.token_program @ CorridorError::MintMismatch)]
    pub usdc_token_program: Interface<'info, TokenInterface>,
    #[account(address = local_issuer.token_program @ CorridorError::MintMismatch)]
    pub local_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

fn require_token_account(account: &TokenAccount, owner: &Pubkey, mint: &Pubkey) -> Result<()> {
    require_keys_eq!(account.owner, *owner, CorridorError::AccountOwnerMismatch);
    require_keys_eq!(account.mint, *mint, CorridorError::MintMismatch);
    Ok(())
}

fn require_issuer_tradeable(issuer: &IssuerEntry, now: i64) -> Result<()> {
    require!(issuer.status == Status::Active, CorridorError::IssuerSuspended);
    require!(issuer.attestation_expires_at > now, CorridorError::AttestationExpired);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn handle_settle_quote(
    mut ctx: Context<SettleQuote>,
    quote_id: u64,
    direction: Direction,
    amount_in: u64,
    amount_out: u64,
    expires_at: i64,
    min_amount_out: u64,
    compliance_hash: [u8; 32],
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let accounts = &ctx.accounts;
    let taker = accounts.taker.key();
    let maker = accounts.maker_participant.wallet;
    let recipient = accounts.recipient_participant.wallet;

    // ---- Checks ----

    require!(amount_in > 0 && amount_out > 0, CorridorError::ZeroAmount);

    // C11
    if recipient != taker {
        let partner = &accounts.recipient_participant;
        require!(
            partner.role == Role::PayoutPartner && partner.status == Status::Active,
            CorridorError::RecipientNotAllowed
        );
    }

    // C13
    require_issuer_tradeable(&accounts.usdc_issuer, now)?;
    require_issuer_tradeable(&accounts.local_issuer, now)?;

    // C16
    require!(now < expires_at, CorridorError::QuoteExpired);
    let ttl = expires_at.checked_sub(now).ok_or(CorridorError::MathOverflow)?;
    require!(ttl <= MAX_QUOTE_TTL, CorridorError::QuoteTtlTooLong);

    let (mint_in, mint_out, token_program_in, token_program_out) = match direction {
        Direction::LocalToUsdc => (
            &accounts.local_mint,
            &accounts.usdc_mint,
            &accounts.local_token_program,
            &accounts.usdc_token_program,
        ),
        Direction::UsdcToLocal => (
            &accounts.usdc_mint,
            &accounts.local_mint,
            &accounts.usdc_token_program,
            &accounts.local_token_program,
        ),
    };
    let (mint_in_key, mint_out_key) = (mint_in.key(), mint_out.key());

    // C9, C11, C12
    require_token_account(&accounts.taker_ata_in, &taker, &mint_in_key)?;
    require_token_account(&accounts.maker_ata_in, &maker, &mint_in_key)?;
    require_token_account(&accounts.maker_ata_out, &maker, &mint_out_key)?;
    require_token_account(&accounts.recipient_ata_out, &recipient, &mint_out_key)?;
    require_token_account(&accounts.treasury_ata_out, &accounts.config.treasury_owner, &mint_out_key)?;

    // C10
    let delegate: Option<Pubkey> = accounts.maker_ata_out.delegate.into();
    require!(
        delegate == Some(accounts.delegate_authority.key())
            && accounts.maker_ata_out.delegated_amount >= amount_out,
        CorridorError::InsufficientAllowance
    );

    // C14, C15
    let message = quote_message(
        &accounts.config.genesis_hash,
        quote_id,
        &maker,
        &taker,
        &mint_in_key,
        &mint_out_key,
        amount_in,
        amount_out,
        expires_at,
    );
    verify_quote_signature(
        &accounts.instructions_sysvar,
        &accounts.maker_participant.quote_signer,
        &message,
    )?;

    // C17
    let (net_amount_out, fee) = split_fee(amount_out, accounts.config.fee_bps)?;
    require!(net_amount_out >= min_amount_out, CorridorError::SlippageExceeded);

    // C18
    let (usdc_amount, local_amount) = match direction {
        Direction::LocalToUsdc => (amount_out, amount_in),
        Direction::UsdcToLocal => (amount_in, amount_out),
    };
    require!(
        local_amount <= accounts.local_issuer.max_trade_amount
            && usdc_amount <= accounts.taker_participant.max_trade_usdc
            && usdc_amount <= accounts.maker_participant.max_trade_usdc,
        CorridorError::TradeTooLarge
    );

    // Owned AccountInfos for the CPIs, taken before the mutable borrow below.
    let (mint_in_info, mint_out_info) = (mint_in.to_account_info(), mint_out.to_account_info());
    let (decimals_in, decimals_out) = (mint_in.decimals, mint_out.decimals);
    let (token_program_in, token_program_out) = (token_program_in.key(), token_program_out.key());

    // ---- Effects ----

    // C19: daily limits in USDC for both participants, own units for the
    // local issuer. The USDC issuer has no counter (F12).
    let today = now / SECONDS_PER_DAY;
    let accounts = &mut ctx.accounts;
    accounts.taker_participant.add_daily_volume(today, usdc_amount)?;
    accounts.maker_participant.add_daily_volume(today, usdc_amount)?;
    accounts.local_issuer.add_daily_volume(today, local_amount)?;

    accounts.quote_receipt.set_inner(QuoteReceipt {
        expires_at,
        rent_payer: taker,
        bump: ctx.bumps.quote_receipt,
    });

    // ---- Interactions ----

    // Leg 1: taker pays maker, signed by the taker.
    transfer_checked(
        CpiContext::new(
            token_program_in,
            TransferChecked {
                from: accounts.taker_ata_in.to_account_info(),
                mint: mint_in_info,
                to: accounts.maker_ata_in.to_account_info(),
                authority: accounts.taker.to_account_info(),
            },
        ),
        amount_in,
        decimals_in,
    )?;

    // Legs 2-3: maker pays recipient and treasury, signed by the maker's
    // delegate PDA within the maker's SPL allowance.
    let delegate_seeds: &[&[&[u8]]] = &[&[
        DELEGATE_SEED,
        maker.as_ref(),
        &[ctx.bumps.delegate_authority],
    ]];
    transfer_checked(
        CpiContext::new_with_signer(
            token_program_out,
            TransferChecked {
                from: accounts.maker_ata_out.to_account_info(),
                mint: mint_out_info.clone(),
                to: accounts.recipient_ata_out.to_account_info(),
                authority: accounts.delegate_authority.to_account_info(),
            },
            delegate_seeds,
        ),
        net_amount_out,
        decimals_out,
    )?;
    if fee > 0 {
        transfer_checked(
            CpiContext::new_with_signer(
                token_program_out,
                TransferChecked {
                    from: accounts.maker_ata_out.to_account_info(),
                    mint: mint_out_info,
                    to: accounts.treasury_ata_out.to_account_info(),
                    authority: accounts.delegate_authority.to_account_info(),
                },
                delegate_seeds,
            ),
            fee,
            decimals_out,
        )?;
    }

    emit_cpi!(QuoteSettled {
        quote_id,
        maker,
        taker,
        recipient,
        mint_in: mint_in_key,
        mint_out: mint_out_key,
        amount_in,
        amount_out,
        fee,
        compliance_hash,
        settled_at: now,
    });
    Ok(())
}
