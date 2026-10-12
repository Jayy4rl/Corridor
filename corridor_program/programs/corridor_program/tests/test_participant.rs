mod common;

use {
    anchor_lang::prelude::Pubkey,
    common::*,
    corridor_program::{
        error::CorridorError,
        events::{ParticipantRegistered, QuoteSignerRotated},
        state::{Participant, Role, Status},
        SECONDS_PER_DAY,
    },
    solana_signer::Signer,
};

fn fetch_participant(env: &Env, wallet: &Pubkey) -> Participant {
    fetch(&env.svm, &participant_pda(wallet))
}

fn register(
    env: &mut Env,
    wallet: &Pubkey,
    role: Role,
    max_trade: u64,
    daily_limit: u64,
    quote_signer: Option<Pubkey>,
) -> litesvm::types::TransactionResult {
    send(
        &mut env.svm,
        &[ix_register_participant(&env.compliance.pubkey(), wallet, role, max_trade, daily_limit, quote_signer)],
        &[&env.compliance],
    )
}

// UC-07 register_participant

#[test]
fn register_taker_sets_fields_and_emits_role_and_hash() {
    let mut env = Env::new();
    let wallet = Pubkey::new_unique();

    let meta = register(&mut env, &wallet, Role::Taker, MAX_TRADE_USDC, DAILY_LIMIT_USDC, None).unwrap();

    let participant = fetch_participant(&env, &wallet);
    assert_eq!(participant.wallet, wallet);
    assert_eq!(participant.role, Role::Taker);
    assert_eq!(participant.kyb_ref_hash, KYB_HASH);
    assert_eq!(participant.quote_signer, Pubkey::default());
    assert_eq!(participant.max_trade_usdc, MAX_TRADE_USDC);
    assert_eq!(participant.daily_limit_usdc, DAILY_LIMIT_USDC);
    assert_eq!(participant.day_index, START_TIME / SECONDS_PER_DAY);
    assert_eq!(participant.day_volume_usdc, 0);
    assert_eq!(participant.status, Status::Active);

    let event: ParticipantRegistered = find_event(&meta).unwrap();
    assert_eq!((event.wallet, event.role, event.kyb_ref_hash), (wallet, Role::Taker, KYB_HASH));
}

#[test]
fn maker_quote_signer_defaults_to_wallet() {
    let mut env = Env::new();
    let wallet = Pubkey::new_unique();

    register(&mut env, &wallet, Role::Maker, MAX_TRADE_USDC, DAILY_LIMIT_USDC, None).unwrap();

    assert_eq!(fetch_participant(&env, &wallet).quote_signer, wallet);
}

#[test]
fn maker_can_register_with_dedicated_quote_key() {
    let mut env = Env::new();
    let (wallet, quote_key) = (Pubkey::new_unique(), Pubkey::new_unique());

    register(&mut env, &wallet, Role::Maker, MAX_TRADE_USDC, DAILY_LIMIT_USDC, Some(quote_key)).unwrap();

    assert_eq!(fetch_participant(&env, &wallet).quote_signer, quote_key);
}

#[test]
fn non_maker_cannot_have_quote_signer() {
    let mut env = Env::new();
    let res = register(
        &mut env,
        &Pubkey::new_unique(),
        Role::Taker,
        MAX_TRADE_USDC,
        DAILY_LIMIT_USDC,
        Some(Pubkey::new_unique()),
    );
    assert_err(res, CorridorError::WrongRole);
}

#[test]
fn payout_partner_needs_no_limits() {
    let mut env = Env::new();
    let wallet = Pubkey::new_unique();

    register(&mut env, &wallet, Role::PayoutPartner, 0, 0, None).unwrap();

    assert_eq!(fetch_participant(&env, &wallet).role, Role::PayoutPartner);
}

#[test]
fn takers_and_makers_need_both_limits() {
    for (role, max_trade, daily_limit) in [
        (Role::Taker, 0, DAILY_LIMIT_USDC),
        (Role::Taker, MAX_TRADE_USDC, 0),
        (Role::Maker, 0, DAILY_LIMIT_USDC),
        (Role::Maker, MAX_TRADE_USDC, 0),
    ] {
        let mut env = Env::new();
        let res = register(&mut env, &Pubkey::new_unique(), role, max_trade, daily_limit, None);
        assert_err(res, CorridorError::InvalidLimits);
    }
}

#[test]
fn register_rejects_admin() {
    let mut env = Env::new();
    let res = send(
        &mut env.svm,
        &[ix_register_participant(
            &env.admin.pubkey(),
            &Pubkey::new_unique(),
            Role::Taker,
            MAX_TRADE_USDC,
            DAILY_LIMIT_USDC,
            None,
        )],
        &[&env.admin],
    );
    assert_err(res, CorridorError::Unauthorized);
}

#[test]
fn register_same_wallet_twice_fails() {
    let mut env = Env::new();
    let wallet = env.participant(Role::Taker).pubkey();

    let res = register(&mut env, &wallet, Role::Maker, MAX_TRADE_USDC, DAILY_LIMIT_USDC, None);
    assert!(res.is_err());
}

// UC-08 update_participant

#[test]
fn update_changes_limits_and_hash_but_not_role_or_counter() {
    let mut env = Env::new();
    let wallet = env.participant(Role::Taker).pubkey();
    let before = fetch_participant(&env, &wallet);

    send(
        &mut env.svm,
        &[ix_update_participant(&env.compliance.pubkey(), &wallet, Some([5u8; 32]), None, Some(42))],
        &[&env.compliance],
    )
    .unwrap();

    let after = fetch_participant(&env, &wallet);
    assert_eq!(after.kyb_ref_hash, [5u8; 32]);
    assert_eq!(after.max_trade_usdc, before.max_trade_usdc);
    assert_eq!(after.daily_limit_usdc, 42);
    assert_eq!(after.role, Role::Taker);
    assert_eq!((after.day_index, after.day_volume_usdc), (before.day_index, before.day_volume_usdc));
}

#[test]
fn update_cannot_zero_a_taker_limit() {
    let mut env = Env::new();
    let wallet = env.participant(Role::Taker).pubkey();

    let res = send(
        &mut env.svm,
        &[ix_update_participant(&env.compliance.pubkey(), &wallet, None, Some(0), None)],
        &[&env.compliance],
    );
    assert_err(res, CorridorError::InvalidLimits);
}

#[test]
fn update_rejects_guardian() {
    let mut env = Env::new();
    let wallet = env.participant(Role::Maker).pubkey();

    let res = send(
        &mut env.svm,
        &[ix_update_participant(&env.guardian.pubkey(), &wallet, None, Some(1), None)],
        &[&env.guardian],
    );
    assert_err(res, CorridorError::Unauthorized);
}

// UC-09 set_participant_status

#[test]
fn guardian_suspends_participant_but_cannot_reactivate() {
    let mut env = Env::new();
    let wallet = env.participant(Role::Maker).pubkey();

    send(
        &mut env.svm,
        &[ix_set_participant_status(&env.guardian.pubkey(), &wallet, Status::Suspended)],
        &[&env.guardian],
    )
    .unwrap();
    assert_eq!(fetch_participant(&env, &wallet).status, Status::Suspended);

    let res = send(
        &mut env.svm,
        &[ix_set_participant_status(&env.guardian.pubkey(), &wallet, Status::Active)],
        &[&env.guardian],
    );
    assert_err(res, CorridorError::GuardianCannotResume);
}

#[test]
fn compliance_reactivates_participant() {
    let mut env = Env::new();
    let wallet = env.participant(Role::Taker).pubkey();
    send(
        &mut env.svm,
        &[ix_set_participant_status(&env.guardian.pubkey(), &wallet, Status::Suspended)],
        &[&env.guardian],
    )
    .unwrap();

    send(
        &mut env.svm,
        &[ix_set_participant_status(&env.compliance.pubkey(), &wallet, Status::Active)],
        &[&env.compliance],
    )
    .unwrap();

    assert_eq!(fetch_participant(&env, &wallet).status, Status::Active);
}

#[test]
fn admin_cannot_change_participant_status() {
    let mut env = Env::new();
    let wallet = env.participant(Role::Taker).pubkey();

    let res = send(
        &mut env.svm,
        &[ix_set_participant_status(&env.admin.pubkey(), &wallet, Status::Suspended)],
        &[&env.admin],
    );
    assert_err(res, CorridorError::Unauthorized);
}

// UC-10 set_quote_signer

#[test]
fn maker_rotates_quote_signer() {
    let mut env = Env::new();
    let maker = env.participant(Role::Maker);
    let new_key = Pubkey::new_unique();

    let meta = send(
        &mut env.svm,
        &[ix_set_quote_signer(&maker.pubkey(), &maker.pubkey(), new_key)],
        &[&maker],
    )
    .unwrap();

    assert_eq!(fetch_participant(&env, &maker.pubkey()).quote_signer, new_key);
    let event: QuoteSignerRotated = find_event(&meta).unwrap();
    assert_eq!(
        (event.maker, event.old_quote_signer, event.new_quote_signer),
        (maker.pubkey(), maker.pubkey(), new_key)
    );
}

#[test]
fn rotation_to_same_key_fails() {
    let mut env = Env::new();
    let maker = env.participant(Role::Maker);

    let res = send(
        &mut env.svm,
        &[ix_set_quote_signer(&maker.pubkey(), &maker.pubkey(), maker.pubkey())],
        &[&maker],
    );
    assert_err(res, CorridorError::SameSigner);
}

#[test]
fn taker_cannot_set_quote_signer() {
    let mut env = Env::new();
    let taker = env.participant(Role::Taker);

    let res = send(
        &mut env.svm,
        &[ix_set_quote_signer(&taker.pubkey(), &taker.pubkey(), Pubkey::new_unique())],
        &[&taker],
    );
    assert_err(res, CorridorError::WrongRole);
}

/// Deviation from UC-10: rotation is allowed while suspended so a leaked key
/// is dead before compliance reactivates the maker.
#[test]
fn suspended_maker_can_rotate_before_reactivation() {
    let mut env = Env::new();
    let maker = env.participant(Role::Maker);
    send(
        &mut env.svm,
        &[ix_set_participant_status(&env.guardian.pubkey(), &maker.pubkey(), Status::Suspended)],
        &[&env.guardian],
    )
    .unwrap();

    let new_key = Pubkey::new_unique();
    send(
        &mut env.svm,
        &[ix_set_quote_signer(&maker.pubkey(), &maker.pubkey(), new_key)],
        &[&maker],
    )
    .unwrap();

    let participant = fetch_participant(&env, &maker.pubkey());
    assert_eq!(participant.quote_signer, new_key);
    assert_eq!(participant.status, Status::Suspended, "rotation must not reactivate");
}

#[test]
fn nobody_else_can_rotate_a_makers_key() {
    let mut env = Env::new();
    let maker = env.participant(Role::Maker);
    let other_maker = env.participant(Role::Maker);

    let res = send(
        &mut env.svm,
        &[ix_set_quote_signer(&other_maker.pubkey(), &maker.pubkey(), other_maker.pubkey())],
        &[&other_maker],
    );
    assert_err(res, CorridorError::Unauthorized);
}

/// The quote key is a hot key that signs quotes only; it must not be able to
/// replace itself or lock the maker out.
#[test]
fn quote_key_cannot_rotate_itself() {
    let mut env = Env::new();
    let maker = env.participant(Role::Maker);
    let quote_key = funded(&mut env.svm);
    send(
        &mut env.svm,
        &[ix_set_quote_signer(&maker.pubkey(), &maker.pubkey(), quote_key.pubkey())],
        &[&maker],
    )
    .unwrap();

    let res = send(
        &mut env.svm,
        &[ix_set_quote_signer(&quote_key.pubkey(), &maker.pubkey(), Pubkey::new_unique())],
        &[&quote_key],
    );
    assert_err(res, CorridorError::Unauthorized);
}
