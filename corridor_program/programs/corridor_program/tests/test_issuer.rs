// LiteSVM's FailedTransactionMetadata is large; the helpers return it as-is.
#![allow(clippy::result_large_err)]

mod common;

use {
    anchor_lang::prelude::Pubkey,
    anchor_spl::{token::ID as TOKEN_ID, token_2022::ID as TOKEN_2022_ID},
    common::*,
    corridor_program::{
        error::CorridorError,
        events::{IssuerRegistered, IssuerStatusChanged},
        state::{IssuerEntry, Status},
        SECONDS_PER_DAY,
    },
    solana_signer::Signer,
};

fn register(env: &mut Env, mint: &Pubkey, expires_at: i64) -> litesvm::types::TransactionResult {
    send(
        &mut env.svm,
        &[ix_register_issuer(&env.admin.pubkey(), mint, expires_at, MAX_TRADE_AMOUNT, DAILY_CAP)],
        &[&env.admin],
    )
}

fn fetch_issuer(env: &Env, mint: &Pubkey) -> IssuerEntry {
    fetch(&env.svm, &issuer_pda(mint))
}

// UC-04 register_issuer

#[test]
fn register_spl_token_mint_records_program_and_decimals() {
    let mut env = Env::new();
    let mint = create_mint(&mut env.svm, &env.admin, &TOKEN_ID, &[], 6);
    let expires_at = START_TIME + ATTESTATION_TTL;

    let meta = register(&mut env, &mint, expires_at).unwrap();

    let issuer = fetch_issuer(&env, &mint);
    assert_eq!(issuer.mint, mint);
    assert_eq!(issuer.token_program, TOKEN_ID);
    assert_eq!(issuer.decimals, 6);
    assert_eq!(issuer.attestation_hash, ATTESTATION_HASH);
    assert_eq!(issuer.attestation_expires_at, expires_at);
    assert_eq!(issuer.max_trade_amount, MAX_TRADE_AMOUNT);
    assert_eq!(issuer.daily_cap, DAILY_CAP);
    assert_eq!(issuer.day_index, START_TIME / SECONDS_PER_DAY);
    assert_eq!(issuer.day_volume, 0);
    assert_eq!(issuer.status, Status::Active);

    let event: IssuerRegistered = find_event(&meta).unwrap();
    assert_eq!(event.mint, mint);
}

#[test]
fn register_plain_token_2022_mint() {
    let mut env = Env::new();
    let mint = create_mint(&mut env.svm, &env.admin, &TOKEN_2022_ID, &[], 2);

    register(&mut env, &mint, START_TIME + ATTESTATION_TTL).unwrap();

    let issuer = fetch_issuer(&env, &mint);
    assert_eq!(issuer.token_program, TOKEN_2022_ID);
    assert_eq!(issuer.decimals, 2);
}

#[test]
fn register_rejects_each_unsupported_extension() {
    for extension in [
        MintExtension::TransferFee,
        MintExtension::TransferHook,
        MintExtension::PermanentDelegate,
        MintExtension::MintCloseAuthority,
    ] {
        let mut env = Env::new();
        let mint = create_mint(&mut env.svm, &env.admin, &TOKEN_2022_ID, &[extension], 6);

        let res = register(&mut env, &mint, START_TIME + ATTESTATION_TTL);
        assert_err(res, CorridorError::UnsupportedMintExtension);
    }
}

#[test]
fn register_finds_unsupported_extension_among_allowed_ones() {
    let mut env = Env::new();
    let mint = create_mint(
        &mut env.svm,
        &env.admin,
        &TOKEN_2022_ID,
        &[MintExtension::MetadataPointer, MintExtension::PermanentDelegate],
        6,
    );

    let res = register(&mut env, &mint, START_TIME + ATTESTATION_TTL);
    assert_err(res, CorridorError::UnsupportedMintExtension);
}

#[test]
fn register_accepts_harmless_extension() {
    let mut env = Env::new();
    let mint = create_mint(&mut env.svm, &env.admin, &TOKEN_2022_ID, &[MintExtension::MetadataPointer], 6);

    register(&mut env, &mint, START_TIME + ATTESTATION_TTL).unwrap();
}

#[test]
fn register_rejects_attestation_expiring_now() {
    let mut env = Env::new();
    let mint = create_mint(&mut env.svm, &env.admin, &TOKEN_ID, &[], 6);

    let res = register(&mut env, &mint, START_TIME);
    assert_err(res, CorridorError::AttestationExpired);
}

#[test]
fn register_rejects_non_admin() {
    let mut env = Env::new();
    let mint = create_mint(&mut env.svm, &env.admin, &TOKEN_ID, &[], 6);

    let res = send(
        &mut env.svm,
        &[ix_register_issuer(&env.compliance.pubkey(), &mint, START_TIME + ATTESTATION_TTL, 1, 1)],
        &[&env.compliance],
    );
    assert_err(res, CorridorError::Unauthorized);
}

#[test]
fn register_rejects_non_mint_account() {
    let mut env = Env::new();
    let not_a_mint = env.compliance.pubkey();

    let res = register(&mut env, &not_a_mint, START_TIME + ATTESTATION_TTL);
    assert!(res.is_err());
}

#[test]
fn register_same_mint_twice_fails() {
    let mut env = Env::new();
    let mint = env.registered_mint(&TOKEN_ID);

    let res = register(&mut env, &mint, START_TIME + ATTESTATION_TTL);
    assert!(res.is_err());
}

// UC-05 update_issuer_attestation

#[test]
fn compliance_refreshes_attestation() {
    let mut env = Env::new();
    let mint = env.registered_mint(&TOKEN_ID);
    let new_expiry = START_TIME + 2 * ATTESTATION_TTL;

    send(
        &mut env.svm,
        &[ix_update_issuer_attestation(&env.compliance.pubkey(), &mint, [9u8; 32], new_expiry)],
        &[&env.compliance],
    )
    .unwrap();

    let issuer = fetch_issuer(&env, &mint);
    assert_eq!(issuer.attestation_hash, [9u8; 32]);
    assert_eq!(issuer.attestation_expires_at, new_expiry);
}

#[test]
fn update_attestation_rejects_admin() {
    let mut env = Env::new();
    let mint = env.registered_mint(&TOKEN_ID);

    let res = send(
        &mut env.svm,
        &[ix_update_issuer_attestation(&env.admin.pubkey(), &mint, [9u8; 32], START_TIME + ATTESTATION_TTL)],
        &[&env.admin],
    );
    assert_err(res, CorridorError::Unauthorized);
}

#[test]
fn update_attestation_rejects_past_expiry() {
    let mut env = Env::new();
    let mint = env.registered_mint(&TOKEN_ID);
    set_time(&mut env.svm, START_TIME + 1_000);

    let res = send(
        &mut env.svm,
        &[ix_update_issuer_attestation(&env.compliance.pubkey(), &mint, [9u8; 32], START_TIME + 999)],
        &[&env.compliance],
    );
    assert_err(res, CorridorError::AttestationExpired);
}

// Handler 9 update_issuer_caps

#[test]
fn admin_updates_caps_and_keeps_day_volume() {
    let mut env = Env::new();
    let mint = env.registered_mint(&TOKEN_ID);
    let before = fetch_issuer(&env, &mint);

    send(
        &mut env.svm,
        &[ix_update_issuer_caps(&env.admin.pubkey(), &mint, Some(5), None)],
        &[&env.admin],
    )
    .unwrap();

    let after = fetch_issuer(&env, &mint);
    assert_eq!(after.max_trade_amount, 5);
    assert_eq!(after.daily_cap, before.daily_cap);
    assert_eq!(after.day_volume, before.day_volume);
    assert_eq!(after.status, Status::Active);
}

#[test]
fn update_caps_rejects_guardian() {
    let mut env = Env::new();
    let mint = env.registered_mint(&TOKEN_ID);

    let res = send(
        &mut env.svm,
        &[ix_update_issuer_caps(&env.guardian.pubkey(), &mint, Some(0), Some(0))],
        &[&env.guardian],
    );
    assert_err(res, CorridorError::Unauthorized);
}

// Handler 10 set_issuer_status

#[test]
fn guardian_suspends_issuer_but_cannot_reactivate() {
    let mut env = Env::new();
    let mint = env.registered_mint(&TOKEN_ID);

    let meta = send(
        &mut env.svm,
        &[ix_set_issuer_status(&env.guardian.pubkey(), &mint, Status::Suspended)],
        &[&env.guardian],
    )
    .unwrap();
    assert_eq!(fetch_issuer(&env, &mint).status, Status::Suspended);
    let event: IssuerStatusChanged = find_event(&meta).unwrap();
    assert_eq!((event.mint, event.status, event.by), (mint, Status::Suspended, env.guardian.pubkey()));

    let res = send(
        &mut env.svm,
        &[ix_set_issuer_status(&env.guardian.pubkey(), &mint, Status::Active)],
        &[&env.guardian],
    );
    assert_err(res, CorridorError::GuardianCannotResume);
}

#[test]
fn admin_reactivates_suspended_issuer() {
    let mut env = Env::new();
    let mint = env.registered_mint(&TOKEN_ID);
    send(&mut env.svm, &[ix_set_issuer_status(&env.guardian.pubkey(), &mint, Status::Suspended)], &[&env.guardian]).unwrap();

    send(&mut env.svm, &[ix_set_issuer_status(&env.admin.pubkey(), &mint, Status::Active)], &[&env.admin]).unwrap();

    assert_eq!(fetch_issuer(&env, &mint).status, Status::Active);
}

#[test]
fn compliance_cannot_change_issuer_status() {
    let mut env = Env::new();
    let mint = env.registered_mint(&TOKEN_ID);

    let res = send(
        &mut env.svm,
        &[ix_set_issuer_status(&env.compliance.pubkey(), &mint, Status::Suspended)],
        &[&env.compliance],
    );
    assert_err(res, CorridorError::Unauthorized);
}
