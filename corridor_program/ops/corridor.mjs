// Client for the Corridor program on devnet: instruction builder driven by
// the IDL, PDAs, signed quotes, v0 settlement transactions with the lookup
// table, and Squads vault execution for admin and compliance actions.
import fs from "node:fs";
import path from "node:path";
import {
  Ed25519Program,
  Keypair,
  PublicKey,
  SystemProgram,
  SYSVAR_INSTRUCTIONS_PUBKEY,
  TransactionInstruction,
  TransactionMessage,
  VersionedTransaction,
} from "@solana/web3.js";
import { TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID, getAssociatedTokenAddressSync } from "@solana/spl-token";
import * as squads from "@sqds/multisig";
import { KEYS_DIR, PROGRAM_ID, confirm, connection, deployer, key, readRecord } from "./lib.mjs";

const idl = JSON.parse(fs.readFileSync(new URL("./idl/corridor_program.json", import.meta.url), "utf8"));

// ---- Keys ----

/** A key from KEYS_DIR, created on first use. */
export function ensureKey(name) {
  const file = path.join(KEYS_DIR, `${name}.json`);
  if (!fs.existsSync(file)) {
    fs.writeFileSync(file, JSON.stringify(Array.from(Keypair.generate().secretKey)), { mode: 0o600 });
  }
  return key(name);
}

// ---- PDAs ----

const seed = (s) => Buffer.from(s);
const pda = (...seeds) => PublicKey.findProgramAddressSync(seeds, PROGRAM_ID)[0];
const u64le = (n) => {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(BigInt(n));
  return b;
};

export const configPda = () => pda(seed("config"));
export const issuerPda = (mint) => pda(seed("issuer"), mint.toBuffer());
export const participantPda = (wallet) => pda(seed("participant"), wallet.toBuffer());
export const receiptPda = (maker, quoteId) => pda(seed("receipt"), maker.toBuffer(), u64le(quoteId));
export const delegatePda = (maker) => pda(seed("delegate"), maker.toBuffer());
export const eventAuthority = () => pda(seed("__event_authority"));

// ---- Instructions from the IDL ----

function encode(type, value, out) {
  if (type === "bool") out.push(Buffer.from([value ? 1 : 0]));
  else if (type === "u8") out.push(Buffer.from([value]));
  else if (type === "u16") {
    const b = Buffer.alloc(2);
    b.writeUInt16LE(value);
    out.push(b);
  } else if (type === "u64") out.push(u64le(value));
  else if (type === "i64") {
    const b = Buffer.alloc(8);
    b.writeBigInt64LE(BigInt(value));
    out.push(b);
  } else if (type === "pubkey") out.push(new PublicKey(value).toBuffer());
  else if (type.array) out.push(Buffer.from(value));
  else if (type.option) {
    if (value == null) out.push(Buffer.from([0]));
    else {
      out.push(Buffer.from([1]));
      encode(type.option, value, out);
    }
  } else if (type.defined) {
    const def = idl.types.find((t) => t.name === type.defined.name);
    const index = def.type.variants.findIndex((v) => v.name === value);
    if (def.type.kind !== "enum" || index < 0) throw new Error(`cannot encode ${type.defined.name}=${value}`);
    out.push(Buffer.from([index]));
  } else throw new Error(`unsupported IDL type ${JSON.stringify(type)}`);
}

/**
 * Builds a Corridor instruction by name. `accounts` maps IDL account names to
 * public keys; fixed addresses (system program, sysvar), the event authority
 * and the program itself are filled in automatically.
 */
export function ix(name, accounts, args = {}) {
  const def = idl.instructions.find((i) => i.name === name);
  if (!def) throw new Error(`no instruction ${name} in IDL`);
  const defaults = { event_authority: eventAuthority(), program: PROGRAM_ID };
  const keys = def.accounts.map((a) => {
    const pubkey = accounts[a.name] ?? defaults[a.name] ?? (a.address && new PublicKey(a.address));
    if (!pubkey) throw new Error(`${name}: missing account ${a.name}`);
    return { pubkey, isSigner: Boolean(a.signer), isWritable: Boolean(a.writable) };
  });
  const parts = [Buffer.from(def.discriminator)];
  for (const arg of def.args) {
    if (!(arg.name in args)) throw new Error(`${name}: missing argument ${arg.name}`);
    encode(arg.type, args[arg.name], parts);
  }
  return new TransactionInstruction({ programId: PROGRAM_ID, keys, data: Buffer.concat(parts) });
}

// ---- The devnet market recorded by the setup scripts ----

export function market() {
  const r = readRecord();
  if (!r.market) throw new Error("devnet.json has no market; run `npm run setup:market` first");
  const usdcMint = new PublicKey(r.usdcMint);
  const zarscMint = new PublicKey(r.market.zarscMint);
  const wallets = {
    taker: ensureKey("market-taker"),
    maker: ensureKey("market-maker"),
    quoteKey: ensureKey("market-maker-quote-key"),
    payout: ensureKey("market-payout"),
  };
  const treasury = new PublicKey(r.treasuryOwner);
  const ata = (owner, mint) =>
    getAssociatedTokenAddressSync(mint, owner, false, mint.equals(usdcMint) ? TOKEN_PROGRAM_ID : TOKEN_2022_PROGRAM_ID);
  return {
    record: r,
    usdcMint,
    zarscMint,
    treasury,
    lookupTable: r.market.lookupTable ? new PublicKey(r.market.lookupTable) : null,
    genesisHash: new PublicKey(r.genesisHash).toBuffer(),
    ...wallets,
    ata,
  };
}

// ---- Quotes and settlement ----

const DOMAIN_TAG = Buffer.from("CORRIDOR_QUOTE_V1");

/** The 241 bytes the maker's quote key signs (see src/quote.rs). */
export function quoteMessage(m, q, maker = m.maker.publicKey, taker = m.taker.publicKey) {
  const [mintIn, mintOut] = q.direction === "LocalToUsdc" ? [m.zarscMint, m.usdcMint] : [m.usdcMint, m.zarscMint];
  const expires = Buffer.alloc(8);
  expires.writeBigInt64LE(BigInt(q.expiresAt));
  return Buffer.concat([
    DOMAIN_TAG,
    m.genesisHash,
    PROGRAM_ID.toBuffer(),
    u64le(q.quoteId),
    maker.toBuffer(),
    taker.toBuffer(),
    mintIn.toBuffer(),
    mintOut.toBuffer(),
    u64le(q.amountIn),
    u64le(q.amountOut),
    expires,
  ]);
}

/** Ed25519 verify instruction; web3.js sets every offset index to u16::MAX. */
export function signatureIx(signer, message) {
  return Ed25519Program.createInstructionWithPrivateKey({ privateKey: signer.secretKey, message });
}

/** Accounts for a correct settlement; tests override fields to attack it. */
export function settleAccounts(m, q, { maker = m.maker.publicKey, taker = m.taker.publicKey, recipient = m.payout.publicKey } = {}) {
  const [mintIn, mintOut] = q.direction === "LocalToUsdc" ? [m.zarscMint, m.usdcMint] : [m.usdcMint, m.zarscMint];
  return {
    taker,
    config: configPda(),
    taker_participant: participantPda(taker),
    maker_participant: participantPda(maker),
    recipient_participant: participantPda(recipient),
    usdc_mint: m.usdcMint,
    local_mint: m.zarscMint,
    usdc_issuer: issuerPda(m.usdcMint),
    local_issuer: issuerPda(m.zarscMint),
    taker_ata_in: m.ata(taker, mintIn),
    maker_ata_in: m.ata(maker, mintIn),
    maker_ata_out: m.ata(maker, mintOut),
    recipient_ata_out: m.ata(recipient, mintOut),
    treasury_ata_out: m.ata(m.treasury, mintOut),
    delegate_authority: delegatePda(maker),
    quote_receipt: receiptPda(maker, q.quoteId),
    usdc_token_program: TOKEN_PROGRAM_ID,
    local_token_program: TOKEN_2022_PROGRAM_ID,
  };
}

export function settleIx(q, accounts) {
  return ix("settle_quote", accounts, {
    quote_id: q.quoteId,
    direction: q.direction,
    amount_in: q.amountIn,
    amount_out: q.amountOut,
    expires_at: q.expiresAt,
    min_amount_out: q.minAmountOut,
    compliance_hash: q.complianceHash,
  });
}

/** A v0 transaction using the market's lookup table (legacy is too large). */
export async function v0Transaction(payer, instructions, signers, lookupTable = null) {
  const tables = [];
  if (lookupTable) tables.push((await connection.getAddressLookupTable(lookupTable)).value);
  const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash("confirmed");
  const message = new TransactionMessage({ payerKey: payer, recentBlockhash: blockhash, instructions }).compileToV0Message(tables);
  const tx = new VersionedTransaction(message);
  tx.sign(signers);
  return { tx, blockhash, lastValidBlockHeight };
}

/** Sends and confirms; throws with the program logs if it fails. */
export async function send(payer, instructions, signers, lookupTable = null) {
  const { tx } = await v0Transaction(payer, instructions, signers, lookupTable);
  return confirm(await connection.sendTransaction(tx));
}

/** Simulates on devnet without sending; returns { err, logs }. */
export async function simulate(payer, instructions, signers, lookupTable = null) {
  const { tx } = await v0Transaction(payer, instructions, signers, lookupTable);
  const { value } = await connection.simulateTransaction(tx, { sigVerify: true, commitment: "confirmed" });
  return { err: value.err, logs: value.logs ?? [] };
}

/** The Anchor error name in simulation logs, e.g. "InvalidQuoteSignature". */
export function anchorError(logs) {
  const line = logs.find((l) => l.includes("Error Code: "));
  return line ? line.match(/Error Code: (\w+)/)[1] : null;
}

/** Current cluster time, so quote expiries match the validator's clock. */
export async function chainTime() {
  const slot = await connection.getSlot("confirmed");
  return (await connection.getBlockTime(slot)) ?? Math.floor(Date.now() / 1000);
}

// ---- Events ----

const EVENT_IX_TAG = Buffer.from("e445a52e51cb9a1d", "hex"); // 0x1d9acb512ea545e4 little-endian

/** Decodes QuoteSettled from a confirmed transaction's self-CPI event. */
export async function quoteSettledEvent(signature) {
  const { default: bs58 } = await import("bs58");
  const disc = Buffer.from(idl.events.find((e) => e.name === "QuoteSettled").discriminator);
  const tx = await connection.getTransaction(signature, { commitment: "confirmed", maxSupportedTransactionVersion: 0 });
  for (const inner of tx?.meta?.innerInstructions ?? []) {
    for (const { data } of inner.instructions) {
      const bytes = Buffer.from(bs58.decode(data));
      if (bytes.subarray(0, 8).equals(EVENT_IX_TAG) && bytes.subarray(8, 16).equals(disc)) {
        const body = bytes.subarray(16);
        return {
          quoteId: body.readBigUInt64LE(0),
          amountIn: body.readBigUInt64LE(168),
          amountOut: body.readBigUInt64LE(176),
          fee: body.readBigUInt64LE(184),
          complianceHash: body.subarray(192, 224),
        };
      }
    }
  }
  return null;
}

// ---- Squads: admin and compliance actions ----

/**
 * Runs `instructions` as the admin or compliance vault: creates a vault
 * transaction and proposal, approves it with two of the three members and
 * executes it. The deployer pays fees and rent.
 */
export async function executeAsVault(role, instructions) {
  const { squads: record } = readRecord();
  const multisigPda = new PublicKey(record[role].multisig);
  const [vaultPda] = squads.getVaultPda({ multisigPda, index: 0 });
  const [first, second] = [key(`${role}-member-1`), key(`${role}-member-2`)];
  const payer = deployer();

  const account = await squads.accounts.Multisig.fromAccountAddress(connection, multisigPda);
  const transactionIndex = BigInt(account.transactionIndex.toString()) + 1n;
  const { blockhash } = await connection.getLatestBlockhash("confirmed");
  const transactionMessage = new TransactionMessage({ payerKey: vaultPda, recentBlockhash: blockhash, instructions });

  await confirm(
    await squads.rpc.vaultTransactionCreate({
      connection,
      feePayer: payer,
      multisigPda,
      transactionIndex,
      creator: first.publicKey,
      rentPayer: payer.publicKey,
      vaultIndex: 0,
      ephemeralSigners: 0,
      transactionMessage,
      signers: [first],
    }),
  );
  // Not squads.rpc.proposalCreate: in @sqds/multisig 2.1.4 it drops
  // `rentPayer`, so the (unfunded) member would be charged the rent.
  const proposal = squads.instructions.proposalCreate({
    multisigPda,
    creator: first.publicKey,
    rentPayer: payer.publicKey,
    transactionIndex,
  });
  await send(payer.publicKey, [proposal], [payer, first]);
  for (const member of [first, second]) {
    await confirm(await squads.rpc.proposalApprove({ connection, feePayer: payer, member, multisigPda, transactionIndex }));
  }
  return confirm(
    await squads.rpc.vaultTransactionExecute({
      connection,
      feePayer: payer,
      multisigPda,
      transactionIndex,
      member: first.publicKey,
      signers: [first],
    }),
  );
}

export function vaultAddress(role) {
  const multisigPda = new PublicKey(readRecord().squads[role].multisig);
  return squads.getVaultPda({ multisigPda, index: 0 })[0];
}

export { SystemProgram, SYSVAR_INSTRUCTIONS_PUBKEY, TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID };
