use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug, InitSpace)]
pub enum Status {
    Active,
    Suspended,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug, InitSpace)]
pub enum Role {
    Taker,
    Maker,
    PayoutPartner,
}

/// Seeds: ["config"]. One per program.
#[account]
#[derive(InitSpace)]
pub struct Config {
    pub admin: Pubkey,
    pub compliance_authority: Pubkey,
    pub guardian: Pubkey,
    /// Immutable after initialisation (C4).
    pub usdc_mint: Pubkey,
    pub fee_bps: u16,
    pub treasury_owner: Pubkey,
    /// Unsigned on purpose: a negative retention would let close_receipt run
    /// before the quote expires, re-opening it to replay (C22).
    pub receipt_retention_secs: u64,
    pub paused: bool,
    /// Two-step admin transfer (F1, F6).
    pub pending_admin: Option<Pubkey>,
    /// Cluster genesis hash, bound into every quote message (F10b). Stored here
    /// because a program cannot read the genesis hash at runtime.
    pub genesis_hash: [u8; 32],
    pub bump: u8,
}

/// Seeds: ["issuer", mint].
#[account]
#[derive(InitSpace)]
pub struct IssuerEntry {
    pub mint: Pubkey,
    pub token_program: Pubkey,
    pub decimals: u8,
    pub attestation_hash: [u8; 32],
    pub attestation_expires_at: i64,
    pub max_trade_amount: u64,
    pub daily_cap: u64,
    /// unix_timestamp / SECONDS_PER_DAY when day_volume was last reset.
    pub day_index: i64,
    /// Not tracked on the USDC entry (F12).
    pub day_volume: u64,
    pub status: Status,
    pub bump: u8,
}

/// Seeds: ["participant", wallet].
#[account]
#[derive(InitSpace)]
pub struct Participant {
    pub wallet: Pubkey,
    pub role: Role,
    pub kyb_ref_hash: [u8; 32],
    /// Makers only; Pubkey::default() for other roles.
    pub quote_signer: Pubkey,
    pub max_trade_usdc: u64,
    pub daily_limit_usdc: u64,
    pub day_index: i64,
    pub day_volume_usdc: u64,
    pub status: Status,
    pub bump: u8,
}

/// Seeds: ["receipt", maker, quote_id.to_le_bytes()]. Replay protection only;
/// trade details live in the QuoteSettled event (F4).
#[account]
#[derive(InitSpace)]
pub struct QuoteReceipt {
    pub expires_at: i64,
    pub rent_payer: Pubkey,
    pub bump: u8,
}

impl Participant {
    /// Takers and makers trade, so both limits must be non-zero (UC-07, UC-08).
    /// Payout partners only receive and are never limit-checked.
    pub fn validate_limits(&self) -> Result<()> {
        if self.role != Role::PayoutPartner {
            require!(
                self.max_trade_usdc > 0 && self.daily_limit_usdc > 0,
                crate::error::CorridorError::InvalidLimits
            );
        }
        Ok(())
    }
}

impl Participant {
    /// C19 for a taker or maker, in USDC base units.
    pub fn add_daily_volume(&mut self, today: i64, usdc_amount: u64) -> Result<()> {
        add_daily_volume(
            &mut self.day_index,
            &mut self.day_volume_usdc,
            today,
            usdc_amount,
            self.daily_limit_usdc,
        )
    }
}

impl IssuerEntry {
    /// C19 for the non-USDC issuer, in its own base units.
    pub fn add_daily_volume(&mut self, today: i64, amount: u64) -> Result<()> {
        add_daily_volume(&mut self.day_index, &mut self.day_volume, today, amount, self.daily_cap)
    }
}

/// C19: zero the counter when the UTC day has changed, then add `amount`,
/// failing if the day's total would exceed `limit`.
fn add_daily_volume(
    day_index: &mut i64,
    day_volume: &mut u64,
    today: i64,
    amount: u64,
    limit: u64,
) -> Result<()> {
    if *day_index != today {
        *day_index = today;
        *day_volume = 0;
    }
    let total = day_volume
        .checked_add(amount)
        .ok_or(crate::error::CorridorError::MathOverflow)?;
    require!(total <= limit, crate::error::CorridorError::DailyLimitExceeded);
    *day_volume = total;
    Ok(())
}
