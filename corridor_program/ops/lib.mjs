// Shared helpers for the devnet ops scripts. Keys live outside the repo in
// KEYS_DIR; deployment addresses (public only) are recorded in devnet.json.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";

// ops/.env (gitignored) supplies RPC_URL; a variable already set in the shell
// takes precedence. An empty value falls back to the public devnet endpoint.
const ENV_FILE = fileURLToPath(new URL("./.env", import.meta.url));
if (fs.existsSync(ENV_FILE)) process.loadEnvFile(ENV_FILE);

export const RPC_URL = process.env.RPC_URL || "https://api.devnet.solana.com";
export const KEYS_DIR =
  process.env.KEYS_DIR ?? path.join(os.homedir(), ".config/solana/corridor-devnet");
export const DEPLOYER_KEY =
  process.env.DEPLOYER_KEY ??
  path.join(os.homedir(), ".config/solana/corridor-devnet-deployer.json");
export const PROGRAM_ID = new PublicKey("3RecGnphiwdGbpiKgvWi6JhSdtrVwtUHQcbTqBvbhiiM");
export const BPF_LOADER_UPGRADEABLE = new PublicKey("BPFLoaderUpgradeab1e11111111111111111111111");

const RECORD = new URL("./devnet.json", import.meta.url);

export const connection = new Connection(RPC_URL, "confirmed");

export function loadKeypair(file) {
  return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(file, "utf8"))));
}

/** A key from KEYS_DIR by name, e.g. key("admin-member-1"). */
export function key(name) {
  return loadKeypair(path.join(KEYS_DIR, `${name}.json`));
}

export function deployer() {
  return loadKeypair(DEPLOYER_KEY);
}

export function readRecord() {
  return fs.existsSync(RECORD) ? JSON.parse(fs.readFileSync(RECORD, "utf8")) : {};
}

/** Merges `fields` into devnet.json. Values must be public data only. */
export function updateRecord(fields) {
  const record = { ...readRecord(), ...fields };
  fs.writeFileSync(RECORD, JSON.stringify(record, null, 2) + "\n");
  return record;
}

/**
 * Waits for `signature` to reach "confirmed" by polling. web3.js's
 * confirmTransaction opens a WebSocket that is never closed, which keeps
 * Node (and `npm test`) running after the work is done.
 */
export async function confirm(signature, timeoutMs = 90_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const {
      value: [status],
    } = await connection.getSignatureStatuses([signature]);
    if (status?.err) throw new Error(`transaction ${signature} failed: ${JSON.stringify(status.err)}`);
    if (status?.confirmationStatus === "confirmed" || status?.confirmationStatus === "finalized") return signature;
    await new Promise((resolve) => setTimeout(resolve, 1_000));
  }
  throw new Error(`transaction ${signature} not confirmed within ${timeoutMs / 1000} s`);
}

export async function exists(address) {
  return (await connection.getAccountInfo(address)) !== null;
}

export function explorer(kind, value) {
  return `https://explorer.solana.com/${kind}/${value}?cluster=devnet`;
}
