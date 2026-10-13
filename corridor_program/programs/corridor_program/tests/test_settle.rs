// LiteSVM's FailedTransactionMetadata is large; the helpers return it as-is.
#![allow(clippy::result_large_err)]

mod common;

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_instruction},
    },
    common::{market::*, *},
    corridor_program::{
        error::CorridorError,
        events::QuoteSettled,
        quote::{quote_message, Direction},
        state::{IssuerEntry, Participant, QuoteReceipt, Role, Status},
        DELEGATE_SEED, MAX_QUOTE_TTL, SECONDS_PER_DAY,
    },
    solana_keypair::Keypair,
    solana_signer::Signer,
};

// ---- Helpers ----

/// A second maker with its own quote key, funded inventory and approvals.
fn add_maker(m: &mut Market) -> (Keypair, Keypair) {
    let maker = m.env.participant(Role::Maker);
    let quote_key = Keypair::new();
    send(
        &mut m.env.svm,
        &[ix_set_quote_signer(&maker.pubkey(), &maker.pubkey(), quote_key.pubkey())],
        &[&maker],
    )
    .unwrap();
    for mint in [m.usdc_mint, m.local_mint] {
        m.open_account(&maker.pubkey(), &mint);
        m.fund(&maker.pubkey(), &mint, STARTING_BALANCE);
        m.approve(&maker, &mint, &delegate_pda(&maker.pubkey()), u64::MAX);
    }
    (maker, quote_key)
}

/// A second taker with funded token accounts.
fn add_taker(m: &mut Market) -> Keypair {
    let taker = m.env.participant(Role::Taker);
    for mint in [m.usdc_mint, m.local_mint] {
        m.open_account(&taker.pubkey(), &mint);
        m.fund(&taker.pubkey(), &mint, STARTING_BALANCE);
    }
    taker
}

fn admin_send(m: &mut Market, build: impl FnOnce(&Market) -> Instruction) {
    let ix = build(m);
    let admin = m.env.admin.insecure_clone();
    send(&mut m.env.svm, &[ix], &[&admin]).unwrap();
}

fn compliance_send(m: &mut Market, build: impl FnOnce(&Market) -> Instruction) {
    let ix = build(m);
    let compliance = m.env.compliance.insecure_clone();
    send(&mut m.env.svm, &[ix], &[&compliance]).unwrap();
}

fn guardian_send(m: &mut Market, build: impl FnOnce(&Market) -> Instruction) {
    let ix = build(m);
    let guardian = m.env.guardian.insecure_clone();
    send(&mut m.env.svm, &[ix], &[&guardian]).unwrap();
}

/// Settles with the default signature and the given (possibly hostile) accounts.
fn settle_with_accounts(m: &mut Market, q: &Quote, accounts: corridor_program::accounts::SettleQuote, expected: CorridorError) {
    let ixs = [m.signature_ix(q), m.settle_ix(q, accounts)];
    m.assert_settle_err(&ixs, expected);
}

fn expect_settle_err(m: &mut Market, q: &Quote, expected: CorridorError) {
    let accounts = m.default_accounts(q);
    settle_with_accounts(m, q, accounts, expected);
}

// ---- Layer 1: happy path, both directions ----

#[test]
fn local_to_usdc_moves_all_three_legs() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let fee = m.fee(q.amount_out);
    let (taker, maker, payout, treasury) =
        (m.taker.pubkey(), m.maker.pubkey(), m.payout.pubkey(), m.env.treasury);
    let (local, usdc) = (m.local_mint, m.usdc_mint);

    m.settle(&q).unwrap();

    assert_eq!(m.balance(&taker, &local), STARTING_BALANCE - LOCAL_AMOUNT);
    assert_eq!(m.balance(&maker, &local), STARTING_BALANCE + LOCAL_AMOUNT);
    assert_eq!(m.balance(&maker, &usdc), STARTING_BALANCE - USDC_AMOUNT);
    assert_eq!(m.balance(&payout, &usdc), USDC_AMOUNT - fee);
    assert_eq!(m.balance(&treasury, &usdc), fee);
    assert_eq!(fee, 1_000_000, "10 bps of 1,000 USDC is 1 USDC");
}

#[test]
fn usdc_to_local_charges_fee_in_local_token() {
    let mut m = Market::new();
    let q = m.quote(Direction::UsdcToLocal);
    let fee = m.fee(q.amount_out);
    let (taker, maker, payout, treasury) =
        (m.taker.pubkey(), m.maker.pubkey(), m.payout.pubkey(), m.env.treasury);
    let (local, usdc) = (m.local_mint, m.usdc_mint);

    m.settle(&q).unwrap();

    assert_eq!(m.balance(&taker, &usdc), STARTING_BALANCE - USDC_AMOUNT);
    assert_eq!(m.balance(&maker, &usdc), STARTING_BALANCE + USDC_AMOUNT);
    assert_eq!(m.balance(&maker, &local), STARTING_BALANCE - LOCAL_AMOUNT);
    assert_eq!(m.balance(&payout, &local), LOCAL_AMOUNT - fee);
    assert_eq!(m.balance(&treasury, &local), fee);
}

#[test]
fn maker_allowance_shrinks_by_amount_out() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let maker_usdc = m.token_account(&m.maker.pubkey(), &m.usdc_mint);

    m.settle(&q).unwrap();

    let account = token_account_state(&m.env.svm, &maker_usdc);
    assert_eq!(account.delegated_amount, u64::MAX - USDC_AMOUNT);
}

#[test]
fn taker_can_be_its_own_recipient() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let taker = m.taker.pubkey();
    let accounts = m.settle_accounts(&q, &m.maker.pubkey(), &taker, &taker);

    let ixs = [m.signature_ix(&q), m.settle_ix(&q, accounts)];
    m.send_as_taker(&ixs).unwrap();

    let fee = m.fee(q.amount_out);
    assert_eq!(m.balance(&taker, &m.usdc_mint.clone()), STARTING_BALANCE + USDC_AMOUNT - fee);
}

#[test]
fn zero_fee_skips_treasury_leg() {
    let mut m = Market::new();
    let admin = m.env.admin.insecure_clone();
    send(&mut m.env.svm, &[ix_update_params(&admin.pubkey(), Some(0), None, None)], &[&admin]).unwrap();
    let q = m.quote(Direction::LocalToUsdc);

    m.settle(&q).unwrap();

    assert_eq!(m.balance(&m.env.treasury.clone(), &m.usdc_mint.clone()), 0);
    assert_eq!(m.balance(&m.payout.pubkey(), &m.usdc_mint.clone()), USDC_AMOUNT);
}

#[test]
fn settlement_writes_receipt_event_and_counters() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let maker = m.maker.pubkey();

    let meta = m.settle(&q).unwrap();
    println!("settle_quote compute units: {}", meta.compute_units_consumed);

    let receipt: QuoteReceipt = fetch(&m.env.svm, &receipt_pda(&maker, q.quote_id));
    assert_eq!(receipt.expires_at, q.expires_at);
    assert_eq!(receipt.rent_payer, m.taker.pubkey());

    let event: QuoteSettled = find_event(&meta).expect("QuoteSettled not emitted");
    assert_eq!(event.quote_id, q.quote_id);
    assert_eq!((event.maker, event.taker, event.recipient), (maker, m.taker.pubkey(), m.payout.pubkey()));
    assert_eq!((event.mint_in, event.mint_out), (m.local_mint, m.usdc_mint));
    assert_eq!((event.amount_in, event.amount_out, event.fee), (LOCAL_AMOUNT, USDC_AMOUNT, m.fee(USDC_AMOUNT)));
    assert_eq!(event.compliance_hash, COMPLIANCE_HASH);
    assert_eq!(event.settled_at, now(&m.env.svm));

    let taker: Participant = fetch(&m.env.svm, &participant_pda(&m.taker.pubkey()));
    let maker_p: Participant = fetch(&m.env.svm, &participant_pda(&maker));
    assert_eq!(taker.day_volume_usdc, USDC_AMOUNT);
    assert_eq!(maker_p.day_volume_usdc, USDC_AMOUNT);
    let local: IssuerEntry = fetch(&m.env.svm, &issuer_pda(&m.local_mint));
    assert_eq!(local.day_volume, LOCAL_AMOUNT);
}

// ---- Layer 2: account binding (C7-C13, F9) ----

/// F9: maker A's quote must never be paid from maker B's inventory, even
/// though B has an open allowance to the program.
#[test]
fn f9_taker_cannot_drain_another_makers_inventory() {
    let mut m = Market::new();
    let (maker_b, _) = add_maker(&mut m);
    let q = m.quote(Direction::LocalToUsdc);
    let victim_before = m.balance(&maker_b.pubkey(), &m.usdc_mint.clone());

    let mut accounts = m.default_accounts(&q);
    accounts.maker_ata_out = m.token_account(&maker_b.pubkey(), &m.usdc_mint);
    settle_with_accounts(&mut m, &q, accounts, CorridorError::AccountOwnerMismatch);

    assert_eq!(m.balance(&maker_b.pubkey(), &m.usdc_mint.clone()), victim_before);
}

/// F9, second half: swapping in maker B's Participant and delegate as well
/// fails, because maker A's quote is not signed by B's quote key.
#[test]
fn f9_quote_cannot_be_rebound_to_another_maker() {
    let mut m = Market::new();
    let (maker_b, _) = add_maker(&mut m);
    let q = m.quote(Direction::LocalToUsdc);

    let accounts = m.settle_accounts(&q, &maker_b.pubkey(), &m.taker.pubkey(), &m.payout.pubkey());
    settle_with_accounts(&mut m, &q, accounts, CorridorError::InvalidQuoteSignature);
}

/// The v1 design's shared ["delegate"] PDA is no longer an accepted delegate.
#[test]
fn approval_to_shared_v1_delegate_is_not_spendable() {
    let mut m = Market::new();
    let shared = Pubkey::find_program_address(&[DELEGATE_SEED], &program_id()).0;
    let maker = m.maker.insecure_clone();
    m.approve(&maker, &m.usdc_mint.clone(), &shared, u64::MAX);
    let q = m.quote(Direction::LocalToUsdc);

    expect_settle_err(&mut m, &q, CorridorError::InsufficientAllowance);
}

#[test]
fn allowance_below_amount_out_fails() {
    let mut m = Market::new();
    let maker = m.maker.insecure_clone();
    m.approve(&maker, &m.usdc_mint.clone(), &delegate_pda(&maker.pubkey()), USDC_AMOUNT - 1);
    let q = m.quote(Direction::LocalToUsdc);

    expect_settle_err(&mut m, &q, CorridorError::InsufficientAllowance);
}

#[test]
fn recipient_must_be_taker_or_payout_partner() {
    let mut m = Market::new();
    let other_taker = add_taker(&mut m);
    let q = m.quote(Direction::LocalToUsdc);

    let accounts = m.settle_accounts(&q, &m.maker.pubkey(), &m.taker.pubkey(), &other_taker.pubkey());
    settle_with_accounts(&mut m, &q, accounts, CorridorError::RecipientNotAllowed);
}

#[test]
fn suspended_payout_partner_cannot_receive() {
    let mut m = Market::new();
    let payout = m.payout.pubkey();
    guardian_send(&mut m, |m| ix_set_participant_status(&m.env.guardian.pubkey(), &payout, Status::Suspended));
    let q = m.quote(Direction::LocalToUsdc);

    expect_settle_err(&mut m, &q, CorridorError::RecipientNotAllowed);
}

#[test]
fn recipient_token_account_must_belong_to_recipient() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let mut accounts = m.default_accounts(&q);
    accounts.recipient_ata_out = m.token_account(&m.taker.pubkey(), &m.usdc_mint);

    settle_with_accounts(&mut m, &q, accounts, CorridorError::AccountOwnerMismatch);
}

#[test]
fn treasury_token_account_must_belong_to_treasury() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let attacker_account = m.open_account(&Pubkey::new_unique(), &m.usdc_mint.clone());
    let mut accounts = m.default_accounts(&q);
    accounts.treasury_ata_out = attacker_account;

    settle_with_accounts(&mut m, &q, accounts, CorridorError::AccountOwnerMismatch);
}

#[test]
fn token_account_on_wrong_mint_fails() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let mut accounts = m.default_accounts(&q);
    accounts.taker_ata_in = m.token_account(&m.taker.pubkey(), &m.usdc_mint);

    settle_with_accounts(&mut m, &q, accounts, CorridorError::MintMismatch);
}

#[test]
fn paused_protocol_rejects_settlement() {
    let mut m = Market::new();
    guardian_send(&mut m, |m| ix_set_paused(&m.env.guardian.pubkey(), true));
    let q = m.quote(Direction::LocalToUsdc);

    expect_settle_err(&mut m, &q, CorridorError::Paused);
}

#[test]
fn suspended_taker_or_maker_cannot_settle() {
    for suspend_maker in [false, true] {
        let mut m = Market::new();
        let wallet = if suspend_maker { m.maker.pubkey() } else { m.taker.pubkey() };
        guardian_send(&mut m, |m| ix_set_participant_status(&m.env.guardian.pubkey(), &wallet, Status::Suspended));
        let q = m.quote(Direction::LocalToUsdc);

        expect_settle_err(&mut m, &q, CorridorError::ParticipantSuspended);
    }
}

#[test]
fn taker_participant_must_be_the_signer() {
    let mut m = Market::new();
    let other_taker = add_taker(&mut m);
    let q = m.quote(Direction::LocalToUsdc);
    let mut accounts = m.default_accounts(&q);
    accounts.taker_participant = participant_pda(&other_taker.pubkey());

    settle_with_accounts(&mut m, &q, accounts, CorridorError::Unauthorized);
}

#[test]
fn maker_cannot_act_as_taker() {
    let mut m = Market::new();
    let (maker_b, _) = add_maker(&mut m);
    let q = m.quote(Direction::LocalToUsdc);
    let accounts = m.settle_accounts(&q, &m.maker.pubkey(), &maker_b.pubkey(), &m.payout.pubkey());
    let ixs = [m.signature_ix(&q), m.settle_ix(&q, accounts)];

    let res = send(&mut m.env.svm, &ixs, &[&maker_b]);
    assert_err_at(res, 1, CorridorError::WrongRole);
}

#[test]
fn suspended_issuer_on_either_leg_blocks_settlement() {
    for suspend_usdc in [false, true] {
        let mut m = Market::new();
        let mint = if suspend_usdc { m.usdc_mint } else { m.local_mint };
        guardian_send(&mut m, |m| ix_set_issuer_status(&m.env.guardian.pubkey(), &mint, Status::Suspended));
        let q = m.quote(Direction::LocalToUsdc);

        expect_settle_err(&mut m, &q, CorridorError::IssuerSuspended);
    }
}

#[test]
fn expired_attestation_blocks_settlement() {
    let mut m = Market::new();
    let later = now(&m.env.svm) + ATTESTATION_TTL;
    set_time(&mut m.env.svm, later);
    let q = m.quote(Direction::LocalToUsdc);

    expect_settle_err(&mut m, &q, CorridorError::AttestationExpired);
}

#[test]
fn usdc_cannot_be_passed_as_local_leg() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let mut accounts = m.default_accounts(&q);
    accounts.local_mint = m.usdc_mint;
    accounts.local_issuer = issuer_pda(&m.usdc_mint);

    settle_with_accounts(&mut m, &q, accounts, CorridorError::PairMustIncludeUsdc);
}

#[test]
fn token_program_must_match_issuer_entry() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let mut accounts = m.default_accounts(&q);
    accounts.usdc_token_program = anchor_spl::token_2022::ID;

    settle_with_accounts(&mut m, &q, accounts, CorridorError::MintMismatch);
}

// ---- Layer 3: amounts, fees and limits (C17-C20, F12) ----

#[test]
fn fee_rounds_down() {
    let mut m = Market::new();
    let mut q = m.quote(Direction::LocalToUsdc);
    q.amount_out = 999; // 10 bps of 999 = 0.999 -> 0
    q.min_amount_out = 999;

    m.settle(&q).unwrap();

    assert_eq!(m.balance(&m.env.treasury.clone(), &m.usdc_mint.clone()), 0);
    assert_eq!(m.balance(&m.payout.pubkey(), &m.usdc_mint.clone()), 999);
}

/// F10c: an admin fee rise between quote and settlement cannot silently
/// short the taker.
#[test]
fn fee_rise_after_quote_trips_min_amount_out() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    admin_send(&mut m, |m| ix_update_params(&m.env.admin.pubkey(), Some(50), None, None));

    expect_settle_err(&mut m, &q, CorridorError::SlippageExceeded);
}

#[test]
fn local_amount_above_issuer_max_trade_fails() {
    let mut m = Market::new();
    admin_send(&mut m, |m| ix_update_issuer_caps(&m.env.admin.pubkey(), &m.local_mint.clone(), Some(LOCAL_AMOUNT - 1), None));
    let q = m.quote(Direction::LocalToUsdc);

    expect_settle_err(&mut m, &q, CorridorError::TradeTooLarge);
}

#[test]
fn usdc_amount_above_either_participant_max_trade_fails() {
    for limit_maker in [false, true] {
        let mut m = Market::new();
        let wallet = if limit_maker { m.maker.pubkey() } else { m.taker.pubkey() };
        compliance_send(&mut m, |m| ix_update_participant(&m.env.compliance.pubkey(), &wallet, None, Some(USDC_AMOUNT - 1), None));
        let q = m.quote(Direction::UsdcToLocal);

        expect_settle_err(&mut m, &q, CorridorError::TradeTooLarge);
    }
}

#[test]
fn taker_daily_limit_resets_at_utc_midnight() {
    let mut m = Market::new();
    let taker = m.taker.pubkey();
    compliance_send(&mut m, |m| ix_update_participant(&m.env.compliance.pubkey(), &taker, None, None, Some(USDC_AMOUNT * 3 / 2)));

    let q1 = m.quote(Direction::LocalToUsdc);
    m.settle(&q1).unwrap();
    let q2 = m.quote(Direction::LocalToUsdc);
    expect_settle_err(&mut m, &q2, CorridorError::DailyLimitExceeded);

    let next_day = (now(&m.env.svm) / SECONDS_PER_DAY + 1) * SECONDS_PER_DAY;
    set_time(&mut m.env.svm, next_day);
    let q3 = m.quote(Direction::LocalToUsdc);
    m.settle(&q3).unwrap();

    let participant: Participant = fetch(&m.env.svm, &participant_pda(&taker));
    assert_eq!(participant.day_index, next_day / SECONDS_PER_DAY);
    assert_eq!(participant.day_volume_usdc, USDC_AMOUNT, "counter must restart, not accumulate");
}

#[test]
fn maker_daily_limit_is_enforced() {
    let mut m = Market::new();
    let maker = m.maker.pubkey();
    compliance_send(&mut m, |m| ix_update_participant(&m.env.compliance.pubkey(), &maker, None, None, Some(USDC_AMOUNT)));

    let q1 = m.quote(Direction::UsdcToLocal);
    m.settle(&q1).unwrap();
    let q2 = m.quote(Direction::UsdcToLocal);
    expect_settle_err(&mut m, &q2, CorridorError::DailyLimitExceeded);
}

#[test]
fn local_issuer_daily_cap_is_enforced_in_its_own_units() {
    let mut m = Market::new();
    admin_send(&mut m, |m| ix_update_issuer_caps(&m.env.admin.pubkey(), &m.local_mint.clone(), None, Some(LOCAL_AMOUNT)));

    let q1 = m.quote(Direction::LocalToUsdc);
    m.settle(&q1).unwrap();
    let q2 = m.quote(Direction::LocalToUsdc);
    expect_settle_err(&mut m, &q2, CorridorError::DailyLimitExceeded);
}

/// F12: the USDC IssuerEntry is never written, so settlements across all
/// pairs do not serialise on it.
#[test]
fn usdc_issuer_is_read_only_and_untouched() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let ix = m.settle_ix(&q, m.default_accounts(&q));
    let usdc_issuer = issuer_pda(&m.usdc_mint);
    let meta = ix.accounts.iter().find(|meta| meta.pubkey == usdc_issuer).unwrap();
    assert!(!meta.is_writable);

    let before: IssuerEntry = fetch(&m.env.svm, &usdc_issuer);
    m.settle(&q).unwrap();
    let after: IssuerEntry = fetch(&m.env.svm, &usdc_issuer);
    assert_eq!((after.day_index, after.day_volume), (before.day_index, before.day_volume));
}

// ---- Layer 4: quote signature (C14-C16, F10) ----

#[test]
fn settlement_without_ed25519_instruction_fails() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let ixs = [m.settle_ix(&q, m.default_accounts(&q))];

    m.assert_settle_err(&ixs, CorridorError::MissingEd25519Instruction);
}

#[test]
fn ed25519_instruction_must_be_immediately_before() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let ixs = [
        m.signature_ix(&q),
        system_instruction::transfer(&m.taker.pubkey(), &m.payout.pubkey(), 1),
        m.settle_ix(&q, m.default_accounts(&q)),
    ];

    m.assert_settle_err(&ixs, CorridorError::MissingEd25519Instruction);
}

#[test]
fn quote_signed_by_maker_wallet_instead_of_quote_key_fails() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let message = m.message(&q, &m.maker.pubkey(), &m.taker.pubkey());
    let ixs = [ed25519_ix(&m.maker, &message), m.settle_ix(&q, m.default_accounts(&q))];

    m.assert_settle_err(&ixs, CorridorError::InvalidQuoteSignature);
}

#[test]
fn tampered_amount_fails() {
    let mut m = Market::new();
    let signed = m.quote(Direction::LocalToUsdc);
    let mut tampered = signed.clone();
    tampered.amount_out += 1;
    let ixs = [m.signature_ix(&signed), m.settle_ix(&tampered, m.default_accounts(&tampered))];

    m.assert_settle_err(&ixs, CorridorError::InvalidQuoteSignature);
}

#[test]
fn quote_for_another_taker_cannot_be_used() {
    let mut m = Market::new();
    let thief = add_taker(&mut m);
    let q = m.quote(Direction::LocalToUsdc);
    let accounts = m.settle_accounts(&q, &m.maker.pubkey(), &thief.pubkey(), &thief.pubkey());
    let ixs = [m.signature_ix(&q), m.settle_ix(&q, accounts)];

    let res = send(&mut m.env.svm, &ixs, &[&thief]);
    assert_err_at(res, 1, CorridorError::InvalidQuoteSignature);
}

/// F10b: a quote signed for another cluster (different genesis hash) fails.
#[test]
fn quote_from_another_cluster_fails() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let (mint_in, mint_out) = m.mints(q.direction);
    let devnet_message = quote_message(
        &[0u8; 32],
        q.quote_id,
        &m.maker.pubkey(),
        &m.taker.pubkey(),
        &mint_in,
        &mint_out,
        q.amount_in,
        q.amount_out,
        q.expires_at,
    );
    let ixs = [ed25519_ix(&m.quote_key, &devnet_message), m.settle_ix(&q, m.default_accounts(&q))];

    m.assert_settle_err(&ixs, CorridorError::InvalidQuoteSignature);
}

#[test]
fn rotation_kills_outstanding_quotes() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let signature = m.signature_ix(&q);
    let maker = m.maker.insecure_clone();
    send(
        &mut m.env.svm,
        &[ix_set_quote_signer(&maker.pubkey(), &maker.pubkey(), Pubkey::new_unique())],
        &[&maker],
    )
    .unwrap();

    let ixs = [signature, m.settle_ix(&q, m.default_accounts(&q))];
    m.assert_settle_err(&ixs, CorridorError::InvalidQuoteSignature);
}

/// F10a: offsets that point verification at another instruction's data are
/// rejected, even though the precompile itself accepts them.
#[test]
fn offsets_pointing_outside_the_ed25519_instruction_fail() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let mut signature = m.signature_ix(&q);
    // message_instruction_index (bytes 14..16) -> instruction 0, which is this
    // same Ed25519 instruction, so the precompile still verifies.
    signature.data[14..16].copy_from_slice(&0u16.to_le_bytes());
    let ixs = [signature, m.settle_ix(&q, m.default_accounts(&q))];

    m.assert_settle_err(&ixs, CorridorError::InvalidEd25519Offsets);
}

#[test]
fn ed25519_instruction_with_two_signatures_fails() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let message = m.message(&q, &m.maker.pubkey(), &m.taker.pubkey());
    let signature = m.quote_key.sign_message(&message);
    // header | two offset blocks | pubkey | signature | message, both blocks
    // describing the same valid signature.
    let (pubkey_at, signature_at) = (30u16, 62u16);
    let message_at = signature_at + 64;
    let mut data = vec![2u8, 0];
    for _ in 0..2 {
        for field in [signature_at, u16::MAX, pubkey_at, u16::MAX, message_at, message.len() as u16, u16::MAX] {
            data.extend_from_slice(&field.to_le_bytes());
        }
    }
    data.extend_from_slice(m.quote_key.pubkey().as_ref());
    data.extend_from_slice(signature.as_ref());
    data.extend_from_slice(&message);
    let ed25519 = Instruction {
        program_id: solana_sdk_ids::ed25519_program::ID,
        accounts: vec![],
        data,
    };
    let ixs = [ed25519, m.settle_ix(&q, m.default_accounts(&q))];

    m.assert_settle_err(&ixs, CorridorError::InvalidEd25519Offsets);
}

#[test]
fn forged_signature_is_rejected_by_the_runtime() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let mut signature = m.signature_ix(&q);
    signature.data[48] ^= 0xff; // first byte of the signature

    let ixs = [signature, m.settle_ix(&q, m.default_accounts(&q))];
    assert!(m.send_as_taker(&ixs).is_err());
}

#[test]
fn quote_expiring_now_is_expired() {
    let mut m = Market::new();
    let mut q = m.quote(Direction::LocalToUsdc);
    q.expires_at = now(&m.env.svm);

    expect_settle_err(&mut m, &q, CorridorError::QuoteExpired);
}

#[test]
fn quote_expires_with_time() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let later = q.expires_at + 1;
    set_time(&mut m.env.svm, later);

    expect_settle_err(&mut m, &q, CorridorError::QuoteExpired);
}

#[test]
fn quote_ttl_is_capped() {
    let mut m = Market::new();
    let mut q = m.quote(Direction::LocalToUsdc);
    q.expires_at = now(&m.env.svm) + MAX_QUOTE_TTL + 1;
    expect_settle_err(&mut m, &q, CorridorError::QuoteTtlTooLong);

    let mut q = m.quote(Direction::LocalToUsdc);
    q.expires_at = now(&m.env.svm) + MAX_QUOTE_TTL;
    m.settle(&q).unwrap();
}

// ---- Layer 5: replay protection (C21) ----

#[test]
fn same_quote_cannot_settle_twice() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    m.settle(&q).unwrap();
    let payout_after_first = m.balance(&m.payout.pubkey(), &m.usdc_mint.clone());

    assert!(m.settle(&q).is_err());
    assert_eq!(m.balance(&m.payout.pubkey(), &m.usdc_mint.clone()), payout_after_first);
}

#[test]
fn quote_ids_are_scoped_per_maker() {
    let mut m = Market::new();
    let (maker_b, quote_key_b) = add_maker(&mut m);
    let q = m.quote(Direction::LocalToUsdc);
    m.settle(&q).unwrap();

    let message = m.message(&q, &maker_b.pubkey(), &m.taker.pubkey());
    let accounts = m.settle_accounts(&q, &maker_b.pubkey(), &m.taker.pubkey(), &m.payout.pubkey());
    let ixs = [ed25519_ix(&quote_key_b, &message), m.settle_ix(&q, accounts)];
    m.send_as_taker(&ixs).unwrap();
}

// ---- Transaction size (spec gap #3) ----

/// A legacy transaction does not fit Solana's 1232-byte packet limit, so the
/// transaction builder must send v0 with an address lookup table holding the
/// accounts that are the same for every settlement. LiteSVM does not enforce
/// the packet limit, which is why this is measured rather than sent.
#[test]
fn settle_needs_v0_transaction_with_lookup_table() {
    const PACKET_DATA_SIZE: usize = 1232;
    const SIGNATURES: usize = 1 + 64; // compact-u16 count + one signature
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let ixs = [m.signature_ix(&q), m.settle_ix(&q, m.default_accounts(&q))];
    let taker = m.taker.pubkey();

    let legacy = SIGNATURES + solana_message::Message::new(&ixs, Some(&taker)).serialize().len();
    assert!(legacy > PACKET_DATA_SIZE, "legacy now fits ({legacy} bytes); the lookup table may be optional");

    let static_accounts = solana_message::AddressLookupTableAccount {
        key: Pubkey::new_unique(),
        addresses: vec![
            config_pda(),
            m.usdc_mint,
            m.local_mint,
            issuer_pda(&m.usdc_mint),
            issuer_pda(&m.local_mint),
            m.token_account(&m.env.treasury, &m.usdc_mint),
            m.token_account(&m.env.treasury, &m.local_mint),
            solana_sdk_ids::sysvar::instructions::ID,
            anchor_spl::token::ID,
            anchor_spl::token_2022::ID,
            anchor_lang::solana_program::system_program::ID,
            event_authority(),
        ],
    };
    let v0 = solana_message::v0::Message::try_compile(&taker, &ixs, &[static_accounts], m.env.svm.latest_blockhash())
        .unwrap();
    let v0_size = SIGNATURES + solana_message::VersionedMessage::V0(v0).serialize().len();
    println!("settle_quote: legacy {legacy} bytes, v0 with lookup table {v0_size} bytes (limit {PACKET_DATA_SIZE})");
    assert!(v0_size <= PACKET_DATA_SIZE, "v0 transaction is {v0_size} bytes");
}

// ---- Attack classes covered by constraints, pinned explicitly ----

/// A fake instructions sysvar would let an attacker feed the signature check
/// any "previous instruction" they like (the Wormhole exploit class). The
/// address constraint must reject it before the handler runs.
#[test]
fn spoofed_instructions_sysvar_is_rejected() {
    let mut m = Market::new();
    let q = m.quote(Direction::LocalToUsdc);
    let mut accounts = m.default_accounts(&q);
    accounts.instructions_sysvar = config_pda();
    let ixs = [m.signature_ix(&q), m.settle_ix(&q, accounts)];

    let res = m.send_as_taker(&ixs);
    let code = anchor_lang::error::ErrorCode::ConstraintAddress as u32;
    assert_eq!(
        res.unwrap_err().err,
        solana_transaction_error::TransactionError::InstructionError(
            1,
            solana_transaction::InstructionError::Custom(code)
        )
    );
}

/// The indexer treats QuoteSettled as the system of record (F4), so a
/// top-level call shaped like an emit_cpi! event must fail: only the program
/// itself can sign as the event authority.
#[test]
fn quote_settled_events_cannot_be_forged() {
    use anchor_lang::{event::EVENT_IX_TAG_LE, AnchorSerialize, Discriminator};
    let mut m = Market::new();
    let fake = QuoteSettled {
        quote_id: 1,
        maker: m.maker.pubkey(),
        taker: m.taker.pubkey(),
        recipient: m.payout.pubkey(),
        mint_in: m.local_mint,
        mint_out: m.usdc_mint,
        amount_in: 1,
        amount_out: u64::MAX,
        fee: 0,
        compliance_hash: [0; 32],
        settled_at: now(&m.env.svm),
    };
    let mut data = [EVENT_IX_TAG_LE, QuoteSettled::DISCRIMINATOR].concat();
    fake.serialize(&mut data).unwrap();
    let forged = Instruction {
        program_id: program_id(),
        accounts: vec![anchor_lang::solana_program::instruction::AccountMeta::new_readonly(event_authority(), false)],
        data,
    };

    assert!(m.send_as_taker(&[forged]).is_err());
}

#[test]
fn zero_amount_on_either_leg_is_rejected() {
    for zero_in in [true, false] {
        let mut m = Market::new();
        let mut q = m.quote(Direction::LocalToUsdc);
        if zero_in {
            q.amount_in = 0;
        } else {
            q.amount_out = 0;
            q.min_amount_out = 0;
        }

        expect_settle_err(&mut m, &q, CorridorError::ZeroAmount);
    }
}
