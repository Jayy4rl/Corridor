use anchor_lang::prelude::*;

use crate::{
    constants::*,
    error::CorridorError,
    events::ReceiptClosed,
    state::{Config, QuoteReceipt},
};

#[event_cpi]
#[derive(Accounts)]
pub struct CloseReceipt<'info> {
    /// Anyone may close: normally Corridor's crank, but the taker or any other
    /// wallet works too, so a dead crank delays refunds but loses nothing (F7).
    pub caller: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(
        mut,
        close = rent_payer,
        has_one = rent_payer @ CorridorError::RentPayerMismatch
    )]
    pub quote_receipt: Account<'info, QuoteReceipt>,
    /// CHECK: only receives the rent refund; bound to the receipt by has_one.
    #[account(mut)]
    pub rent_payer: UncheckedAccount<'info>,
}

/// C22. Closing is only allowed once the quote has expired (retention is
/// unsigned), so a resubmitted quote fails QuoteExpired instead of finding
/// its receipt gone. Works while paused.
pub fn handle_close_receipt(ctx: Context<CloseReceipt>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let retention = i64::try_from(ctx.accounts.config.receipt_retention_secs)
        .map_err(|_| CorridorError::MathOverflow)?;
    let closable_after = ctx
        .accounts
        .quote_receipt
        .expires_at
        .checked_add(retention)
        .ok_or(CorridorError::MathOverflow)?;
    require!(now > closable_after, CorridorError::RetentionNotElapsed);

    emit_cpi!(ReceiptClosed {
        receipt: ctx.accounts.quote_receipt.key(),
        rent_payer: ctx.accounts.rent_payer.key(),
        closed_by: ctx.accounts.caller.key(),
    });
    Ok(())
}
