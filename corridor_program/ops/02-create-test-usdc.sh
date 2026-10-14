#!/usr/bin/env bash
# Creates the devnet test USDC mint: classic SPL Token, 6 decimals like real
# USDC, mint authority = deployer so tests can fund accounts. config.usdc_mint
# is immutable, so this mint is permanent for this devnet deployment.
# Safe to re-run: reuses the mint keypair and skips creation if it exists.
set -euo pipefail
KEYS_DIR="${KEYS_DIR:-$HOME/.config/solana/corridor-devnet}"
DEPLOYER_KEY="${DEPLOYER_KEY:-$HOME/.config/solana/corridor-devnet-deployer.json}"
MINT_KEY="$KEYS_DIR/test-usdc-mint.json"
cd "$(dirname "$0")"

# RPC endpoint from ops/.env (gitignored); a shell variable takes precedence.
if [ -z "${RPC_URL:-}" ] && [ -f .env ]; then
  RPC_URL="$(sed -n 's/^RPC_URL=//p' .env)"
fi
URL="${RPC_URL:-devnet}"

test -e "$MINT_KEY" || solana-keygen new --no-bip39-passphrase --silent -o "$MINT_KEY" >/dev/null
MINT="$(solana address -k "$MINT_KEY")"

if solana account "$MINT" --url "$URL" >/dev/null 2>&1; then
  echo "test USDC mint already exists: $MINT"
else
  spl-token create-token "$MINT_KEY" --decimals 6 \
    --mint-authority "$(solana address -k "$DEPLOYER_KEY")" \
    --fee-payer "$DEPLOYER_KEY" --url "$URL"
fi

GENESIS="$(solana genesis-hash --url "$URL")"
node -e "import('./lib.mjs').then(({ updateRecord }) => updateRecord({ usdcMint: '$MINT', usdcDecimals: 6, genesisHash: '$GENESIS' }))"
echo "usdcMint $MINT"
echo "genesisHash $GENESIS"
