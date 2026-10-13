use anchor_lang::prelude::*;
use solana_instructions_sysvar::{load_current_index_checked, load_instruction_at_checked};

use crate::error::CorridorError;

/// Ed25519 precompile data: u8 count | u8 padding | 14-byte offsets per
/// signature (sig offset, sig ix, pubkey offset, pubkey ix, msg offset,
/// msg size, msg ix — all u16 LE) | payload.
const HEADER_LEN: usize = 2;
const OFFSETS_LEN: usize = 14;
const PUBKEY_LEN: usize = 32;
/// Offsets point into the Ed25519 instruction's own data.
const THIS_INSTRUCTION: u16 = u16::MAX;

/// C14-C15: the instruction immediately before this one must be an Ed25519
/// verification of exactly one signature, by `expected_signer`, over
/// `expected_message`, with every offset pointing into its own data.
///
/// The runtime has already checked the signature itself (a failed precompile
/// fails the transaction); this proves it was *this* key over *this* message.
pub fn verify_quote_signature(
    instructions_sysvar: &AccountInfo,
    expected_signer: &Pubkey,
    expected_message: &[u8],
) -> Result<()> {
    let current = load_current_index_checked(instructions_sysvar)?;
    require!(current > 0, CorridorError::MissingEd25519Instruction);
    let ix = load_instruction_at_checked(usize::from(current - 1), instructions_sysvar)?;
    require_keys_eq!(
        ix.program_id,
        solana_sdk_ids::ed25519_program::ID,
        CorridorError::MissingEd25519Instruction
    );

    let (pubkey, message) = parse_single_signature(&ix.data)?;
    require!(
        pubkey == expected_signer.as_ref() && message == expected_message,
        CorridorError::InvalidQuoteSignature
    );
    Ok(())
}

/// Returns the (pubkey, message) an Ed25519 instruction verified, if it holds
/// exactly one signature whose offsets all point into its own data. Pure, so
/// it can be fuzzed: any input must yield Ok or InvalidEd25519Offsets.
pub fn parse_single_signature(data: &[u8]) -> Result<(&[u8], &[u8])> {
    require!(
        data.len() >= HEADER_LEN + OFFSETS_LEN && data[0] == 1,
        CorridorError::InvalidEd25519Offsets
    );
    let read_u16 = |at: usize| u16::from_le_bytes([data[at], data[at + 1]]);
    let offsets = HEADER_LEN;
    let signature_ix = read_u16(offsets + 2);
    let pubkey_offset = usize::from(read_u16(offsets + 4));
    let pubkey_ix = read_u16(offsets + 6);
    let message_offset = usize::from(read_u16(offsets + 8));
    let message_len = usize::from(read_u16(offsets + 10));
    let message_ix = read_u16(offsets + 12);
    require!(
        signature_ix == THIS_INSTRUCTION
            && pubkey_ix == THIS_INSTRUCTION
            && message_ix == THIS_INSTRUCTION,
        CorridorError::InvalidEd25519Offsets
    );

    let pubkey = data
        .get(pubkey_offset..pubkey_offset + PUBKEY_LEN)
        .ok_or(CorridorError::InvalidEd25519Offsets)?;
    let message = data
        .get(message_offset..message_offset + message_len)
        .ok_or(CorridorError::InvalidEd25519Offsets)?;
    Ok((pubkey, message))
}
