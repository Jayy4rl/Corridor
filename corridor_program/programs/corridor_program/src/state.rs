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

#[account]
#[derive(InitSpace)]
pub struct Config {
    pub admin: Pubkey,
    pub compliance_authority: Pubkey,
    pub guardian: Pubkey,
    pub usdc_mint: Pubkey,
    pub fee_bps: u16,
    pub treasury_owner: Pubkey,
    pub receipt_retention_secs: u64,
    pub paused: bool,
    pub pending_admin: Option<Pubkey>,
    pub genesis_hash: [u8; 32],
    pub bump: u8,
}

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
    pub day_index: i64,
    pub day_volume: u64,
    pub status: Status,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct Participant {
    pub wallet: Pubkey,
    pub role: Role,
    pub kyb_ref_hash: [u8; 32],
    pub quote_signer: Pubkey,
    pub max_trade_usdc: u64,
    pub daily_limit_usdc: u64,
    pub day_index: i64,
    pub day_volume_usdc: u64,
    pub status: Status,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct QuoteReceipt {
    pub expires_at: i64,
    pub rent_payer: Pubkey,
    pub bump: u8,
}

impl Participant {
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
    pub fn add_daily_volume(&mut self, today: i64, amount: u64) -> Result<()> {
        add_daily_volume(&mut self.day_index, &mut self.day_volume, today, amount, self.daily_cap)
    }
}

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
