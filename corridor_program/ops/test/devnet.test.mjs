// Devnet test suite for the deployed Corridor program.
//
//   npm run setup:market   # once
//   npm test
//
// Successful paths send real transactions and check balances and events.
// Refusals are simulated against devnet state (no SOL spent, nothing
// recorded) and must fail with the exact Anchor error.
import assert from "node:assert/strict";
import { after, before, describe, test } from "node:test";
import { PublicKey } from "@solana/web3.js";
import {
  anchorError,
  chainTime,
  configPda,
  executeAsVault,
  issuerPda,
  ix,
  market,
  participantPda,
  quoteMessage,
  quoteSettledEvent,
  receiptPda,
  send,
  settleAccounts,
  settleIx,
  signatureIx,
  simulate,
  vaultAddress,
} from "../corridor.mjs";
import { PROGRAM_ID, connection, deployer, explorer, key, readRecord } from "../lib.mjs";

// A small trade so test funds last: 180.00 ZARSC for 10 USDC.
const ZARSC_AMOUNT = 18_000n;
const USDC_AMOUNT = 10_000_000n;
const COMPLIANCE_HASH = Array(32).fill(3);

let m;
let feeBps;
let quoteCounter = 0;
let settledQuote; // first successful settlement, reused by replay and close tests

/** A unique, correctly priced quote expiring `ttl` seconds from chain time. */
async function newQuote(direction, ttl = 60) {
  const [amountIn, amountOut] = direction === "LocalToUsdc" ? [ZARSC_AMOUNT, USDC_AMOUNT] : [USDC_AMOUNT, ZARSC_AMOUNT];
  const fee = (amountOut * BigInt(feeBps)) / 10_000n;
  return {
    quoteId: BigInt(Date.now()) * 1000n + BigInt(quoteCounter++),
    direction,
    amountIn,
    amountOut,
    expiresAt: (await chainTime()) + ttl,
    minAmountOut: amountOut - fee,
    complianceHash: COMPLIANCE_HASH,
  };
}

/** [quote-key signature, settle_quote] for a quote, optionally with hostile accounts. */
function settlementIxs(q, { signer = m.quoteKey, accounts = settleAccounts(m, q), signed = q } = {}) {
  return [signatureIx(signer, quoteMessage(m, signed)), settleIx(q, accounts)];
}

const settle = (ixs) => send(m.taker.publicKey, ixs, [m.taker], m.lookupTable);

async function expectRefused(ixs, expected, { payer = m.taker, lookupTable = m.lookupTable } = {}) {
  const { err, logs } = await simulate(payer.publicKey, ixs, [payer], lookupTable);
  assert.ok(err, `expected ${expected}, but the transaction would succeed`);
  if (expected === "ReceiptExists") {
    assert.ok(logs.some((l) => l.includes("already in use")), `expected the receipt to block it:\n${logs.join("\n")}`);
  } else {
    assert.equal(anchorError(logs), expected, logs.join("\n"));
  }
}

async function balance(owner, mint) {
  const { value } = await connection.getTokenAccountBalance(m.ata(owner, mint));
  return BigInt(value.amount);
}

async function readConfig() {
  const d = (await connection.getAccountInfo(configPda())).data;
  return {
    admin: new PublicKey(d.subarray(8, 40)),
    compliance: new PublicKey(d.subarray(40, 72)),
    guardian: new PublicKey(d.subarray(72, 104)),
    usdcMint: new PublicKey(d.subarray(104, 136)),
    feeBps: d.readUInt16LE(136),
    paused: d[178] === 1,
  };
}

describe("Corridor settlement program on devnet", () => {
  before(async () => {
    m = market();
    assert.ok(m.lookupTable, "no lookup table recorded; run `npm run setup:market` first");
    feeBps = (await readConfig()).feeBps;
  });

  after(async () => {
    // Never leave devnet paused if a test failed half-way through.
    if (m && (await readConfig()).paused) {
      await executeAsVault("admin", [ix("set_paused", { authority: vaultAddress("admin"), config: configPda() }, { paused: false })]);
    }
  });

  describe("deployment", () => {
    test("program is deployed and executable", async () => {
      const program = await connection.getAccountInfo(PROGRAM_ID);
      assert.ok(program?.executable);
    });

    test("config holds the Squads vaults, guardian and test USDC", async () => {
      const config = await readConfig();
      const record = readRecord();
      assert.ok(config.admin.equals(vaultAddress("admin")), "admin is the admin Squads vault");
      assert.ok(config.compliance.equals(vaultAddress("compliance")), "compliance is the compliance Squads vault");
      assert.equal(config.guardian.toBase58(), record.guardian);
      assert.equal(config.usdcMint.toBase58(), record.usdcMint);
      assert.equal(config.paused, false);
    });

    test("both issuers and all three participants are registered", async () => {
      for (const address of [
        issuerPda(m.usdcMint),
        issuerPda(m.zarscMint),
        participantPda(m.taker.publicKey),
        participantPda(m.maker.publicKey),
        participantPda(m.payout.publicKey),
      ]) {
        const account = await connection.getAccountInfo(address);
        assert.ok(account?.owner.equals(PROGRAM_ID), `${address.toBase58()} is a Corridor account`);
      }
    });
  });

  describe("settlement", () => {
    test("settles ZARSC → USDC atomically, paying the payout partner and the treasury", async () => {
      const q = await newQuote("LocalToUsdc");
      const fee = (q.amountOut * BigInt(feeBps)) / 10_000n;
      const tracked = [
        [m.taker.publicKey, m.zarscMint, -q.amountIn],
        [m.maker.publicKey, m.zarscMint, q.amountIn],
        [m.maker.publicKey, m.usdcMint, -q.amountOut],
        [m.payout.publicKey, m.usdcMint, q.amountOut - fee],
        [m.treasury, m.usdcMint, fee],
      ];
      const before = await Promise.all(tracked.map(([owner, mint]) => balance(owner, mint)));

      const signature = await settle(settlementIxs(q));
      console.log(`      settled: ${explorer("tx", signature)}`);

      const after = await Promise.all(tracked.map(([owner, mint]) => balance(owner, mint)));
      tracked.forEach(([, , delta], i) => assert.equal(after[i] - before[i], delta));
      assert.ok(await connection.getAccountInfo(receiptPda(m.maker.publicKey, q.quoteId)), "replay receipt created");
      settledQuote = q;
    });

    test("emits QuoteSettled for the indexer", async () => {
      const q = await newQuote("LocalToUsdc");
      const signature = await settle(settlementIxs(q));
      const event = await quoteSettledEvent(signature);
      assert.ok(event, "QuoteSettled not found in the transaction");
      assert.equal(event.quoteId, q.quoteId);
      assert.equal(event.amountIn, q.amountIn);
      assert.equal(event.amountOut, q.amountOut);
      assert.equal(event.fee, (q.amountOut * BigInt(feeBps)) / 10_000n);
      assert.deepEqual([...event.complianceHash], COMPLIANCE_HASH);
    });

    test("settles the reverse direction, USDC → ZARSC", async () => {
      const q = await newQuote("UsdcToLocal");
      const before = await balance(m.payout.publicKey, m.zarscMint);
      await settle(settlementIxs(q));
      const fee = (q.amountOut * BigInt(feeBps)) / 10_000n;
      assert.equal((await balance(m.payout.publicKey, m.zarscMint)) - before, q.amountOut - fee);
    });
  });

  describe("refusals (simulated on devnet, nothing moves)", () => {
    test("rejects replaying a settled quote", async () => {
      await expectRefused(settlementIxs(settledQuote), "ReceiptExists");
    });

    test("rejects a tampered amount", async () => {
      const signed = await newQuote("LocalToUsdc");
      const tampered = { ...signed, amountOut: signed.amountOut + 1_000_000n };
      await expectRefused(settlementIxs(tampered, { signed }), "InvalidQuoteSignature");
    });

    test("rejects a quote signed by the maker wallet instead of its quote key", async () => {
      const q = await newQuote("LocalToUsdc");
      await expectRefused(settlementIxs(q, { signer: m.maker }), "InvalidQuoteSignature");
    });

    test("rejects paying out of an account the maker does not own (F9)", async () => {
      const q = await newQuote("LocalToUsdc");
      const accounts = { ...settleAccounts(m, q), maker_ata_out: m.ata(m.taker.publicKey, m.usdcMint) };
      await expectRefused(settlementIxs(q, { accounts }), "AccountOwnerMismatch");
    });

    test("rejects an expired quote", async () => {
      const q = await newQuote("LocalToUsdc", -5);
      await expectRefused(settlementIxs(q), "QuoteExpired");
    });

    test("rejects a quote valid for longer than 120 seconds", async () => {
      const q = await newQuote("LocalToUsdc", 600);
      await expectRefused(settlementIxs(q), "QuoteTtlTooLong");
    });

    test("rejects closing a receipt before the retention period", async () => {
      const caller = deployer();
      const close = ix("close_receipt", {
        caller: caller.publicKey,
        config: configPda(),
        quote_receipt: receiptPda(m.maker.publicKey, settledQuote.quoteId),
        rent_payer: m.taker.publicKey,
      });
      await expectRefused([close], "RetentionNotElapsed", { payer: caller, lookupTable: null });
    });
  });

  describe("emergency stop", () => {
    test("guardian pauses; settlement and guardian unpause are refused; admin multisig resumes", async () => {
      const guardian = key("guardian");
      const setPaused = (authority, paused) => ix("set_paused", { authority, config: configPda() }, { paused });

      await send(guardian.publicKey, [setPaused(guardian.publicKey, true)], [guardian]);
      assert.equal((await readConfig()).paused, true);

      await expectRefused(settlementIxs(await newQuote("LocalToUsdc")), "Paused");
      await expectRefused([setPaused(guardian.publicKey, false)], "GuardianCannotResume", { payer: guardian, lookupTable: null });

      const signature = await executeAsVault("admin", [setPaused(vaultAddress("admin"), false)]);
      console.log(`      admin multisig unpaused: ${explorer("tx", signature)}`);
      assert.equal((await readConfig()).paused, false);

      await settle(settlementIxs(await newQuote("LocalToUsdc")));
    });
  });
});
