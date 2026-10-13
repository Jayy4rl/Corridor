# Corridor

Corridor is a Solana program that lets licensed African money-transfer businesses swap a rand stablecoin (ZARSC) for USDC with a vetted trading partner, in a single transaction where both sides settle at the same moment.

## Purpose

Make cross-border payments from South Africa to its neighbours faster, cheaper and safer for the businesses that move the money. The first target is the South Africa → Zimbabwe corridor.

## The problem

- **Sending money into Africa is expensive.** Sub-Saharan Africa is the most expensive region in the world to send money to, at around 8–9% of the amount sent on average. Some South Africa → Zimbabwe services charge up to 9%.
- **The corridor is large.** Formal transfers from South Africa to Zimbabwe reached about R11.8bn in 2024, and South Africa is Zimbabwe's largest source of remittances.
- **Money transfer businesses tie up cash.** To pay out quickly, they keep money waiting in advance with partners in the destination country, often for days. They also buy dollars through banks and middlemen, one partner at a time.
- **Settlement is slow and risky.** When two businesses swap currencies, one usually pays first and trusts the other to pay back. If the second payment fails, the first business is exposed.
- **New rules are coming.** South Africa's draft cross-border crypto rules require licensed providers to report every cross-border crypto transfer. Businesses need a clear, checkable record of each trade.

## Our approach

Corridor uses stablecoins on Solana (digital tokens worth exactly one rand or one dollar) and a program that enforces three rules:

1. **Only approved businesses can trade.** A compliance authority registers each business after it passes identity checks (KYB). Anonymous wallets cannot use the program.
2. **Both sides pay at the same time, or neither does.** The rand and dollar transfers happen in one transaction. If either transfer fails, the whole trade is cancelled and nobody is left exposed.
3. **Every trade leaves a record.** The program emits an event with the parties, amounts and time, which the business can use for its reporting.

## How it works

1. The deployer sets up the program with the allowed token pair (ZARSC / USDC) and the compliance authority.
2. The compliance authority registers the operator (the money-transfer business) and the market maker (the trading partner).
3. The operator and the market maker agree a price and both sign one settlement transaction.
4. The program checks that both are registered, that the tokens are ZARSC and USDC, and that each receiving account belongs to the right party.
5. ZARSC moves from the operator to the market maker, and USDC moves from the market maker to the operator, in one atomic step.
6. The program emits a settlement event recording the trade.

## What's in the proof of concept

| Instruction | What it does | Who signs |
| --- | --- | --- |
| `initialize_config` | Stores the allowed token pair and the compliance authority | Deployer |
| `register_participant` | Admits an approved business as an operator (taker) or market maker | Compliance authority |
| `settle_quote` | Checks the trade and swaps ZARSC for USDC atomically | Operator and market maker |

Program accounts:

- `Config` (seeds: `["config"]`): the allowed token pair and the compliance authority.
- `Participant` (seeds: `["participant", wallet]`): one per approved business, recording its role.

## Not in the proof of concept (roadmap)

- Off-chain signed quotes, so the market maker does not need to be online for every trade
- Sending USDC straight to a payout partner in Zimbabwe
- Per-trade and daily limits, pausing, and participant suspension
- Routing trades across other venues (for example Jupiter or Circle StableFX)
- Automatic compliance reporting to the South African Reserve Bank (FinSurv)

## Built with

Solana · Anchor · SPL Token
