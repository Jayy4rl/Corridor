use anchor_lang::prelude::*;

// v2 error list (spec p.22), with v1 aliases folded in:
// GuardianCannotUnpause / GuardianCannotReactivate -> GuardianCannotResume,
// AttestationAlreadyExpired -> AttestationExpired. `Paused` is added because
// C7 needs an error and the v2 list omits it.
#[error_code]
pub enum CorridorError {
    #[msg("Signer is not authorised for this instruction")]
    Unauthorized,
    #[msg("Guardian may only pause or suspend, never resume")]
    GuardianCannotResume,
    #[msg("Fee exceeds MAX_FEE_BPS")]
    FeeTooHigh,
    #[msg("Protocol is paused")]
    Paused,
    #[msg("Mint uses a Token-2022 extension that v1 does not support")]
    UnsupportedMintExtension,
    #[msg("Issuer attestation has expired")]
    AttestationExpired,
    #[msg("Issuer is suspended")]
    IssuerSuspended,
    #[msg("Participant is suspended")]
    ParticipantSuspended,
    #[msg("Participant has the wrong role for this action")]
    WrongRole,
    #[msg("Recipient must be the taker or an active payout partner")]
    RecipientNotAllowed,
    #[msg("Exactly one leg must be USDC")]
    PairMustIncludeUsdc,
    #[msg("Token account owner does not match")]
    AccountOwnerMismatch,
    #[msg("Token account or issuer mint does not match")]
    MintMismatch,
    #[msg("Maker delegate allowance is missing or too small")]
    InsufficientAllowance,
    #[msg("Previous instruction is not an Ed25519 verification")]
    MissingEd25519Instruction,
    #[msg("Ed25519 instruction offsets are not self-referencing")]
    InvalidEd25519Offsets,
    #[msg("Quote signature does not match the maker's quote signer or quote terms")]
    InvalidQuoteSignature,
    #[msg("Quote has expired")]
    QuoteExpired,
    #[msg("Quote expiry exceeds MAX_QUOTE_TTL")]
    QuoteTtlTooLong,
    #[msg("Amount out after fee is below min_amount_out")]
    SlippageExceeded,
    #[msg("Trade exceeds a per-trade limit")]
    TradeTooLarge,
    #[msg("Trade exceeds a daily limit")]
    DailyLimitExceeded,
    #[msg("Arithmetic overflow")]
    MathOverflow,
    #[msg("Receipt retention period has not elapsed")]
    RetentionNotElapsed,
    #[msg("Rent payer does not match the receipt")]
    RentPayerMismatch,
    #[msg("No pending admin to accept")]
    NoPendingAdmin,
    #[msg("New quote signer equals the current one")]
    SameSigner,
    #[msg("Limits are invalid for this role")]
    InvalidLimits,
}
