mod common;

use {
    anchor_lang::prelude::Pubkey,
    common::*,
    corridor_program::{
        error::CorridorError,
        events::{AdminAccepted, ConfigInitialized, ParamsUpdated, PauseChanged},
        MAX_FEE_BPS,
    },
    solana_keypair::Keypair,
    solana_signer::Signer,
};

// UC-01 initialize_config

#[test]
fn initialize_config_sets_every_field_and_emits_event() {
    let deployer = Keypair::new();
    let mut svm = new_svm(Some(deployer.pubkey()));
    svm.airdrop(&deployer.pubkey(), 10_000_000_000).unwrap();
    let (admin, compliance, guardian) = (Keypair::new(), Keypair::new(), Keypair::new());
    let args = default_config_args(&admin, &compliance, &guardian);
    let (usdc_mint, treasury_owner) = (args.usdc_mint, args.treasury_owner);

    let meta = send(&mut svm, &[ix_initialize_config(&deployer.pubkey(), args)], &[&deployer]).unwrap();

    let config = fetch_config(&svm);
    assert_eq!(config.admin, admin.pubkey());
    assert_eq!(config.compliance_authority, compliance.pubkey());
    assert_eq!(config.guardian, guardian.pubkey());
    assert_eq!(config.usdc_mint, usdc_mint);
    assert_eq!(config.fee_bps, DEFAULT_FEE_BPS);
    assert_eq!(config.treasury_owner, treasury_owner);
    assert_eq!(config.receipt_retention_secs, DEFAULT_RETENTION_SECS);
    assert_eq!(config.genesis_hash, GENESIS_HASH);
    assert!(!config.paused);
    assert_eq!(config.pending_admin, None);

    let event: ConfigInitialized = find_event(&meta).expect("ConfigInitialized not emitted");
    assert_eq!(event.admin, admin.pubkey());
}

#[test]
fn initialize_config_rejects_non_upgrade_authority() {
    let deployer = Keypair::new();
    let mut svm = new_svm(Some(deployer.pubkey()));
    let attacker = funded(&mut svm);
    let args = default_config_args(&attacker, &attacker, &attacker);

    let res = send(&mut svm, &[ix_initialize_config(&attacker.pubkey(), args)], &[&attacker]);
    assert_err(res, CorridorError::Unauthorized);
}

#[test]
fn initialize_config_rejects_immutable_program() {
    let mut svm = new_svm(None);
    let caller = funded(&mut svm);
    let args = default_config_args(&caller, &caller, &caller);

    let res = send(&mut svm, &[ix_initialize_config(&caller.pubkey(), args)], &[&caller]);
    assert_err(res, CorridorError::Unauthorized);
}

#[test]
fn initialize_config_rejects_fee_above_max() {
    let deployer = Keypair::new();
    let mut svm = new_svm(Some(deployer.pubkey()));
    svm.airdrop(&deployer.pubkey(), 10_000_000_000).unwrap();
    let mut args = default_config_args(&deployer, &deployer, &deployer);
    args.fee_bps = MAX_FEE_BPS + 1;

    let res = send(&mut svm, &[ix_initialize_config(&deployer.pubkey(), args)], &[&deployer]);
    assert_err(res, CorridorError::FeeTooHigh);
}

#[test]
fn initialize_config_runs_only_once() {
    let mut env = Env::new();
    let args = default_config_args(&env.admin, &env.compliance, &env.guardian);

    let res = send(&mut env.svm, &[ix_initialize_config(&env.deployer.pubkey(), args)], &[&env.deployer]);
    assert!(res.is_err());
}

// Handler 2 update_params

#[test]
fn update_params_changes_only_given_fields() {
    let mut env = Env::new();
    let before = fetch_config(&env.svm);

    let meta = send(
        &mut env.svm,
        &[ix_update_params(&env.admin.pubkey(), Some(MAX_FEE_BPS), None, None)],
        &[&env.admin],
    )
    .unwrap();

    let after = fetch_config(&env.svm);
    assert_eq!(after.fee_bps, MAX_FEE_BPS);
    assert_eq!(after.treasury_owner, before.treasury_owner);
    assert_eq!(after.receipt_retention_secs, before.receipt_retention_secs);

    let event: ParamsUpdated = find_event(&meta).unwrap();
    assert_eq!((event.old_fee_bps, event.new_fee_bps), (DEFAULT_FEE_BPS, MAX_FEE_BPS));
}

#[test]
fn update_params_rejects_fee_above_max() {
    let mut env = Env::new();
    let res = send(
        &mut env.svm,
        &[ix_update_params(&env.admin.pubkey(), Some(MAX_FEE_BPS + 1), None, None)],
        &[&env.admin],
    );
    assert_err(res, CorridorError::FeeTooHigh);
}

#[test]
fn update_params_rejects_non_admin() {
    let mut env = Env::new();
    let res = send(
        &mut env.svm,
        &[ix_update_params(&env.guardian.pubkey(), Some(0), None, None)],
        &[&env.guardian],
    );
    assert_err(res, CorridorError::Unauthorized);
}

// Handler 3 set_operational_authorities

#[test]
fn set_operational_authorities_rotates_guardian_only() {
    let mut env = Env::new();
    let new_guardian = Pubkey::new_unique();

    send(
        &mut env.svm,
        &[ix_set_operational_authorities(&env.admin.pubkey(), None, Some(new_guardian))],
        &[&env.admin],
    )
    .unwrap();

    let config = fetch_config(&env.svm);
    assert_eq!(config.guardian, new_guardian);
    assert_eq!(config.compliance_authority, env.compliance.pubkey());
}

#[test]
fn set_operational_authorities_rejects_compliance_authority() {
    let mut env = Env::new();
    let res = send(
        &mut env.svm,
        &[ix_set_operational_authorities(&env.compliance.pubkey(), Some(env.compliance.pubkey()), None)],
        &[&env.compliance],
    );
    assert_err(res, CorridorError::Unauthorized);
}

// Handlers 4-5 propose_admin / accept_admin

#[test]
fn admin_transfer_takes_effect_only_on_accept() {
    let mut env = Env::new();
    let new_admin = funded(&mut env.svm);

    send(&mut env.svm, &[ix_propose_admin(&env.admin.pubkey(), Some(new_admin.pubkey()))], &[&env.admin]).unwrap();
    let config = fetch_config(&env.svm);
    assert_eq!(config.admin, env.admin.pubkey(), "admin must not change on propose");
    assert_eq!(config.pending_admin, Some(new_admin.pubkey()));

    let meta = send(&mut env.svm, &[ix_accept_admin(&new_admin.pubkey())], &[&new_admin]).unwrap();
    let config = fetch_config(&env.svm);
    assert_eq!(config.admin, new_admin.pubkey());
    assert_eq!(config.pending_admin, None);
    let event: AdminAccepted = find_event(&meta).unwrap();
    assert_eq!(event.old_admin, env.admin.pubkey());

    // The old admin has lost its powers.
    let res = send(&mut env.svm, &[ix_update_params(&env.admin.pubkey(), Some(0), None, None)], &[&env.admin]);
    assert_err(res, CorridorError::Unauthorized);
}

#[test]
fn accept_admin_rejects_anyone_but_pending_admin() {
    let mut env = Env::new();
    let new_admin = Pubkey::new_unique();
    let intruder = funded(&mut env.svm);
    send(&mut env.svm, &[ix_propose_admin(&env.admin.pubkey(), Some(new_admin))], &[&env.admin]).unwrap();

    let res = send(&mut env.svm, &[ix_accept_admin(&intruder.pubkey())], &[&intruder]);
    assert_err(res, CorridorError::Unauthorized);
}

#[test]
fn accept_admin_fails_with_nothing_pending() {
    let mut env = Env::new();
    let candidate = funded(&mut env.svm);
    let res = send(&mut env.svm, &[ix_accept_admin(&candidate.pubkey())], &[&candidate]);
    assert_err(res, CorridorError::NoPendingAdmin);
}

#[test]
fn propose_admin_none_cancels_pending() {
    let mut env = Env::new();
    let candidate = funded(&mut env.svm);
    send(&mut env.svm, &[ix_propose_admin(&env.admin.pubkey(), Some(candidate.pubkey()))], &[&env.admin]).unwrap();
    send(&mut env.svm, &[ix_propose_admin(&env.admin.pubkey(), None)], &[&env.admin]).unwrap();

    let res = send(&mut env.svm, &[ix_accept_admin(&candidate.pubkey())], &[&candidate]);
    assert_err(res, CorridorError::NoPendingAdmin);
}

#[test]
fn propose_admin_rejects_non_admin() {
    let mut env = Env::new();
    let res = send(
        &mut env.svm,
        &[ix_propose_admin(&env.guardian.pubkey(), Some(env.guardian.pubkey()))],
        &[&env.guardian],
    );
    assert_err(res, CorridorError::Unauthorized);
}

// UC-03 set_paused

#[test]
fn guardian_can_pause_but_not_unpause() {
    let mut env = Env::new();

    let meta = send(&mut env.svm, &[ix_set_paused(&env.guardian.pubkey(), true)], &[&env.guardian]).unwrap();
    assert!(fetch_config(&env.svm).paused);
    let event: PauseChanged = find_event(&meta).unwrap();
    assert!(event.paused);
    assert_eq!(event.by, env.guardian.pubkey());

    let res = send(&mut env.svm, &[ix_set_paused(&env.guardian.pubkey(), false)], &[&env.guardian]);
    assert_err(res, CorridorError::GuardianCannotResume);
    assert!(fetch_config(&env.svm).paused);
}

#[test]
fn admin_can_pause_and_unpause() {
    let mut env = Env::new();
    send(&mut env.svm, &[ix_set_paused(&env.admin.pubkey(), true)], &[&env.admin]).unwrap();
    send(&mut env.svm, &[ix_set_paused(&env.admin.pubkey(), false)], &[&env.admin]).unwrap();
    assert!(!fetch_config(&env.svm).paused);
}

#[test]
fn compliance_authority_cannot_pause() {
    let mut env = Env::new();
    let res = send(&mut env.svm, &[ix_set_paused(&env.compliance.pubkey(), true)], &[&env.compliance]);
    assert_err(res, CorridorError::Unauthorized);
}

#[test]
fn admin_handlers_still_work_while_paused() {
    let mut env = Env::new();
    send(&mut env.svm, &[ix_set_paused(&env.guardian.pubkey(), true)], &[&env.guardian]).unwrap();
    send(&mut env.svm, &[ix_update_params(&env.admin.pubkey(), Some(0), None, None)], &[&env.admin]).unwrap();
    assert_eq!(fetch_config(&env.svm).fee_bps, 0);
}
