// LiteSVM's FailedTransactionMetadata is large; the helpers return it as-is.
#![allow(clippy::result_large_err)]

//! End-to-end fuzzing of settle_quote: random direction, amounts, fee and
//! slippage bound against a fresh market. Every trade must either settle
//! exactly as a reference model predicts or fail with nothing moved.

mod common;

use {
    common::{market::*, *},
    corridor_program::{quote::Direction, state::Participant},
    proptest::prelude::*,
    solana_signer::Signer,
};

const MAX_AMOUNT: u64 = 3 * MAX_TRADE_USDC;

fn snapshot(m: &Market) -> Vec<u64> {
    let owners = [m.taker.pubkey(), m.maker.pubkey(), m.payout.pubkey(), m.env.treasury];
    owners
        .iter()
        .flat_map(|owner| [m.balance(owner, &m.usdc_mint), m.balance(owner, &m.local_mint)])
        .collect()
}

fn total(balances: &[u64], mint_index: usize) -> u128 {
    balances.iter().skip(mint_index).step_by(2).map(|&b| u128::from(b)).sum()
}

proptest! {
    // Each case builds a full market in LiteSVM, so keep the count modest.
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn settlement_is_exact_or_changes_nothing(
        local_to_usdc in any::<bool>(),
        // Lower bound 0 so zero amounts (now rejected) are exercised too.
        amount_in in 0..=MAX_AMOUNT,
        amount_out in 0..=MAX_AMOUNT,
        fee_bps in 0..=corridor_program::MAX_FEE_BPS,
        slippage_room in 0..=10_000u64,
    ) {
        let mut m = Market::new();
        let admin = m.env.admin.insecure_clone();
        send(&mut m.env.svm, &[ix_update_params(&admin.pubkey(), Some(fee_bps), None, None)], &[&admin]).unwrap();

        let direction = if local_to_usdc { Direction::LocalToUsdc } else { Direction::UsdcToLocal };
        let mut q = m.quote(direction);
        q.amount_in = amount_in;
        q.amount_out = amount_out;
        let fee = amount_out * u64::from(fee_bps) / 10_000;
        let net = amount_out - fee;
        // Sometimes demand slightly more than the fee leaves.
        q.min_amount_out = (net + 5_000).saturating_sub(slippage_room);

        // Reference model, using the market's default limits.
        let (usdc_amount, local_amount) = if local_to_usdc { (amount_out, amount_in) } else { (amount_in, amount_out) };
        let expected_ok = amount_in > 0
            && amount_out > 0
            && usdc_amount <= MAX_TRADE_USDC
            && local_amount <= MAX_TRADE_AMOUNT
            && net >= q.min_amount_out;

        let before = snapshot(&m);
        let taker_before: Participant = fetch(&m.env.svm, &participant_pda(&m.taker.pubkey()));
        let result = m.settle(&q);
        let after = snapshot(&m);

        prop_assert_eq!(result.is_ok(), expected_ok, "model disagrees: {:?}", result.err().map(|e| e.err));
        // Tokens are never created or destroyed, whatever happened.
        prop_assert_eq!(total(&before, 0), total(&after, 0));
        prop_assert_eq!(total(&before, 1), total(&after, 1));

        let taker_after: Participant = fetch(&m.env.svm, &participant_pda(&m.taker.pubkey()));
        if expected_ok {
            // Index layout: [owner * 2 + (0 = usdc, 1 = local)], owners = taker, maker, payout, treasury.
            let (in_i, out_i) = if local_to_usdc { (1, 0) } else { (0, 1) };
            prop_assert_eq!(after[in_i], before[in_i] - amount_in);
            prop_assert_eq!(after[2 + in_i], before[2 + in_i] + amount_in);
            prop_assert_eq!(after[2 + out_i], before[2 + out_i] - amount_out);
            prop_assert_eq!(after[4 + out_i], before[4 + out_i] + net);
            prop_assert_eq!(after[6 + out_i], before[6 + out_i] + fee);
            prop_assert_eq!(taker_after.day_volume_usdc, taker_before.day_volume_usdc + usdc_amount);
        } else {
            prop_assert_eq!(&after, &before);
            prop_assert_eq!(taker_after.day_volume_usdc, taker_before.day_volume_usdc);
        }
    }
}
