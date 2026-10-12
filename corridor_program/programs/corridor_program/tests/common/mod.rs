#![allow(dead_code)]

pub mod market;

use {
    anchor_lang::{
        error::ERROR_CODE_OFFSET,
        event::EVENT_IX_TAG_LE,
        prelude::{Clock, Pubkey},
        solana_program::{
            bpf_loader_upgradeable, instruction::Instruction, system_instruction,
            system_program,
        },
        AccountDeserialize, AnchorDeserialize, Discriminator, InstructionData, ToAccountMetas,
    },
    anchor_spl::token_2022::spl_token_2022::{
        self,
        extension::{metadata_pointer, transfer_fee, transfer_hook, ExtensionType},
    },
    corridor_program::{
        error::CorridorError,
        instructions::InitializeConfigArgs,
        state::{Config, Role, Status},
    },
    litesvm::{
        types::{TransactionMetadata, TransactionResult},
        LiteSVM,
    },
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::{versioned::VersionedTransaction, InstructionError},
    solana_transaction_error::TransactionError,
};

pub const DEFAULT_FEE_BPS: u16 = 10;
pub const DEFAULT_RETENTION_SECS: u64 = 3_600;
pub const GENESIS_HASH: [u8; 32] = [7u8; 32];
/// 2026-09-21T00:00:00Z; tests run at a realistic wall-clock time.
pub const START_TIME: i64 = 1_790_000_000;
pub const ATTESTATION_HASH: [u8; 32] = [1u8; 32];
pub const ATTESTATION_TTL: i64 = 30 * 86_400;
pub const MAX_TRADE_AMOUNT: u64 = 1_000_000_000_000;
pub const DAILY_CAP: u64 = 10_000_000_000_000;
pub const KYB_HASH: [u8; 32] = [2u8; 32];
/// USDC has 6 decimals: 100k and 1m USDC.
pub const MAX_TRADE_USDC: u64 = 100_000_000_000;
pub const DAILY_LIMIT_USDC: u64 = 1_000_000_000_000;

pub fn program_id() -> Pubkey {
    corridor_program::id()
}

pub fn config_pda() -> Pubkey {
    Pubkey::find_program_address(&[corridor_program::CONFIG_SEED], &program_id()).0
}

pub fn event_authority() -> Pubkey {
    Pubkey::find_program_address(&[b"__event_authority"], &program_id()).0
}

pub fn program_data_address() -> Pubkey {
    Pubkey::find_program_address(&[program_id().as_ref()], &bpf_loader_upgradeable::ID).0
}

/// Deploys the program and sets its upgrade authority. LiteSVM's add_program
/// creates ProgramData with no authority, so the metadata is patched in place.
/// Bincode layout: u32 variant (3) | u64 slot | u8 Option tag | [u8; 32] key.
pub fn new_svm(upgrade_authority: Option<Pubkey>) -> LiteSVM {
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/corridor_program.so"
    ));
    svm.add_program(program_id(), bytes).unwrap();

    let address = program_data_address();
    let mut account = svm.get_account(&address).unwrap();
    match upgrade_authority {
        Some(key) => {
            account.data[12] = 1;
            account.data[13..45].copy_from_slice(key.as_ref());
        }
        None => account.data[12] = 0,
    }
    svm.set_account(address, account).unwrap();
    svm
}

pub fn set_time(svm: &mut LiteSVM, unix_timestamp: i64) {
    let mut clock = svm.get_sysvar::<Clock>();
    clock.unix_timestamp = unix_timestamp;
    svm.set_sysvar(&clock);
}

pub fn now(svm: &LiteSVM) -> i64 {
    svm.get_sysvar::<Clock>().unix_timestamp
}

pub fn funded(svm: &mut LiteSVM) -> Keypair {
    let keypair = Keypair::new();
    svm.airdrop(&keypair.pubkey(), 10_000_000_000).unwrap();
    keypair
}

/// Signs with every signer; the first one pays. The blockhash is expired after
/// each send so identical instructions can be resubmitted in one test.
pub fn send(svm: &mut LiteSVM, ixs: &[Instruction], signers: &[&Keypair]) -> TransactionResult {
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(ixs, Some(&signers[0].pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
    let res = svm.send_transaction(tx);
    svm.expire_blockhash();
    res
}

pub fn assert_err(res: TransactionResult, expected: CorridorError) {
    assert_err_at(res, 0, expected);
}

/// Asserts that instruction `index` of the transaction failed with `expected`.
pub fn assert_err_at(res: TransactionResult, index: u8, expected: CorridorError) {
    let code = ERROR_CODE_OFFSET + expected as u32;
    match res {
        Err(failed) => assert_eq!(
            failed.err,
            TransactionError::InstructionError(index, InstructionError::Custom(code)),
            "logs: {:#?}",
            failed.meta.logs
        ),
        Ok(_) => panic!("expected {expected:?}, transaction succeeded"),
    }
}

pub fn fetch<T: AccountDeserialize>(svm: &LiteSVM, address: &Pubkey) -> T {
    let account = svm.get_account(address).expect("account missing");
    T::try_deserialize(&mut account.data.as_slice()).unwrap()
}

pub fn fetch_config(svm: &LiteSVM) -> Config {
    fetch(svm, &config_pda())
}

/// Finds the first emit_cpi! event of type E in a successful transaction.
/// Self-CPI event data is EVENT_IX_TAG_LE | event discriminator | borsh body.
pub fn find_event<E: AnchorDeserialize + Discriminator>(meta: &TransactionMetadata) -> Option<E> {
    let prefix: Vec<u8> = [EVENT_IX_TAG_LE, E::DISCRIMINATOR].concat();
    meta.inner_instructions
        .iter()
        .flatten()
        .map(|inner| &inner.instruction.data)
        .find(|data| data.starts_with(&prefix))
        .map(|data| E::try_from_slice(&data[prefix.len()..]).unwrap())
}

// Mints

#[derive(Clone, Copy, Debug)]
pub enum MintExtension {
    TransferFee,
    TransferHook,
    PermanentDelegate,
    MintCloseAuthority,
    MetadataPointer,
}

/// Creates a mint through the real token program: classic SPL Token when
/// `token_program` is spl_token's ID, otherwise Token-2022 with `extensions`.
pub fn create_mint(
    svm: &mut LiteSVM,
    payer: &Keypair,
    token_program: &Pubkey,
    extensions: &[MintExtension],
    decimals: u8,
) -> Pubkey {
    let mint = Keypair::new();
    let authority = payer.pubkey();
    let types: Vec<ExtensionType> = extensions
        .iter()
        .map(|extension| match extension {
            MintExtension::TransferFee => ExtensionType::TransferFeeConfig,
            MintExtension::TransferHook => ExtensionType::TransferHook,
            MintExtension::PermanentDelegate => ExtensionType::PermanentDelegate,
            MintExtension::MintCloseAuthority => ExtensionType::MintCloseAuthority,
            MintExtension::MetadataPointer => ExtensionType::MetadataPointer,
        })
        .collect();
    let space =
        ExtensionType::try_calculate_account_len::<spl_token_2022::state::Mint>(&types).unwrap();

    let mut ixs = vec![system_instruction::create_account(
        &authority,
        &mint.pubkey(),
        svm.minimum_balance_for_rent_exemption(space),
        space as u64,
        token_program,
    )];
    for extension in extensions {
        ixs.push(
            match extension {
                MintExtension::TransferFee => transfer_fee::instruction::initialize_transfer_fee_config(
                    token_program, &mint.pubkey(), Some(&authority), Some(&authority), 100, u64::MAX,
                ),
                MintExtension::TransferHook => transfer_hook::instruction::initialize(
                    token_program, &mint.pubkey(), Some(authority), Some(Pubkey::new_unique()),
                ),
                MintExtension::PermanentDelegate => spl_token_2022::instruction::initialize_permanent_delegate(
                    token_program, &mint.pubkey(), &authority,
                ),
                MintExtension::MintCloseAuthority => spl_token_2022::instruction::initialize_mint_close_authority(
                    token_program, &mint.pubkey(), Some(&authority),
                ),
                MintExtension::MetadataPointer => metadata_pointer::instruction::initialize(
                    token_program, &mint.pubkey(), Some(authority), Some(mint.pubkey()),
                ),
            }
            .unwrap(),
        );
    }
    ixs.push(
        spl_token_2022::instruction::initialize_mint2(token_program, &mint.pubkey(), &authority, Some(&authority), decimals)
            .unwrap(),
    );
    send(svm, &ixs, &[payer, &mint]).unwrap();
    mint.pubkey()
}

pub fn issuer_pda(mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[corridor_program::ISSUER_SEED, mint.as_ref()], &program_id()).0
}

pub fn participant_pda(wallet: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[corridor_program::PARTICIPANT_SEED, wallet.as_ref()], &program_id()).0
}

// Instruction builders

pub fn default_config_args(admin: &Keypair, compliance: &Keypair, guardian: &Keypair) -> InitializeConfigArgs {
    InitializeConfigArgs {
        admin: admin.pubkey(),
        compliance_authority: compliance.pubkey(),
        guardian: guardian.pubkey(),
        usdc_mint: Pubkey::new_unique(),
        fee_bps: DEFAULT_FEE_BPS,
        treasury_owner: Pubkey::new_unique(),
        receipt_retention_secs: DEFAULT_RETENTION_SECS,
        genesis_hash: GENESIS_HASH,
    }
}

pub fn ix_initialize_config(deployer: &Pubkey, args: InitializeConfigArgs) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::InitializeConfig { args }.data(),
        corridor_program::accounts::InitializeConfig {
            deployer: *deployer,
            config: config_pda(),
            program_data: program_data_address(),
            system_program: system_program::ID,
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

pub fn ix_update_params(
    admin: &Pubkey,
    fee_bps: Option<u16>,
    treasury_owner: Option<Pubkey>,
    receipt_retention_secs: Option<u64>,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::UpdateParams {
            fee_bps,
            treasury_owner,
            receipt_retention_secs,
        }
        .data(),
        corridor_program::accounts::UpdateParams {
            admin: *admin,
            config: config_pda(),
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

pub fn ix_set_operational_authorities(
    admin: &Pubkey,
    compliance_authority: Option<Pubkey>,
    guardian: Option<Pubkey>,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::SetOperationalAuthorities {
            compliance_authority,
            guardian,
        }
        .data(),
        corridor_program::accounts::SetOperationalAuthorities {
            admin: *admin,
            config: config_pda(),
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

pub fn ix_propose_admin(admin: &Pubkey, new_admin: Option<Pubkey>) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::ProposeAdmin { new_admin }.data(),
        corridor_program::accounts::ProposeAdmin {
            admin: *admin,
            config: config_pda(),
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

pub fn ix_accept_admin(new_admin: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::AcceptAdmin {}.data(),
        corridor_program::accounts::AcceptAdmin {
            new_admin: *new_admin,
            config: config_pda(),
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

pub fn ix_set_paused(authority: &Pubkey, paused: bool) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::SetPaused { paused }.data(),
        corridor_program::accounts::SetPaused {
            authority: *authority,
            config: config_pda(),
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

pub fn ix_register_issuer(
    admin: &Pubkey,
    mint: &Pubkey,
    attestation_expires_at: i64,
    max_trade_amount: u64,
    daily_cap: u64,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::RegisterIssuer {
            attestation_hash: ATTESTATION_HASH,
            attestation_expires_at,
            max_trade_amount,
            daily_cap,
        }
        .data(),
        corridor_program::accounts::RegisterIssuer {
            admin: *admin,
            config: config_pda(),
            mint: *mint,
            issuer_entry: issuer_pda(mint),
            system_program: system_program::ID,
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

pub fn ix_update_issuer_attestation(
    compliance: &Pubkey,
    mint: &Pubkey,
    attestation_hash: [u8; 32],
    attestation_expires_at: i64,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::UpdateIssuerAttestation {
            attestation_hash,
            attestation_expires_at,
        }
        .data(),
        corridor_program::accounts::UpdateIssuerAttestation {
            compliance: *compliance,
            config: config_pda(),
            issuer_entry: issuer_pda(mint),
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

pub fn ix_update_issuer_caps(
    admin: &Pubkey,
    mint: &Pubkey,
    max_trade_amount: Option<u64>,
    daily_cap: Option<u64>,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::UpdateIssuerCaps {
            max_trade_amount,
            daily_cap,
        }
        .data(),
        corridor_program::accounts::UpdateIssuerCaps {
            admin: *admin,
            config: config_pda(),
            issuer_entry: issuer_pda(mint),
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

pub fn ix_set_issuer_status(authority: &Pubkey, mint: &Pubkey, status: Status) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::SetIssuerStatus { status }.data(),
        corridor_program::accounts::SetIssuerStatus {
            authority: *authority,
            config: config_pda(),
            issuer_entry: issuer_pda(mint),
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

pub fn ix_register_participant(
    compliance: &Pubkey,
    wallet: &Pubkey,
    role: Role,
    max_trade_usdc: u64,
    daily_limit_usdc: u64,
    quote_signer: Option<Pubkey>,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::RegisterParticipant {
            wallet: *wallet,
            role,
            kyb_ref_hash: KYB_HASH,
            max_trade_usdc,
            daily_limit_usdc,
            quote_signer,
        }
        .data(),
        corridor_program::accounts::RegisterParticipant {
            compliance: *compliance,
            config: config_pda(),
            participant: participant_pda(wallet),
            system_program: system_program::ID,
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

pub fn ix_update_participant(
    compliance: &Pubkey,
    wallet: &Pubkey,
    kyb_ref_hash: Option<[u8; 32]>,
    max_trade_usdc: Option<u64>,
    daily_limit_usdc: Option<u64>,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::UpdateParticipant {
            kyb_ref_hash,
            max_trade_usdc,
            daily_limit_usdc,
        }
        .data(),
        corridor_program::accounts::UpdateParticipant {
            compliance: *compliance,
            config: config_pda(),
            participant: participant_pda(wallet),
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

pub fn ix_set_participant_status(authority: &Pubkey, wallet: &Pubkey, status: Status) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::SetParticipantStatus { status }.data(),
        corridor_program::accounts::SetParticipantStatus {
            authority: *authority,
            config: config_pda(),
            participant: participant_pda(wallet),
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

/// `participant_wallet` picks the Participant account; `signer` signs. They
/// differ only in tests that try to rotate someone else's key.
pub fn ix_set_quote_signer(signer: &Pubkey, participant_wallet: &Pubkey, new_quote_signer: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &corridor_program::instruction::SetQuoteSigner { new_quote_signer }.data(),
        corridor_program::accounts::SetQuoteSigner {
            maker: *signer,
            maker_participant: participant_pda(participant_wallet),
            event_authority: event_authority(),
            program: program_id(),
        }
        .to_account_metas(None),
    )
}

/// A deployed, initialised protocol with funded role keys.
pub struct Env {
    pub svm: LiteSVM,
    pub deployer: Keypair,
    pub admin: Keypair,
    pub compliance: Keypair,
    pub guardian: Keypair,
    /// A real classic-SPL mint with 6 decimals; config.usdc_mint points here.
    pub usdc_mint: Pubkey,
    pub treasury: Pubkey,
}

impl Env {
    pub fn new() -> Self {
        let deployer = Keypair::new();
        let mut svm = new_svm(Some(deployer.pubkey()));
        set_time(&mut svm, START_TIME);
        svm.airdrop(&deployer.pubkey(), 10_000_000_000).unwrap();
        let admin = funded(&mut svm);
        let compliance = funded(&mut svm);
        let guardian = funded(&mut svm);

        let usdc_mint = create_mint(&mut svm, &admin, &anchor_spl::token::ID, &[], 6);
        let treasury = Pubkey::new_unique();

        let mut args = default_config_args(&admin, &compliance, &guardian);
        args.usdc_mint = usdc_mint;
        args.treasury_owner = treasury;
        send(
            &mut svm,
            &[ix_initialize_config(&deployer.pubkey(), args)],
            &[&deployer],
        )
        .unwrap();

        Self { svm, deployer, admin, compliance, guardian, usdc_mint, treasury }
    }

    /// Funds a new wallet and registers it with default limits (zero for
    /// payout partners). Makers get the wallet as their quote signer.
    pub fn participant(&mut self, role: Role) -> Keypair {
        let wallet = funded(&mut self.svm);
        let (max_trade, daily_limit) = match role {
            Role::PayoutPartner => (0, 0),
            _ => (MAX_TRADE_USDC, DAILY_LIMIT_USDC),
        };
        send(
            &mut self.svm,
            &[ix_register_participant(&self.compliance.pubkey(), &wallet.pubkey(), role, max_trade, daily_limit, None)],
            &[&self.compliance],
        )
        .unwrap();
        wallet
    }

    /// Creates a mint and registers it with default caps and a 30-day attestation.
    pub fn registered_mint(&mut self, token_program: &Pubkey) -> Pubkey {
        let mint = create_mint(&mut self.svm, &self.admin, token_program, &[], 6);
        let expires_at = now(&self.svm) + ATTESTATION_TTL;
        send(
            &mut self.svm,
            &[ix_register_issuer(&self.admin.pubkey(), &mint, expires_at, MAX_TRADE_AMOUNT, DAILY_CAP)],
            &[&self.admin],
        )
        .unwrap();
        mint
    }
}
