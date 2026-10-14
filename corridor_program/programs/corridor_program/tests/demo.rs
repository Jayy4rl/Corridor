// LiteSVM's FailedTransactionMetadata is large; the helpers return it as-is.
#![allow(clippy::result_large_err)]

//! Narrated demo of the settlement program, scene by scene.
//!
//!   cargo test --test demo -- --nocapture
//!   DEMO_PAUSE=3 cargo test --test demo -- --nocapture   # 3 s between scenes
//!
//! Runs against LiteSVM with the real SPL Token, Token-2022 and Ed25519
//! programs. Every claim printed is also asserted, so the demo cannot narrate
//! something the program did not do.

mod common;

use {
    anchor_lang::{error::ERROR_CODE_OFFSET, prelude::Pubkey, solana_program::instruction::Instruction},
    common::{market::*, *},
    corridor_program::{
        error::CorridorError,
        events::QuoteSettled,
        quote::Direction,
        state::{IssuerEntry, Participant, Role, Status},
    },
    litesvm::types::TransactionResult,
    solana_keypair::Keypair,
    solana_signer::Signer,
    solana_transaction::InstructionError,
    solana_transaction_error::TransactionError,
    std::{thread, time::Duration},
};

/// Starting inventory for the operator and the maker.
const DEMO_USDC: u64 = 50_000_000_000; // 50,000 USDC (6 decimals)
const DEMO_ZARSC: u64 = 100_000_000; // 1,000,000.00 ZARSC (2 decimals)

// ---- Presentation helpers ----

fn scene(number: u32, title: &str) {
    if let Some(seconds) = std::env::var("DEMO_PAUSE").ok().and_then(|s| s.parse().ok()) {
        thread::sleep(Duration::from_secs(seconds));
    }
    println!("\n━━━ SCENE {number} · {title} ━━━\n");
}

fn say(line: &str) {
    println!("  {line}");
}

/// 1800000 with 2 decimals -> "18,000.00".
fn amount(value: u64, decimals: u8) -> String {
    let scale = 10u64.pow(u32::from(decimals));
    let whole = (value / scale).to_string();
    let mut grouped = String::new();
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if decimals == 0 {
        return grouped;
    }
    let mut fraction = format!("{:0width$}", value % scale, width = usize::from(decimals));
    while fraction.len() > 2 && fraction.ends_with('0') {
        fraction.pop();
    }
    format!("{grouped}.{fraction}")
}

fn change(before: u64, after: u64, decimals: u8) -> String {
    match after.cmp(&before) {
        std::cmp::Ordering::Greater => format!("+{}", amount(after - before, decimals)),
        std::cmp::Ordering::Less => format!("-{}", amount(before - after, decimals)),
        std::cmp::Ordering::Equal => "·".into(),
    }
}

fn short(key: &Pubkey) -> String {
    let s = key.to_string();
    format!("{}…{}", &s[..4], &s[s.len() - 4..])
}

// ---- Balances shown in the tables ----

struct Line {
    label: &'static str,
    owner: Pubkey,
    mint: Pubkey,
    decimals: u8,
    symbol: &'static str,
}

fn lines(m: &Market) -> Vec<Line> {
    let (zar, usdc) = (m.local_mint, m.usdc_mint);
    vec![
        Line { label: "Operator (taker)", owner: m.taker.pubkey(), mint: zar, decimals: LOCAL_DECIMALS, symbol: "ZARSC" },
        Line { label: "Operator (taker)", owner: m.taker.pubkey(), mint: usdc, decimals: USDC_DECIMALS, symbol: "USDC" },
        Line { label: "Market maker", owner: m.maker.pubkey(), mint: zar, decimals: LOCAL_DECIMALS, symbol: "ZARSC" },
        Line { label: "Market maker", owner: m.maker.pubkey(), mint: usdc, decimals: USDC_DECIMALS, symbol: "USDC" },
        Line { label: "Payout partner", owner: m.payout.pubkey(), mint: usdc, decimals: USDC_DECIMALS, symbol: "USDC" },
        Line { label: "Corridor treasury", owner: m.env.treasury, mint: usdc, decimals: USDC_DECIMALS, symbol: "USDC" },
    ]
}

fn snapshot(m: &Market) -> Vec<u64> {
    lines(m).iter().map(|l| m.balance(&l.owner, &l.mint)).collect()
}

fn print_table(m: &Market, before: &[u64], after: &[u64]) {
    say(&format!("{:<20} {:<6} {:>14} {:>14} {:>12}", "Account", "Token", "Before", "After", "Change"));
    for (i, l) in lines(m).iter().enumerate() {
        say(&format!(
            "{:<20} {:<6} {:>14} {:>14} {:>12}",
            l.label,
            l.symbol,
            amount(before[i], l.decimals),
            amount(after[i], l.decimals),
            change(before[i], after[i], l.decimals)
        ));
    }
}

// ---- Refusals: asserted, then narrated ----

enum Expect {
    Program(CorridorError),
    /// The System Program refuses to create a receipt that already exists.
    ReceiptExists,
}

/// Sends `ixs`, asserts the transaction was refused with `expected` and that
/// no tracked balance moved, then prints one line.
fn refuse(m: &mut Market, attempt: &str, ixs: &[Instruction], signer: &Keypair, expected: Expect) {
    let before = snapshot(m);
    let res: TransactionResult = send(&mut m.env.svm, ixs, &[signer]);
    let code = match &res {
        Err(failed) => match &failed.err {
            TransactionError::InstructionError(_, InstructionError::Custom(code)) => *code,
            other => panic!("{attempt}: unexpected failure {other:?}"),
        },
        Ok(_) => panic!("{attempt}: the program ACCEPTED this; the demo would be lying"),
    };
    let reason = match expected {
        Expect::Program(error) => {
            assert_eq!(code, ERROR_CODE_OFFSET + error as u32, "{attempt}: wrong error");
            format!("{error:?}")
        }
        Expect::ReceiptExists => {
            assert_eq!(code, 0, "{attempt}: expected the receipt to block it");
            "receipt already exists".into()
        }
    };
    assert_eq!(snapshot(m), before, "{attempt}: balances moved on a refused trade");
    say(&format!("✗ {attempt:<52} → refused: {reason}"));
}

/// A second maker with its own key, inventory and approvals (for the F9 scene).
fn add_maker(m: &mut Market) -> Keypair {
    let maker = m.env.participant(Role::Maker);
    for mint in [m.usdc_mint, m.local_mint] {
        m.open_account(&maker.pubkey(), &mint);
        m.fund(&maker.pubkey(), &mint, if mint == m.usdc_mint { DEMO_USDC } else { DEMO_ZARSC });
        m.approve(&maker, &mint, &delegate_pda(&maker.pubkey()), u64::MAX);
    }
    maker
}

fn admin_send(m: &mut Market, ix: Instruction) {
    let admin = m.env.admin.insecure_clone();
    send(&mut m.env.svm, &[ix], &[&admin]).unwrap();
}

fn compliance_send(m: &mut Market, ix: Instruction) {
    let compliance = m.env.compliance.insecure_clone();
    send(&mut m.env.svm, &[ix], &[&compliance]).unwrap();
}

#[test]
fn narrated_demo() {
    println!("\n╔══════════════════════════════════════════════════════════════════════╗");
    println!("║  Corridor settlement program · narrated demo                         ║");
    println!("║  ZAR stablecoin ↔ USDC between KYB'd counterparties, atomically      ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    say("Runtime: LiteSVM with the real SPL Token, Token-2022 and Ed25519 programs.");

    // ---------------------------------------------------------------- 1
    scene(1, "Who and what may trade");
    let mut m = Market::with_balances(DEMO_USDC, DEMO_ZARSC);
    let config = fetch_config(&m.env.svm);
    say(&format!("✓ Protocol initialised by the upgrade authority: fee {} bps, {} min receipt retention",
        config.fee_bps, config.receipt_retention_secs / 60));
    say(&format!("  Three separate authorities: admin {}, compliance {}, guardian {} (can only stop things)",
        short(&config.admin), short(&config.compliance_authority), short(&config.guardian)));
    for (name, mint) in [("USDC ", m.usdc_mint), ("ZARSC", m.local_mint)] {
        let issuer: IssuerEntry = fetch(&m.env.svm, &issuer_pda(&mint));
        let program = if issuer.token_program == anchor_spl::token::ID { "SPL Token " } else { "Token-2022" };
        say(&format!("✓ {name} allowlisted: {program}, {} decimals, reserve attestation valid {} more days",
            issuer.decimals, (issuer.attestation_expires_at - now(&m.env.svm)) / 86_400));
    }
    for (name, wallet) in [("Operator      ", m.taker.pubkey()), ("Market maker  ", m.maker.pubkey()), ("Payout partner", m.payout.pubkey())] {
        let p: Participant = fetch(&m.env.svm, &participant_pda(&wallet));
        let limits = if p.role == Role::PayoutPartner {
            "receives only, no limits".to_string()
        } else {
            format!("max {} USDC per trade, {} USDC per day", amount(p.max_trade_usdc, 6), amount(p.daily_limit_usdc, 6))
        };
        say(&format!("✓ {name} {} admitted as {:?}: {limits}", short(&wallet), p.role));
    }
    let maker: Participant = fetch(&m.env.svm, &participant_pda(&m.maker.pubkey()));
    say(&format!("✓ Maker signs quotes with a separate hot key {}, not its inventory wallet", short(&maker.quote_signer)));
    say(&format!("✓ Maker approved its own delegate PDA {}: inventory never leaves its account", short(&delegate_pda(&m.maker.pubkey()))));
    say("  On-chain, only 32-byte KYB and attestation hashes are stored, never personal data.");

    // ---------------------------------------------------------------- 2
    scene(2, "A trade: 18,000.00 ZARSC → 1,000 USDC, paid straight to the payout partner");
    let q = m.quote(Direction::LocalToUsdc);
    say(&format!("1. The maker's quote key signs a 241-byte quote: id {}, expires in {} s,", q.quote_id, QUOTE_TTL));
    say("   bound to this program, this cluster, this maker, this operator, both mints and both amounts.");
    say("2. The operator sends ONE transaction: [Ed25519 signature check, settle_quote].");
    let before = snapshot(&m);
    let meta = m.settle(&q).unwrap();
    let after = snapshot(&m);
    say(&format!("✓ Settled atomically in {} compute units. All three transfers or none:\n", meta.compute_units_consumed));
    print_table(&m, &before, &after);
    let event: QuoteSettled = find_event(&meta).unwrap();
    say("");
    say(&format!("✓ QuoteSettled event for the indexer: quote {}, fee {} USDC, compliance hash {:02x}{:02x}…",
        event.quote_id, amount(event.fee, 6), event.compliance_hash[0], event.compliance_hash[1]));
    say(&format!("✓ Replay receipt created at {}, rent paid by the operator", short(&receipt_pda(&m.maker.pubkey(), q.quote_id))));

    // ---------------------------------------------------------------- 3
    scene(3, "What the program refuses (each attempt leaves every balance untouched)");
    let taker = m.taker.insecure_clone();

    let ixs = [m.signature_ix(&q), m.settle_ix(&q, m.default_accounts(&q))];
    refuse(&mut m, "Submit the same quote again (replay)", &ixs, &taker, Expect::ReceiptExists);

    let signed = m.quote(Direction::LocalToUsdc);
    let mut tampered = signed.clone();
    tampered.amount_out += 1_000_000;
    let ixs = [m.signature_ix(&signed), m.settle_ix(&tampered, m.default_accounts(&tampered))];
    refuse(&mut m, "Change the signed amount by 1 USDC", &ixs, &taker, Expect::Program(CorridorError::InvalidQuoteSignature));

    let other_operator = m.env.participant(Role::Taker);
    for mint in [m.usdc_mint, m.local_mint] {
        m.open_account(&other_operator.pubkey(), &mint);
        m.fund(&other_operator.pubkey(), &mint, if mint == m.usdc_mint { DEMO_USDC } else { DEMO_ZARSC });
    }
    let q3 = m.quote(Direction::LocalToUsdc);
    let accounts = m.settle_accounts(&q3, &m.maker.pubkey(), &other_operator.pubkey(), &other_operator.pubkey());
    let ixs = [m.signature_ix(&q3), m.settle_ix(&q3, accounts)];
    refuse(&mut m, "Another operator uses a quote not meant for it", &ixs, &other_operator, Expect::Program(CorridorError::InvalidQuoteSignature));

    let victim = add_maker(&mut m);
    let q4 = m.quote(Direction::LocalToUsdc);
    let mut accounts = m.default_accounts(&q4);
    accounts.maker_ata_out = m.token_account(&victim.pubkey(), &m.usdc_mint);
    let ixs = [m.signature_ix(&q4), m.settle_ix(&q4, accounts)];
    let victim_usdc = m.balance(&victim.pubkey(), &m.usdc_mint);
    refuse(&mut m, "Get paid from a different maker's inventory (F9)", &ixs, &taker, Expect::Program(CorridorError::AccountOwnerMismatch));
    assert_eq!(m.balance(&victim.pubkey(), &m.usdc_mint), victim_usdc);

    let mut q5 = m.quote(Direction::LocalToUsdc);
    q5.expires_at = now(&m.env.svm) + 600;
    let ixs = [m.signature_ix(&q5), m.settle_ix(&q5, m.default_accounts(&q5))];
    refuse(&mut m, "Use a quote valid for 10 minutes (cap is 120 s)", &ixs, &taker, Expect::Program(CorridorError::QuoteTtlTooLong));

    let q6 = m.quote(Direction::LocalToUsdc);
    set_time(&mut m.env.svm, q6.expires_at + 1);
    let ixs = [m.signature_ix(&q6), m.settle_ix(&q6, m.default_accounts(&q6))];
    refuse(&mut m, "Settle a quote one second after it expired", &ixs, &taker, Expect::Program(CorridorError::QuoteExpired));

    let compliance = m.env.compliance.pubkey();
    compliance_send(&mut m, ix_update_participant(&compliance, &taker.pubkey(), None, Some(500_000_000), None));
    let q7 = m.quote(Direction::LocalToUsdc);
    let ixs = [m.signature_ix(&q7), m.settle_ix(&q7, m.default_accounts(&q7))];
    refuse(&mut m, "Trade above the operator's 500 USDC compliance limit", &ixs, &taker, Expect::Program(CorridorError::TradeTooLarge));
    compliance_send(&mut m, ix_update_participant(&compliance, &taker.pubkey(), None, Some(MAX_TRADE_USDC), None));

    let q8 = m.quote(Direction::LocalToUsdc);
    let admin = m.env.admin.pubkey();
    admin_send(&mut m, ix_update_params(&admin, Some(50), None, None));
    let ixs = [m.signature_ix(&q8), m.settle_ix(&q8, m.default_accounts(&q8))];
    refuse(&mut m, "Fee raised after quoting would short the operator", &ixs, &taker, Expect::Program(CorridorError::SlippageExceeded));
    admin_send(&mut m, ix_update_params(&admin, Some(10), None, None));

    let rogue_ixs = [m.signature_ix(&q8), m.settle_ix(&q8, m.default_accounts(&q8))];
    let message = m.message(&q8, &m.maker.pubkey(), &taker.pubkey());
    let forged = [ed25519_ix(&m.maker, &message), rogue_ixs[1].clone()];
    refuse(&mut m, "Quote signed by the maker wallet, not its quote key", &forged, &taker, Expect::Program(CorridorError::InvalidQuoteSignature));

    // ---------------------------------------------------------------- 4
    scene(4, "Emergency stop");
    let guardian = m.env.guardian.insecure_clone();
    send(&mut m.env.svm, &[ix_set_paused(&guardian.pubkey(), true)], &[&guardian]).unwrap();
    say("✓ Guardian (monitoring service hot key) pauses the protocol");
    let q9 = m.quote(Direction::UsdcToLocal);
    let ixs = [m.signature_ix(&q9), m.settle_ix(&q9, m.default_accounts(&q9))];
    refuse(&mut m, "Settle while paused", &ixs, &taker, Expect::Program(CorridorError::Paused));
    let ixs = [ix_set_paused(&guardian.pubkey(), false)];
    refuse(&mut m, "Guardian tries to unpause", &ixs, &guardian, Expect::Program(CorridorError::GuardianCannotResume));
    admin_send(&mut m, ix_set_paused(&admin, false));
    say("✓ Only the admin (a Squads multisig on devnet) can resume");
    let before = snapshot(&m);
    let q10 = m.quote(Direction::UsdcToLocal);
    m.settle(&q10).unwrap();
    let after = snapshot(&m);
    say("✓ Trading resumes: the reverse direction, 1,000 USDC → 18,000.00 ZARSC, settles");
    assert_ne!(before, after);

    // ---------------------------------------------------------------- 5
    scene(5, "Housekeeping: rent comes back, replay stays impossible");
    let receipt = receipt_pda(&m.maker.pubkey(), q.quote_id);
    let rent = m.env.svm.get_balance(&receipt).unwrap();
    set_time(&mut m.env.svm, q.expires_at + DEFAULT_RETENTION_SECS as i64 + 1);
    let crank = funded(&mut m.env.svm);
    let operator_sol = m.env.svm.get_balance(&taker.pubkey()).unwrap();
    send(&mut m.env.svm, &[ix_close_receipt(&crank.pubkey(), &receipt, &taker.pubkey())], &[&crank]).unwrap();
    assert_eq!(m.env.svm.get_balance(&taker.pubkey()).unwrap(), operator_sol + rent);
    say(&format!("✓ After the retention hour, Corridor's crank closes the first receipt; the operator gets {} SOL back", amount(rent, 9)));
    let ixs = [m.signature_ix(&q), m.settle_ix(&q, m.default_accounts(&q))];
    refuse(&mut m, "Replay the original quote once its receipt is gone", &ixs, &taker, Expect::Program(CorridorError::QuoteExpired));

    let suspended = m.payout.pubkey();
    let guardian_key = guardian.pubkey();
    send(&mut m.env.svm, &[ix_set_participant_status(&guardian_key, &suspended, Status::Suspended)], &[&guardian]).unwrap();
    let q11 = m.quote(Direction::LocalToUsdc);
    let ixs = [m.signature_ix(&q11), m.settle_ix(&q11, m.default_accounts(&q11))];
    refuse(&mut m, "Pay a payout partner suspended by monitoring", &ixs, &taker, Expect::Program(CorridorError::RecipientNotAllowed));

    println!("\n━━━ DONE ━━━\n");
    say("Every line above was asserted against the program's real behaviour.");
    say("Full suite: anchor build --arch v0 --ignore-keys && cargo test   (133 tests)\n");
}
