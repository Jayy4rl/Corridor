// UC-01 initialize_config, signed by the upgrade authority (the deployer).
// Sets admin and compliance to the Squads vaults, the guardian to its hot
// key, and the treasury to the deployer. Reads back and checks the stored
// Config afterwards. Safe to re-run: does nothing if Config already exists.
import { createHash } from "node:crypto";
import { PublicKey, SystemProgram, Transaction, TransactionInstruction } from "@solana/web3.js";
import {
  BPF_LOADER_UPGRADEABLE,
  PROGRAM_ID,
  confirm,
  connection,
  deployer,
  exists,
  explorer,
  key,
  readRecord,
  updateRecord,
} from "./lib.mjs";

const FEE_BPS = 10;
const RECEIPT_RETENTION_SECS = 3_600n;

const record = readRecord();
for (const field of ["squads", "usdcMint", "genesisHash"]) {
  if (!record[field]) throw new Error(`devnet.json is missing ${field}; run the earlier steps first`);
}

const payer = deployer();
const [config] = PublicKey.findProgramAddressSync([Buffer.from("config")], PROGRAM_ID);
const [programData] = PublicKey.findProgramAddressSync([PROGRAM_ID.toBuffer()], BPF_LOADER_UPGRADEABLE);
const [eventAuthority] = PublicKey.findProgramAddressSync([Buffer.from("__event_authority")], PROGRAM_ID);

const args = {
  admin: new PublicKey(record.squads.admin.vault),
  complianceAuthority: new PublicKey(record.squads.compliance.vault),
  guardian: key("guardian").publicKey,
  usdcMint: new PublicKey(record.usdcMint),
  feeBps: FEE_BPS,
  treasuryOwner: payer.publicKey,
  receiptRetentionSecs: RECEIPT_RETENTION_SECS,
  // The genesis hash is base58 of 32 bytes, the same encoding as a pubkey.
  genesisHash: new PublicKey(record.genesisHash).toBuffer(),
};

if (await exists(config)) {
  console.log("Config already exists; not re-initialising");
} else {
  // Anchor instruction: sha256("global:initialize_config")[..8] | Borsh(args)
  const discriminator = createHash("sha256").update("global:initialize_config").digest().subarray(0, 8);
  const feeBps = Buffer.alloc(2);
  feeBps.writeUInt16LE(args.feeBps);
  const retention = Buffer.alloc(8);
  retention.writeBigUInt64LE(args.receiptRetentionSecs);
  const data = Buffer.concat([
    discriminator,
    args.admin.toBuffer(),
    args.complianceAuthority.toBuffer(),
    args.guardian.toBuffer(),
    args.usdcMint.toBuffer(),
    feeBps,
    args.treasuryOwner.toBuffer(),
    retention,
    args.genesisHash,
  ]);
  const ix = new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: payer.publicKey, isSigner: true, isWritable: true },
      { pubkey: config, isSigner: false, isWritable: true },
      { pubkey: programData, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      { pubkey: eventAuthority, isSigner: false, isWritable: false },
      { pubkey: PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data,
  });
  const signature = await confirm(await connection.sendTransaction(new Transaction().add(ix), [payer]));
  console.log(`initialize_config: ${explorer("tx", signature)}`);
  updateRecord({ initializeConfigTx: signature });
}

// Read back Config and compare every field with what was intended.
const info = await connection.getAccountInfo(config);
const d = info.data;
let at = 8;
const pubkey = () => new PublicKey(d.subarray(at, (at += 32)));
const stored = {
  admin: pubkey(),
  complianceAuthority: pubkey(),
  guardian: pubkey(),
  usdcMint: pubkey(),
  feeBps: d.readUInt16LE(at),
};
at += 2;
stored.treasuryOwner = pubkey();
stored.receiptRetentionSecs = d.readBigUInt64LE(at);
at += 8;
stored.paused = d[at++] === 1;
stored.pendingAdmin = d[at++] === 1 ? pubkey() : null;
stored.genesisHash = new PublicKey(d.subarray(at, (at += 32)));

const checks = [
  ["admin (admin vault)", stored.admin.equals(args.admin)],
  ["compliance (compliance vault)", stored.complianceAuthority.equals(args.complianceAuthority)],
  ["guardian", stored.guardian.equals(args.guardian)],
  ["usdc_mint", stored.usdcMint.equals(args.usdcMint)],
  ["fee_bps", stored.feeBps === args.feeBps],
  ["treasury_owner (deployer)", stored.treasuryOwner.equals(args.treasuryOwner)],
  ["receipt_retention_secs", stored.receiptRetentionSecs === args.receiptRetentionSecs],
  ["genesis_hash", stored.genesisHash.toBase58() === record.genesisHash],
  ["not paused", stored.paused === false],
  ["no pending admin", stored.pendingAdmin === null],
];
for (const [name, ok] of checks) console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
if (checks.some(([, ok]) => !ok)) process.exit(1);

updateRecord({
  config: config.toBase58(),
  guardian: args.guardian.toBase58(),
  treasuryOwner: args.treasuryOwner.toBase58(),
  feeBps: FEE_BPS,
  receiptRetentionSecs: Number(RECEIPT_RETENTION_SECS),
});
console.log(`Config ${explorer("address", config.toBase58())}`);
