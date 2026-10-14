// One-time devnet market for the test suite: a test ZARSC mint, both issuers
// registered through the admin Squads vault, a taker, maker (with a separate
// quote key) and payout partner registered through the compliance vault,
// funded token accounts, the maker's approvals to its delegate PDA, and an
// address lookup table for v0 settlement transactions.
// Safe to re-run: every step checks what already exists.
import {
  AddressLookupTableProgram,
  LAMPORTS_PER_SOL,
  PublicKey,
  SystemProgram,
} from "@solana/web3.js";
import {
  MINT_SIZE,
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  createApproveCheckedInstruction,
  createAssociatedTokenAccountIdempotentInstruction,
  createInitializeMint2Instruction,
  createMintToCheckedInstruction,
  getAssociatedTokenAddressSync,
} from "@solana/spl-token";
import {
  configPda,
  delegatePda,
  ensureKey,
  eventAuthority,
  executeAsVault,
  issuerPda,
  ix,
  participantPda,
  send,
  vaultAddress,
} from "./corridor.mjs";
import { connection, deployer, exists, explorer, key, readRecord, updateRecord } from "./lib.mjs";

const ZARSC_DECIMALS = 2;
const USDC_DECIMALS = 6;
const ATTESTATION_DAYS = 90;
const MAX_TRADE_AMOUNT = 10_000_000_00n; // 10,000,000.00 ZARSC per trade
const DAILY_CAP = 100_000_000_00n; // 100,000,000.00 ZARSC per day
const MAX_TRADE_USDC = 100_000_000_000n; // 100,000 USDC
const DAILY_LIMIT_USDC = 1_000_000_000_000n; // 1,000,000 USDC
const FUND_USDC = 50_000_000_000n; // 50,000 USDC each for taker and maker
const FUND_ZARSC = 1_000_000_00n; // 1,000,000.00 ZARSC each
const KYB_HASH = Array(32).fill(2);
const ATTESTATION_HASH = Array(32).fill(1);

const payer = deployer();
const record = readRecord();
const usdcMint = new PublicKey(record.usdcMint);
const step = (text) => console.log(`\n▸ ${text}`);

// 1. SOL for accounts that pay rent or fees.
step("Funding vaults, taker, maker and guardian with SOL");
const toFund = [
  ["admin vault", vaultAddress("admin"), 0.05],
  ["compliance vault", vaultAddress("compliance"), 0.05],
  ["taker", ensureKey("market-taker").publicKey, 0.05],
  ["maker", ensureKey("market-maker").publicKey, 0.02],
  ["guardian", key("guardian").publicKey, 0.02],
];
for (const [name, address, sol] of toFund) {
  const balance = await connection.getBalance(address);
  if (balance >= (sol / 2) * LAMPORTS_PER_SOL) {
    console.log(`  ${name} already has ${balance / LAMPORTS_PER_SOL} SOL`);
    continue;
  }
  await send(payer.publicKey, [SystemProgram.transfer({ fromPubkey: payer.publicKey, toPubkey: address, lamports: sol * LAMPORTS_PER_SOL })], [payer]);
  console.log(`  ${name} funded with ${sol} SOL`);
}

// 2. Test ZARSC: Token-2022, 2 decimals, deployer as mint authority.
step("Test ZARSC mint (Token-2022)");
const zarscKey = ensureKey("test-zarsc-mint");
const zarscMint = zarscKey.publicKey;
if (await exists(zarscMint)) {
  console.log(`  exists: ${zarscMint.toBase58()}`);
} else {
  const lamports = await connection.getMinimumBalanceForRentExemption(MINT_SIZE);
  await send(
    payer.publicKey,
    [
      SystemProgram.createAccount({ fromPubkey: payer.publicKey, newAccountPubkey: zarscMint, space: MINT_SIZE, lamports, programId: TOKEN_2022_PROGRAM_ID }),
      createInitializeMint2Instruction(zarscMint, ZARSC_DECIMALS, payer.publicKey, null, TOKEN_2022_PROGRAM_ID),
    ],
    [payer, zarscKey],
  );
  console.log(`  created: ${zarscMint.toBase58()}`);
}
updateRecord({ market: { ...(readRecord().market ?? {}), zarscMint: zarscMint.toBase58() } });

// 3. Issuers, through the admin vault (one Squads proposal for both).
step("Registering USDC and ZARSC issuers via the admin multisig");
const attestationExpiresAt = Math.floor(Date.now() / 1000) + ATTESTATION_DAYS * 86_400;
const adminVault = vaultAddress("admin");
const issuerIxs = [];
for (const mint of [usdcMint, zarscMint]) {
  if (await exists(issuerPda(mint))) {
    console.log(`  already registered: ${mint.toBase58()}`);
    continue;
  }
  issuerIxs.push(
    ix(
      "register_issuer",
      { admin: adminVault, config: configPda(), mint, issuer_entry: issuerPda(mint) },
      {
        attestation_hash: ATTESTATION_HASH,
        attestation_expires_at: attestationExpiresAt,
        max_trade_amount: MAX_TRADE_AMOUNT,
        daily_cap: DAILY_CAP,
      },
    ),
  );
}
if (issuerIxs.length) console.log(`  executed: ${explorer("tx", await executeAsVault("admin", issuerIxs))}`);

// 4. Participants, through the compliance vault.
step("Registering taker, maker and payout partner via the compliance multisig");
const taker = ensureKey("market-taker").publicKey;
const maker = ensureKey("market-maker").publicKey;
const quoteKey = ensureKey("market-maker-quote-key").publicKey;
const payout = ensureKey("market-payout").publicKey;
const complianceVault = vaultAddress("compliance");
const participants = [
  [taker, "Taker", MAX_TRADE_USDC, DAILY_LIMIT_USDC, null],
  [maker, "Maker", MAX_TRADE_USDC, DAILY_LIMIT_USDC, quoteKey],
  [payout, "PayoutPartner", 0n, 0n, null],
];
const participantIxs = [];
for (const [wallet, role, maxTrade, dailyLimit, quoteSigner] of participants) {
  if (await exists(participantPda(wallet))) {
    console.log(`  already registered: ${role} ${wallet.toBase58()}`);
    continue;
  }
  participantIxs.push(
    ix(
      "register_participant",
      { compliance: complianceVault, config: configPda(), participant: participantPda(wallet) },
      {
        wallet,
        role,
        kyb_ref_hash: KYB_HASH,
        max_trade_usdc: maxTrade,
        daily_limit_usdc: dailyLimit,
        quote_signer: quoteSigner,
      },
    ),
  );
}
if (participantIxs.length) console.log(`  executed: ${explorer("tx", await executeAsVault("compliance", participantIxs))}`);

// 5. Token accounts, balances and the maker's approvals.
step("Token accounts, balances and maker approvals");
const programFor = (mint) => (mint.equals(usdcMint) ? TOKEN_PROGRAM_ID : TOKEN_2022_PROGRAM_ID);
const decimalsFor = (mint) => (mint.equals(usdcMint) ? USDC_DECIMALS : ZARSC_DECIMALS);
const ata = (owner, mint) => getAssociatedTokenAddressSync(mint, owner, false, programFor(mint));
for (const mint of [usdcMint, zarscMint]) {
  const owners = [taker, maker, payout, payer.publicKey];
  await send(
    payer.publicKey,
    owners.map((owner) => createAssociatedTokenAccountIdempotentInstruction(payer.publicKey, ata(owner, mint), owner, mint, programFor(mint))),
    [payer],
  );
  for (const owner of [taker, maker]) {
    const target = mint.equals(usdcMint) ? FUND_USDC : FUND_ZARSC;
    const { value } = await connection.getTokenAccountBalance(ata(owner, mint));
    const missing = target - BigInt(value.amount);
    if (missing > 0n) {
      await send(payer.publicKey, [createMintToCheckedInstruction(mint, ata(owner, mint), payer.publicKey, missing, decimalsFor(mint), [], programFor(mint))], [payer]);
    }
  }
  const makerKey = ensureKey("market-maker");
  await send(
    payer.publicKey,
    [createApproveCheckedInstruction(ata(maker, mint), mint, delegatePda(maker), maker, 2n ** 64n - 1n, decimalsFor(mint), [], programFor(mint))],
    [payer, makerKey],
  );
  console.log(`  ${mint.equals(usdcMint) ? "USDC " : "ZARSC"}: accounts ready, taker and maker funded, maker approval set`);
}

// 6. Address lookup table holding the accounts every settlement uses.
step("Address lookup table for v0 settlement transactions");
let lookupTable = readRecord().market?.lookupTable;
if (lookupTable && (await exists(new PublicKey(lookupTable)))) {
  console.log(`  exists: ${lookupTable}`);
} else {
  const addresses = [
    configPda(),
    eventAuthority(),
    usdcMint,
    zarscMint,
    issuerPda(usdcMint),
    issuerPda(zarscMint),
    participantPda(taker),
    participantPda(maker),
    participantPda(payout),
    delegatePda(maker),
    new PublicKey("Sysvar1nstructions1111111111111111111111111"),
    TOKEN_PROGRAM_ID,
    TOKEN_2022_PROGRAM_ID,
    SystemProgram.programId,
    ...[usdcMint, zarscMint].flatMap((mint) => [taker, maker, payout, payer.publicKey].map((owner) => ata(owner, mint))),
  ];
  // The table address derives from a recent slot that the validator checking
  // the transaction must recognise. Behind a load-balanced public RPC the
  // slot can come from a node out of step with it, so retry with a fresh one.
  for (let attempt = 1; ; attempt++) {
    const recentSlot = await connection.getSlot("finalized");
    const [create, table] = AddressLookupTableProgram.createLookupTable({ authority: payer.publicKey, payer: payer.publicKey, recentSlot });
    const extend = AddressLookupTableProgram.extendLookupTable({ payer: payer.publicKey, authority: payer.publicKey, lookupTable: table, addresses });
    try {
      await send(payer.publicKey, [create, extend], [payer]);
      lookupTable = table.toBase58();
      break;
    } catch (error) {
      const staleSlot = String(error?.transactionLogs ?? error?.message).includes("is not a recent slot");
      if (!staleSlot || attempt === 5) throw error;
      console.log(`  slot ${recentSlot} not recognised by the RPC node, retrying (${attempt}/5)`);
      await new Promise((resolve) => setTimeout(resolve, 3_000));
    }
  }
  // A new table is usable from the next slot onwards.
  await new Promise((resolve) => setTimeout(resolve, 2_000));
  console.log(`  created with ${addresses.length} addresses: ${lookupTable}`);
}

updateRecord({
  market: {
    zarscMint: zarscMint.toBase58(),
    lookupTable,
    taker: taker.toBase58(),
    maker: maker.toBase58(),
    makerQuoteKey: quoteKey.toBase58(),
    payout: payout.toBase58(),
  },
});
console.log("\nMarket ready. Run the suite with: npm test");

