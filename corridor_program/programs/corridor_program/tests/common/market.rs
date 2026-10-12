//! A ready-to-trade venue for settle_quote tests: USDC (classic SPL) and a
//! local stablecoin (Token-2022) registered, a taker, a maker quoting with a
//! dedicated hot key, a payout partner, funded token accounts, and the maker's
//! SPL approvals to its own delegate PDA.

use {
    super::*,
    anchor_lang::solana_program::{instruction::Instruction, program_pack::Pack},
    anchor_spl::{
        token::ID as TOKEN_ID,
        token_2022::{
            spl_token_2022::{self, extension::StateWithExtensions},
            ID as TOKEN_2022_ID,
        },
    },
    corridor_program::{
        quote::{quote_message, Direction},
        state::Role,
        BPS_DENOMINATOR,
    },
    std::collections::HashMap,
};

pub const USDC_DECIMALS: u8 = 6;
/// Deliberately different from USDC so a decimals mix-up fails transfer_checked.
pub const LOCAL_DECIMALS: u8 = 2;
pub const STARTING_BALANCE: u64 = 1_000_000_000_000_000;
/// The default trade: 18,000.00 ZAR for 1,000 USDC.
pub const LOCAL_AMOUNT: u64 = 1_800_000;
pub const USDC_AMOUNT: u64 = 1_000_000_000;
pub const COMPLIANCE_HASH: [u8; 32] = [3u8; 32];
pub const QUOTE_TTL: i64 = 60;

pub fn delegate_pda(maker: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[corridor_program::DELEGATE_SEED, maker.as_ref()], &program_id()).0
}

pub fn receipt_pda(maker: &Pubkey, quote_id: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[corridor_program::RECEIPT_SEED, maker.as_ref(), &quote_id.to_le_bytes()],
        &program_id(),
    )
    .0
}

pub fn token_account_state(svm: &LiteSVM, address: &Pubkey) -> spl_token_2022::state::Account {
    let account = svm.get_account(address).expect("token account missing");
    StateWithExtensions::<spl_token_2022::state::Account>::unpack(&account.data)
        .unwrap()
        .base
}

/// Builds an Ed25519 precompile instruction the way a well-behaved client
/// does: one signature, all offsets pointing into this instruction's own data
/// (instruction index u16::MAX). Layout: header | pubkey | signature | message.
pub fn ed25519_ix(signer: &Keypair, message: &[u8]) -> Instruction {
    const PUBKEY_OFFSET: u16 = 16;
    const SIGNATURE_OFFSET: u16 = PUBKEY_OFFSET + 32;
    const MESSAGE_OFFSET: u16 = SIGNATURE_OFFSET + 64;
    let signature = signer.sign_message(message);

    let mut data = vec![1u8, 0];
    for field in [
        SIGNATURE_OFFSET,
        u16::MAX,
        PUBKEY_OFFSET,
        u16::MAX,
        MESSAGE_OFFSET,
        message.len() as u16,
        u16::MAX,
    ] {
        data.extend_from_slice(&field.to_le_bytes());
    }
    data.extend_from_slice(signer.pubkey().as_ref());
    data.extend_from_slice(signature.as_ref());
    data.extend_from_slice(message);
    Instruction { program_id: solana_sdk_ids::ed25519_program::ID, accounts: vec![], data }
}

#[derive(Clone, Debug)]
pub struct Quote {
    pub quote_id: u64,
    pub direction: Direction,
    pub amount_in: u64,
    pub amount_out: u64,
    pub expires_at: i64,
    pub min_amount_out: u64,
    pub compliance_hash: [u8; 32],
}

pub struct Market {
    pub env: Env,
    pub usdc_mint: Pubkey,
    pub local_mint: Pubkey,
    pub taker: Keypair,
    pub maker: Keypair,
    pub quote_key: Keypair,
    pub payout: Keypair,
    token_accounts: HashMap<(Pubkey, Pubkey), Pubkey>,
    next_quote_id: u64,
}

impl Market {
    pub fn new() -> Self {
        let mut env = Env::new();
        let usdc_mint = env.usdc_mint;
        let expires_at = now(&env.svm) + ATTESTATION_TTL;
        send(
            &mut env.svm,
            &[ix_register_issuer(&env.admin.pubkey(), &usdc_mint, expires_at, MAX_TRADE_AMOUNT, DAILY_CAP)],
            &[&env.admin],
        )
        .unwrap();
        let local_mint = create_mint(&mut env.svm, &env.admin, &TOKEN_2022_ID, &[], LOCAL_DECIMALS);
        send(
            &mut env.svm,
            &[ix_register_issuer(&env.admin.pubkey(), &local_mint, expires_at, MAX_TRADE_AMOUNT, DAILY_CAP)],
            &[&env.admin],
        )
        .unwrap();

        let taker = env.participant(Role::Taker);
        let maker = env.participant(Role::Maker);
        let payout = env.participant(Role::PayoutPartner);
        let quote_key = Keypair::new();
        send(
            &mut env.svm,
            &[ix_set_quote_signer(&maker.pubkey(), &maker.pubkey(), quote_key.pubkey())],
            &[&maker],
        )
        .unwrap();

        let treasury = env.treasury;
        let mut market = Self {
            env,
            usdc_mint,
            local_mint,
            taker,
            maker,
            quote_key,
            payout,
            token_accounts: HashMap::new(),
            next_quote_id: 1,
        };
        for owner in [market.taker.pubkey(), market.maker.pubkey(), market.payout.pubkey(), treasury] {
            for mint in [usdc_mint, local_mint] {
                market.open_account(&owner, &mint);
            }
        }
        for owner in [market.taker.pubkey(), market.maker.pubkey()] {
            for mint in [usdc_mint, local_mint] {
                market.fund(&owner, &mint, STARTING_BALANCE);
            }
        }
        let maker = market.maker.insecure_clone();
        for mint in [usdc_mint, local_mint] {
            market.approve(&maker, &mint, &delegate_pda(&maker.pubkey()), u64::MAX);
        }
        market
    }

    // Token plumbing

    pub fn token_program(&self, mint: &Pubkey) -> Pubkey {
        if *mint == self.usdc_mint { TOKEN_ID } else { TOKEN_2022_ID }
    }

    pub fn decimals(&self, mint: &Pubkey) -> u8 {
        if *mint == self.usdc_mint { USDC_DECIMALS } else { LOCAL_DECIMALS }
    }

    /// Creates (and remembers) a token account for `owner` on `mint`.
    pub fn open_account(&mut self, owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        let account = Keypair::new();
        let token_program = self.token_program(mint);
        let space = spl_token_2022::state::Account::LEN;
        let payer = self.env.admin.insecure_clone();
        let ixs = [
            system_instruction::create_account(
                &payer.pubkey(),
                &account.pubkey(),
                self.env.svm.minimum_balance_for_rent_exemption(space),
                space as u64,
                &token_program,
            ),
            spl_token_2022::instruction::initialize_account3(&token_program, &account.pubkey(), mint, owner).unwrap(),
        ];
        send(&mut self.env.svm, &ixs, &[&payer, &account]).unwrap();
        self.token_accounts.insert((*owner, *mint), account.pubkey());
        account.pubkey()
    }

    pub fn token_account(&self, owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        self.token_accounts[&(*owner, *mint)]
    }

    pub fn fund(&mut self, owner: &Pubkey, mint: &Pubkey, amount: u64) {
        let ix = spl_token_2022::instruction::mint_to(
            &self.token_program(mint),
            mint,
            &self.token_account(owner, mint),
            &self.env.admin.pubkey(),
            &[],
            amount,
        )
        .unwrap();
        let admin = self.env.admin.insecure_clone();
        send(&mut self.env.svm, &[ix], &[&admin]).unwrap();
    }

    pub fn approve(&mut self, owner: &Keypair, mint: &Pubkey, delegate: &Pubkey, amount: u64) {
        let ix = spl_token_2022::instruction::approve_checked(
            &self.token_program(mint),
            &self.token_account(&owner.pubkey(), mint),
            mint,
            delegate,
            &owner.pubkey(),
            &[],
            amount,
            self.decimals(mint),
        )
        .unwrap();
        send(&mut self.env.svm, &[ix], &[owner]).unwrap();
    }

    pub fn balance(&self, owner: &Pubkey, mint: &Pubkey) -> u64 {
        token_account_state(&self.env.svm, &self.token_account(owner, mint)).amount
    }

    // Quotes

    /// (mint_in, mint_out) for a direction: in is what the taker pays.
    pub fn mints(&self, direction: Direction) -> (Pubkey, Pubkey) {
        match direction {
            Direction::LocalToUsdc => (self.local_mint, self.usdc_mint),
            Direction::UsdcToLocal => (self.usdc_mint, self.local_mint),
        }
    }

    /// A fresh default quote expiring in QUOTE_TTL seconds, with
    /// min_amount_out set to exactly what the current fee leaves.
    pub fn quote(&mut self, direction: Direction) -> Quote {
        let (amount_in, amount_out) = match direction {
            Direction::LocalToUsdc => (LOCAL_AMOUNT, USDC_AMOUNT),
            Direction::UsdcToLocal => (USDC_AMOUNT, LOCAL_AMOUNT),
        };
        let quote_id = self.next_quote_id;
        self.next_quote_id += 1;
        Quote {
            quote_id,
            direction,
            amount_in,
            amount_out,
            expires_at: now(&self.env.svm) + QUOTE_TTL,
            min_amount_out: amount_out - self.fee(amount_out),
            compliance_hash: COMPLIANCE_HASH,
        }
    }

    pub fn fee(&self, amount_out: u64) -> u64 {
        amount_out * u64::from(fetch_config(&self.env.svm).fee_bps) / BPS_DENOMINATOR
    }

    pub fn message(&self, quote: &Quote, maker: &Pubkey, taker: &Pubkey) -> Vec<u8> {
        let (mint_in, mint_out) = self.mints(quote.direction);
        quote_message(
            &GENESIS_HASH,
            quote.quote_id,
            maker,
            taker,
            &mint_in,
            &mint_out,
            quote.amount_in,
            quote.amount_out,
            quote.expires_at,
        )
    }

    /// The default quote signature: the market maker's quote key over a
    /// message naming the market maker and taker.
    pub fn signature_ix(&self, quote: &Quote) -> Instruction {
        ed25519_ix(&self.quote_key, &self.message(quote, &self.maker.pubkey(), &self.taker.pubkey()))
    }

    /// Accounts for a correct settlement; tests mutate fields to attack it.
    pub fn settle_accounts(
        &self,
        quote: &Quote,
        maker: &Pubkey,
        taker: &Pubkey,
        recipient: &Pubkey,
    ) -> corridor_program::accounts::SettleQuote {
        let (mint_in, mint_out) = self.mints(quote.direction);
        corridor_program::accounts::SettleQuote {
            taker: *taker,
            config: config_pda(),
            taker_participant: participant_pda(taker),
            maker_participant: participant_pda(maker),
            recipient_participant: participant_pda(recipient),
            usdc_mint: self.usdc_mint,
            local_mint: self.local_mint,
            usdc_issuer: issuer_pda(&self.usdc_mint),
            local_issuer: issuer_pda(&self.local_mint),
            taker_ata_in: self.token_account(taker, &mint_in),
            maker_ata_in: self.token_account(maker, &mint_in),
            maker_ata_out: self.token_account(maker, &mint_out),
            recipient_ata_out: self.token_account(recipient, &mint_out),
            treasury_ata_out: self.token_account(&self.env.treasury, &mint_out),
            delegate_authority: delegate_pda(maker),
            quote_receipt: receipt_pda(maker, quote.quote_id),
            instructions_sysvar: solana_sdk_ids::sysvar::instructions::ID,
            usdc_token_program: TOKEN_ID,
            local_token_program: TOKEN_2022_ID,
            system_program: system_program::ID,
            event_authority: event_authority(),
            program: program_id(),
        }
    }

    pub fn default_accounts(&self, quote: &Quote) -> corridor_program::accounts::SettleQuote {
        self.settle_accounts(quote, &self.maker.pubkey(), &self.taker.pubkey(), &self.payout.pubkey())
    }

    pub fn settle_ix(&self, quote: &Quote, accounts: corridor_program::accounts::SettleQuote) -> Instruction {
        Instruction::new_with_bytes(
            program_id(),
            &corridor_program::instruction::SettleQuote {
                quote_id: quote.quote_id,
                direction: quote.direction,
                amount_in: quote.amount_in,
                amount_out: quote.amount_out,
                expires_at: quote.expires_at,
                min_amount_out: quote.min_amount_out,
                compliance_hash: quote.compliance_hash,
            }
            .data(),
            accounts.to_account_metas(None),
        )
    }

    /// Sends `ixs` signed (and paid) by the market taker.
    pub fn send_as_taker(&mut self, ixs: &[Instruction]) -> TransactionResult {
        let taker = self.taker.insecure_clone();
        send(&mut self.env.svm, ixs, &[&taker])
    }

    /// The happy path: signature + settlement, delivered to the payout partner.
    pub fn settle(&mut self, quote: &Quote) -> TransactionResult {
        let ixs = [self.signature_ix(quote), self.settle_ix(quote, self.default_accounts(quote))];
        self.send_as_taker(&ixs)
    }

    /// Runs `settle` and asserts a specific error at the settle instruction
    /// (index 1, after the Ed25519 instruction).
    pub fn assert_settle_err(&mut self, ixs: &[Instruction], expected: CorridorError) {
        let res = self.send_as_taker(ixs);
        assert_err_at(res, ixs.len() as u8 - 1, expected);
    }
}
