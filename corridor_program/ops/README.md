# Devnet operations

Scripts that stand up the Corridor settlement program on devnet. Each step is
safe to re-run: it skips anything that already exists.

| Step | Script | What it does |
|---|---|---|
| 1 | `node 01-create-multisigs.mjs` | Admin and compliance Squads v4 multisigs, 2-of-3, no time lock |
| 2 | `./02-create-test-usdc.sh` | Test USDC mint (classic SPL, 6 decimals), records the devnet genesis hash |
| 3 | `node 03-initialize-config.mjs` | `initialize_config`, then reads Config back and checks every field |
| 4 | `npm run setup:market` | Test ZARSC mint, issuers and participants registered through Squads, funded token accounts, maker approvals, lookup table |
| – | `npm test` | Devnet test suite (`test/devnet.test.mjs`) against the deployed program |

Run `npm install` once first. The program itself is deployed separately with
`solana program deploy` (see the commit history for the command).

## Keys

Private keys are never in the repo. By default they are read from:

- `~/.config/solana/corridor-devnet-deployer.json`: deployer, upgrade authority, treasury owner, test USDC mint authority (override with `DEPLOYER_KEY`)
- `~/.config/solana/corridor-devnet/`: multisig members and create keys, guardian, test mint keypair (override with `KEYS_DIR`)

Back these up: losing the deployer loses the upgrade authority, and losing
two members of a multisig loses that role.

## Addresses

`devnet.json` records every public address this setup created. It contains
no secrets.

Config is set so that admin and compliance actions are signed by the Squads
**vault** PDAs, so they have to go through a Squads proposal, approval and
execution. The guardian is a single hot key (spec F7).
