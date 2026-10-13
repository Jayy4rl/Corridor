pub mod auth;
pub mod constants;
pub mod ed25519;
pub mod error;
pub mod events;
pub mod instructions;
pub mod quote;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use quote::Direction;
pub use state::*;

declare_id!("3RecGnphiwdGbpiKgvWi6JhSdtrVwtUHQcbTqBvbhiiM");

#[program]
pub mod corridor_program {
    use super::*;

    // Handlers are numbered as in the v2 instruction table.

    // 1
    pub fn initialize_config(
        ctx: Context<InitializeConfig>,
        args: InitializeConfigArgs,
    ) -> Result<()> {
        instructions::initialize_config::handle_initialize_config(ctx, args)
    }

    // 2
    pub fn update_params(
        ctx: Context<UpdateParams>,
        fee_bps: Option<u16>,
        treasury_owner: Option<Pubkey>,
        receipt_retention_secs: Option<u64>,
    ) -> Result<()> {
        instructions::update_params::handle_update_params(
            ctx,
            fee_bps,
            treasury_owner,
            receipt_retention_secs,
        )
    }

    // 3
    pub fn set_operational_authorities(
        ctx: Context<SetOperationalAuthorities>,
        compliance_authority: Option<Pubkey>,
        guardian: Option<Pubkey>,
    ) -> Result<()> {
        instructions::set_operational_authorities::handle_set_operational_authorities(
            ctx,
            compliance_authority,
            guardian,
        )
    }

    // 4
    pub fn propose_admin(ctx: Context<ProposeAdmin>, new_admin: Option<Pubkey>) -> Result<()> {
        instructions::propose_admin::handle_propose_admin(ctx, new_admin)
    }

    // 5
    pub fn accept_admin(ctx: Context<AcceptAdmin>) -> Result<()> {
        instructions::accept_admin::handle_accept_admin(ctx)
    }

    // 6
    pub fn set_paused(ctx: Context<SetPaused>, paused: bool) -> Result<()> {
        instructions::set_paused::handle_set_paused(ctx, paused)
    }

    // 7
    pub fn register_issuer(
        ctx: Context<RegisterIssuer>,
        attestation_hash: [u8; 32],
        attestation_expires_at: i64,
        max_trade_amount: u64,
        daily_cap: u64,
    ) -> Result<()> {
        instructions::register_issuer::handle_register_issuer(
            ctx,
            attestation_hash,
            attestation_expires_at,
            max_trade_amount,
            daily_cap,
        )
    }

    // 8
    pub fn update_issuer_attestation(
        ctx: Context<UpdateIssuerAttestation>,
        attestation_hash: [u8; 32],
        attestation_expires_at: i64,
    ) -> Result<()> {
        instructions::update_issuer_attestation::handle_update_issuer_attestation(
            ctx,
            attestation_hash,
            attestation_expires_at,
        )
    }

    // 9
    pub fn update_issuer_caps(
        ctx: Context<UpdateIssuerCaps>,
        max_trade_amount: Option<u64>,
        daily_cap: Option<u64>,
    ) -> Result<()> {
        instructions::update_issuer_caps::handle_update_issuer_caps(ctx, max_trade_amount, daily_cap)
    }

    // 10
    pub fn set_issuer_status(ctx: Context<SetIssuerStatus>, status: Status) -> Result<()> {
        instructions::set_issuer_status::handle_set_issuer_status(ctx, status)
    }

    // 11
    pub fn register_participant(
        ctx: Context<RegisterParticipant>,
        wallet: Pubkey,
        role: Role,
        kyb_ref_hash: [u8; 32],
        max_trade_usdc: u64,
        daily_limit_usdc: u64,
        quote_signer: Option<Pubkey>,
    ) -> Result<()> {
        instructions::register_participant::handle_register_participant(
            ctx,
            wallet,
            role,
            kyb_ref_hash,
            max_trade_usdc,
            daily_limit_usdc,
            quote_signer,
        )
    }

    // 12
    pub fn update_participant(
        ctx: Context<UpdateParticipant>,
        kyb_ref_hash: Option<[u8; 32]>,
        max_trade_usdc: Option<u64>,
        daily_limit_usdc: Option<u64>,
    ) -> Result<()> {
        instructions::update_participant::handle_update_participant(
            ctx,
            kyb_ref_hash,
            max_trade_usdc,
            daily_limit_usdc,
        )
    }

    // 13
    pub fn set_participant_status(ctx: Context<SetParticipantStatus>, status: Status) -> Result<()> {
        instructions::set_participant_status::handle_set_participant_status(ctx, status)
    }

    // 14
    pub fn set_quote_signer(ctx: Context<SetQuoteSigner>, new_quote_signer: Pubkey) -> Result<()> {
        instructions::set_quote_signer::handle_set_quote_signer(ctx, new_quote_signer)
    }

    // 15
    #[allow(clippy::too_many_arguments)]
    pub fn settle_quote(
        ctx: Context<SettleQuote>,
        quote_id: u64,
        direction: Direction,
        amount_in: u64,
        amount_out: u64,
        expires_at: i64,
        min_amount_out: u64,
        compliance_hash: [u8; 32],
    ) -> Result<()> {
        instructions::settle_quote::handle_settle_quote(
            ctx,
            quote_id,
            direction,
            amount_in,
            amount_out,
            expires_at,
            min_amount_out,
            compliance_hash,
        )
    }

    // 16
    pub fn close_receipt(ctx: Context<CloseReceipt>) -> Result<()> {
        instructions::close_receipt::handle_close_receipt(ctx)
    }
}
