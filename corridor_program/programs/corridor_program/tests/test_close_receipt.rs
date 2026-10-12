mod common;

use {
    common::{market::*, *},
    corridor_program::{error::CorridorError, events::ReceiptClosed, quote::Direction},
    solana_signer::Signer,
};

/// Settles one default quote and returns it with its receipt address.
fn settled(m: &mut Market) -> (Quote, anchor_lang::prelude::Pubkey) {
    let q = m.quote(Direction::LocalToUsdc);
    m.settle(&q).unwrap();
    let receipt = receipt_pda(&m.maker.pubkey(), q.quote_id);
    (q, receipt)
}

fn retention_elapsed(q: &Quote) -> i64 {
    q.expires_at + DEFAULT_RETENTION_SECS as i64 + 1
}

#[test]
fn crank_closes_receipt_and_rent_returns_to_taker() {
    let mut m = Market::new();
    let (q, receipt) = settled(&mut m);
    let crank = funded(&mut m.env.svm);
    let taker = m.taker.pubkey();
    let rent = m.env.svm.get_balance(&receipt).unwrap();
    let taker_before = m.env.svm.get_balance(&taker).unwrap();
    let crank_before = m.env.svm.get_balance(&crank.pubkey()).unwrap();
    set_time(&mut m.env.svm, retention_elapsed(&q));

    let meta = send(&mut m.env.svm, &[ix_close_receipt(&crank.pubkey(), &receipt, &taker)], &[&crank]).unwrap();

    assert!(m.env.svm.get_account(&receipt).is_none(), "receipt must be closed");
    assert_eq!(m.env.svm.get_balance(&taker).unwrap(), taker_before + rent);
    assert!(m.env.svm.get_balance(&crank.pubkey()).unwrap() < crank_before, "crank only pays the fee");
    let event: ReceiptClosed = find_event(&meta).unwrap();
    assert_eq!((event.receipt, event.rent_payer, event.closed_by), (receipt, taker, crank.pubkey()));
}

#[test]
fn taker_can_close_its_own_receipt() {
    let mut m = Market::new();
    let (q, receipt) = settled(&mut m);
    set_time(&mut m.env.svm, retention_elapsed(&q));
    let taker = m.taker.pubkey();

    m.send_as_taker(&[ix_close_receipt(&taker, &receipt, &taker)]).unwrap();
    assert!(m.env.svm.get_account(&receipt).is_none());
}

/// C22 is strict: at exactly expires_at + retention the receipt stays.
#[test]
fn receipt_cannot_close_before_retention_elapses() {
    let mut m = Market::new();
    let (q, receipt) = settled(&mut m);
    let crank = funded(&mut m.env.svm);
    let taker = m.taker.pubkey();
    set_time(&mut m.env.svm, retention_elapsed(&q) - 1);

    let res = send(&mut m.env.svm, &[ix_close_receipt(&crank.pubkey(), &receipt, &taker)], &[&crank]);
    assert_err(res, CorridorError::RetentionNotElapsed);
}

#[test]
fn rent_cannot_be_redirected_to_the_caller() {
    let mut m = Market::new();
    let (q, receipt) = settled(&mut m);
    let crank = funded(&mut m.env.svm);
    set_time(&mut m.env.svm, retention_elapsed(&q));

    let res = send(&mut m.env.svm, &[ix_close_receipt(&crank.pubkey(), &receipt, &crank.pubkey())], &[&crank]);
    assert_err(res, CorridorError::RentPayerMismatch);
}

#[test]
fn close_works_while_paused() {
    let mut m = Market::new();
    let (q, receipt) = settled(&mut m);
    let guardian = m.env.guardian.insecure_clone();
    send(&mut m.env.svm, &[ix_set_paused(&guardian.pubkey(), true)], &[&guardian]).unwrap();
    set_time(&mut m.env.svm, retention_elapsed(&q));
    let taker = m.taker.pubkey();

    m.send_as_taker(&[ix_close_receipt(&taker, &receipt, &taker)]).unwrap();
}

#[test]
fn receipt_cannot_be_closed_twice() {
    let mut m = Market::new();
    let (q, receipt) = settled(&mut m);
    set_time(&mut m.env.svm, retention_elapsed(&q));
    let taker = m.taker.pubkey();
    m.send_as_taker(&[ix_close_receipt(&taker, &receipt, &taker)]).unwrap();

    assert!(m.send_as_taker(&[ix_close_receipt(&taker, &receipt, &taker)]).is_err());
}

/// The guarantee UC-12 rests on: once the receipt is gone, resubmitting the
/// identical signed quote fails on expiry rather than settling again.
#[test]
fn replay_after_close_fails_quote_expired() {
    let mut m = Market::new();
    let (q, receipt) = settled(&mut m);
    set_time(&mut m.env.svm, retention_elapsed(&q));
    let taker = m.taker.pubkey();
    m.send_as_taker(&[ix_close_receipt(&taker, &receipt, &taker)]).unwrap();
    let payout_before = m.balance(&m.payout.pubkey(), &m.usdc_mint.clone());

    let ixs = [m.signature_ix(&q), m.settle_ix(&q, m.default_accounts(&q))];
    m.assert_settle_err(&ixs, CorridorError::QuoteExpired);
    assert_eq!(m.balance(&m.payout.pubkey(), &m.usdc_mint.clone()), payout_before);
}

/// Retention is read from Config at close time, and zero is safe: the quote
/// has still expired before its receipt can go.
#[test]
fn zero_retention_allows_close_right_after_expiry() {
    let mut m = Market::new();
    let (q, receipt) = settled(&mut m);
    let admin = m.env.admin.insecure_clone();
    send(&mut m.env.svm, &[ix_update_params(&admin.pubkey(), None, None, Some(0))], &[&admin]).unwrap();
    let taker = m.taker.pubkey();

    set_time(&mut m.env.svm, q.expires_at);
    let res = m.send_as_taker(&[ix_close_receipt(&taker, &receipt, &taker)]);
    assert_err(res, CorridorError::RetentionNotElapsed);

    set_time(&mut m.env.svm, q.expires_at + 1);
    m.send_as_taker(&[ix_close_receipt(&taker, &receipt, &taker)]).unwrap();
}
