// LiteSVM's FailedTransactionMetadata is large; the helpers return it as-is.
#![allow(clippy::result_large_err)]

//! Property tests for the pure logic settle_quote depends on: fee split,
//! daily counters, Ed25519 instruction parsing and quote encoding.

mod common;

use {
    anchor_lang::prelude::Pubkey,
    common::market::ed25519_ix,
    corridor_program::{
        ed25519::parse_single_signature,
        quote::{quote_message, split_fee, QUOTE_MESSAGE_LEN},
        state::{Participant, Role, Status},
        BPS_DENOMINATOR, MAX_FEE_BPS,
    },
    proptest::prelude::*,
    solana_keypair::Keypair,
    solana_signer::Signer,
};

fn participant(daily_limit_usdc: u64) -> Participant {
    Participant {
        wallet: Pubkey::new_unique(),
        role: Role::Taker,
        kyb_ref_hash: [0; 32],
        quote_signer: Pubkey::default(),
        max_trade_usdc: u64::MAX,
        daily_limit_usdc,
        day_index: 0,
        day_volume_usdc: 0,
        status: Status::Active,
        bump: 0,
    }
}

proptest! {
    // ---- Fee split (C17) ----

    #[test]
    fn fee_split_conserves_and_rounds_down(amount_out in any::<u64>(), fee_bps in 0..=BPS_DENOMINATOR as u16) {
        let (net, fee) = split_fee(amount_out, fee_bps).unwrap();
        prop_assert_eq!(u128::from(net) + u128::from(fee), u128::from(amount_out));
        prop_assert_eq!(
            u128::from(fee),
            u128::from(amount_out) * u128::from(fee_bps) / u128::from(BPS_DENOMINATOR)
        );
    }

    #[test]
    fn higher_fee_never_pays_the_recipient_more(amount_out in any::<u64>(), a in 0..=MAX_FEE_BPS, b in 0..=MAX_FEE_BPS) {
        let (low, high) = (a.min(b), a.max(b));
        let (net_low, _) = split_fee(amount_out, low).unwrap();
        let (net_high, _) = split_fee(amount_out, high).unwrap();
        prop_assert!(net_high <= net_low);
    }

    // ---- Daily counters (C19) ----

    /// Replays random trades over a few UTC days against a reference model.
    /// A rejected trade reverts its transaction, so state is restored on Err.
    #[test]
    fn daily_counter_matches_reference_model(
        limit in 0u64..1_000_000,
        trades in prop::collection::vec((0i64..3, 0u64..400_000), 0..40),
    ) {
        let mut account = participant(limit);
        let (mut model_day, mut model_volume) = (0i64, 0u64);
        for (days_forward, amount) in trades {
            let today = model_day + days_forward;
            if today != model_day {
                model_day = today;
                model_volume = 0;
            }
            let expected_ok = model_volume + amount <= limit;

            let before = account.clone();
            let ok = account.add_daily_volume(today, amount).is_ok();
            if !ok {
                account = before;
            }

            prop_assert_eq!(ok, expected_ok);
            if ok {
                model_volume += amount;
                prop_assert_eq!(account.day_index, today);
            }
            // A rejected trade reverts its rollover too, so the stored counter
            // may still belong to an earlier day; it counts as zero for today
            // because the next accepted trade resets it first.
            let effective_today = if account.day_index == model_day { account.day_volume_usdc } else { 0 };
            prop_assert_eq!(effective_today, model_volume);
            prop_assert!(account.day_volume_usdc <= limit);
        }
    }

    #[test]
    fn daily_counter_errors_instead_of_wrapping(headroom in 0u64..1_000, amount in any::<u64>()) {
        let mut account = participant(u64::MAX);
        account.day_volume_usdc = u64::MAX - headroom;
        let result = account.add_daily_volume(0, amount);
        prop_assert_eq!(result.is_ok(), amount <= headroom);
    }

    // ---- Ed25519 parsing (C14) ----

    #[test]
    fn ed25519_parser_never_panics(data in prop::collection::vec(any::<u8>(), 0..400)) {
        let _ = parse_single_signature(&data);
    }

    #[test]
    fn ed25519_parser_reads_back_what_the_client_signed(message in prop::collection::vec(any::<u8>(), 0..300)) {
        let signer = Keypair::new();
        let ix = ed25519_ix(&signer, &message);
        let (pubkey, parsed) = parse_single_signature(&ix.data).unwrap();
        let expected_pubkey = signer.pubkey();
        prop_assert_eq!(pubkey, expected_pubkey.as_ref());
        prop_assert_eq!(parsed, message.as_slice());
    }

    /// F10a: any instruction index other than u16::MAX, in any of the three
    /// index fields, is rejected.
    #[test]
    fn ed25519_parser_rejects_foreign_instruction_index(field in 0usize..3, index in 0u16..u16::MAX) {
        let mut ix = ed25519_ix(&Keypair::new(), b"quote");
        let at = [4, 8, 14][field]; // signature, pubkey, message instruction index
        ix.data[at..at + 2].copy_from_slice(&index.to_le_bytes());
        prop_assert!(parse_single_signature(&ix.data).is_err());
    }

    #[test]
    fn ed25519_parser_rejects_signature_counts_other_than_one(count in any::<u8>()) {
        prop_assume!(count != 1);
        let mut ix = ed25519_ix(&Keypair::new(), b"quote");
        ix.data[0] = count;
        prop_assert!(parse_single_signature(&ix.data).is_err());
    }

    // ---- Quote encoding (C15) ----

    /// Fixed width means the encoding is injective: two quotes that differ in
    /// any field never share signed bytes.
    #[test]
    fn quote_message_is_fixed_width_and_injective(
        a in any::<(u64, u64, u64, i64, [u8; 32])>(),
        b in any::<(u64, u64, u64, i64, [u8; 32])>(),
    ) {
        let keys = [Pubkey::new_unique(), Pubkey::new_unique(), Pubkey::new_unique(), Pubkey::new_unique()];
        let encode = |(quote_id, amount_in, amount_out, expires_at, genesis): (u64, u64, u64, i64, [u8; 32])| {
            quote_message(&genesis, quote_id, &keys[0], &keys[1], &keys[2], &keys[3], amount_in, amount_out, expires_at)
        };
        let (message_a, message_b) = (encode(a), encode(b));
        prop_assert_eq!(message_a.len(), QUOTE_MESSAGE_LEN);
        prop_assert_eq!(message_a == message_b, a == b);
    }
}

#[test]
fn quote_message_is_241_bytes() {
    assert_eq!(QUOTE_MESSAGE_LEN, 241);
}
