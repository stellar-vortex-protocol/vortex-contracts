# Supported Source Chains and Token Address Formats

**Issue:** [#132](https://github.com/stellar-vortex-protocol/vortex-contracts/issues/132)  
**Branch:** `docs/supported-chains-table`  
**Status:** Living document — update when the `add_allowed_src_chain` allowlist is populated for each deployment

---

## 1. Overview

Vortex intents have a `src_chain` field (a plain string, e.g. `"ethereum"`) and
a `src_token` field (the token's address on that chain). Off-chain tooling must
use the canonical strings listed here, or `SrcChainAllowlistEnabled` will reject
the intent when enforcement is active.

The destination chain is always **Stellar** — `dst_token` is a Stellar SAC or
SEP-41 address. This document covers the source-chain side only.

---

## 2. Canonical `src_chain` Strings

These are the values the contract recognises via `add_allowed_src_chain()`:

| `src_chain` value | Network | Chain type | Wormhole chain ID | Status |
|---|---|---|---|---|
| `"ethereum"` | Ethereum Mainnet | EVM | 2 | Supported |
| `"base"` | Base Mainnet | EVM (L2, Coinbase) | 30 | Supported |
| `"polygon"` | Polygon PoS | EVM | 5 | Supported |
| `"arbitrum"` | Arbitrum One | EVM (L2, Offchain Labs) | 23 | Supported |
| `"optimism"` | OP Mainnet | EVM (L2, Optimism) | 24 | Supported |
| `"avalanche"` | Avalanche C-Chain | EVM | 6 | Supported |
| `"bsc"` | BNB Smart Chain | EVM | 4 | Supported |
| `"solana"` | Solana Mainnet Beta | SVM | 1 | Supported |

> **Case-sensitive.** The contract stores and compares these strings literally.
> `"Ethereum"` and `"ETHEREUM"` are not the same as `"ethereum"`.

> **On-chain `src_token` format validation** (`validate_src_token`, #127) runs
> for `"ethereum"`, `"base"`, `"polygon"`, `"arbitrum"`, `"optimism"` (EVM
> rules) and `"solana"` (base58 rules). `"avalanche"` and `"bsc"` are accepted
> as source chains but their `src_token` is **not** format-checked on-chain yet
> — off-chain tooling must validate those itself. Adding them to the validator
> is tracked separately.

---

## 3. Source Token Address Formats by Chain

### 3.1 EVM Chains (Ethereum, Base, Polygon, Arbitrum, Optimism, Avalanche, BSC)

EVM token addresses are 20-byte hex strings prefixed with `0x`, checksummed
per [EIP-55](https://eips.ethereum.org/EIPS/eip-55). Vortex accepts both
checksummed and lowercase variants (the contract stores them as strings and
does not validate checksum on-chain; off-chain tooling should normalise to
checksummed form for human readability).

**Format:**
```
0x<40 hex digits>
```

**Example CLI usage:**
```bash
--src_chain '"ethereum"' \
--src_token '"0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"'
```

> Note the escaped inner quotes — the Stellar CLI requires string arguments to
> be wrapped in `'"…"'`.

### 3.2 Solana

A Solana token is identified by its **SPL mint address**: the base58 encoding
of a 32-byte ed25519 public key (a `solana_program::pubkey::Pubkey`). This is
the same string wallets and explorers display for a token.

**Format:**
```
<base58 string, 32–44 characters, no "0x" prefix>
```

**On-chain validation rules** (`validate_src_token` for `src_chain = "solana"`):

| Rule | Value | Why |
|---|---|---|
| Length | 32–44 characters inclusive | A 32-byte value base58-encodes to at most 44 digits; a leading-zero-byte key can be as short as 32. Real mints observed are 43–44. |
| Alphabet | Bitcoin/IPFS base58: `123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz` | Standard base58; **excludes `0` (zero), `O`, `I`, `l`** to avoid visual ambiguity. |
| No `0x` prefix | rejected | Solana has no `0x` convention; a `0x…` string is an EVM address submitted against the wrong chain. |

A value that breaks any rule makes `submit_intent` fail with
`Error::InvalidSrcToken` (28).

> **Verification source.** The 32-byte key size and base58 rendering are from
> the Solana SDK (`solana_program::pubkey::Pubkey`, `bs58` crate — Bitcoin
> alphabet). The 32–44 character bound and the sample mints in §4.8 were
> checked against Solana Explorer / the Solana token list.

**Example CLI usage:**
```bash
--src_chain '"solana"' \
--src_token '"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"'   # USDC (SPL), 44 chars
```

> **Decimals differ from EVM.** SPL token decimals are set per mint and are
> **not** uniformly 6 or 18. USDC and USDT are 6, but wrapped SOL is 9 and many
> project tokens use other values. Always read the mint's `decimals` field
> (e.g. `getTokenSupply` / the mint account) rather than assuming — see §4.8
> and the [Decimal Normalization](../README.md#decimal-normalization-for-src_amount)
> table in the README (which now has a Solana row).

---

## 4. Common Token Addresses by Chain

The table below lists the most commonly used source tokens. Always verify
addresses against the official project sources before using in production —
token contracts can be migrated or deprecated.

### Ethereum

| Token | Contract address | Decimals |
|---|---|---|
| WETH (Wrapped ETH) | `0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2` | 18 |
| USDC | `0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48` | 6 |
| USDT | `0xdAC17F958D2ee523a2206206994597C13D831ec7` | 6 |
| WBTC | `0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599` | 8 |
| DAI | `0x6B175474E89094C44Da98b954EedeAC495271d0F` | 18 |

### Base

| Token | Contract address | Decimals |
|---|---|---|
| WETH | `0x4200000000000000000000000000000000000006` | 18 |
| USDC | `0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913` | 6 |
| cbETH | `0x2Ae3F1Ec7F1F5012CFEab0185bfc7aa3cf0DEc22` | 18 |

### Polygon

| Token | Contract address | Decimals |
|---|---|---|
| WMATIC | `0x0d500B1d8E8eF31E21C99d1Db9A6444d3ADf1270` | 18 |
| USDC.e (bridged) | `0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174` | 6 |
| USDC (native) | `0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359` | 6 |
| WETH | `0x7ceB23fD6bC0adD59E62ac25578270cFf1b9f619` | 18 |

### Arbitrum One

| Token | Contract address | Decimals |
|---|---|---|
| WETH | `0x82aF49447D8a07e3bd95BD0d56f35241523fBab1` | 18 |
| USDC | `0xaf88d065e77c8cC2239327C5EDb3A432268e5831` | 6 |
| USDC.e (bridged) | `0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8` | 6 |
| ARB | `0x912CE59144191C1204E64559FE8253a0e49E6548` | 18 |

### OP Mainnet (Optimism)

| Token | Contract address | Decimals |
|---|---|---|
| WETH | `0x4200000000000000000000000000000000000006` | 18 |
| USDC | `0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85` | 6 |
| USDC.e (bridged) | `0x7F5c764cBc14f9669B88837ca1490cCa17c31607` | 6 |
| OP | `0x4200000000000000000000000000000000000042` | 18 |

### Avalanche C-Chain

| Token | Contract address | Decimals |
|---|---|---|
| WAVAX | `0xB31f66AA3C1e785363F0875A1B74E27b85FD66c7` | 18 |
| USDC | `0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E` | 6 |
| USDT.e | `0xc7198437980c041c805A1EDcbA50c1Ce5db95118` | 6 |

### BNB Smart Chain

| Token | Contract address | Decimals |
|---|---|---|
| WBNB | `0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c` | 18 |
| USDT | `0x55d398326f99059fF775485246999027B3197955` | 18 |
| USDC | `0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d` | 18 |
| BTCB | `0x7130d2A12B9BCbFAe4f2634d864A1Ee1Ce3Ead9c` | 18 |

> **BSC stablecoin pitfall:** USDT and USDC on BSC use **18 decimals**, not 6.
> See [Decimal Normalization](../README.md#decimal-normalization-for-src_amount)
> in the README for the full worked-example table.

### 4.8 Solana (SPL mints)

Addresses are base58 SPL mint addresses. Unlike EVM, **decimals vary widely per
mint** — do not assume 6 or 18.

| Token | Mint address | Decimals |
|---|---|---|
| USDC | `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v` | 6 |
| USDT | `Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB` | 6 |
| Wrapped SOL (wSOL) | `So11111111111111111111111111111111111111112` | 9 |
| JitoSOL | `J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn` | 9 |
| BONK | `DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263` | 5 |

> **Solana decimals pitfall:** wSOL and most LSTs are **9 decimals**, BONK is
> **5**, stablecoins are **6**. `src_amount = human_amount × 10^decimals` still
> holds, but `decimals` must be read from the mint account per token. Verify
> mint addresses against Solana Explorer before production use.

---

## 5. Allowlist Management

The contract's `src_chain` allowlist is off by default. To enforce it:

```bash
# Add each supported chain
stellar contract invoke --id <CONTRACT_ID> --source <ADMIN_SECRET> --network testnet -- \
  add_allowed_src_chain --chain '"ethereum"'

stellar contract invoke --id <CONTRACT_ID> --source <ADMIN_SECRET> --network testnet -- \
  add_allowed_src_chain --chain '"base"'

# (repeat for each chain in §2)

# Enable enforcement
stellar contract invoke --id <CONTRACT_ID> --source <ADMIN_SECRET> --network testnet -- \
  set_src_chain_allowlist_enabled --enabled true
```

To remove a chain (e.g., if it's deprecated):

```bash
stellar contract invoke --id <CONTRACT_ID> --source <ADMIN_SECRET> --network testnet -- \
  remove_allowed_src_chain --chain '"optimism"'
```

After removal, any new `submit_intent` call with `src_chain = "optimism"` will
fail with `Error::SrcChainNotAllowed`. Existing already-accepted intents are
unaffected.

---

## 6. Adding a New Chain

To add support for a new source chain:

1. Choose a lowercase `src_chain` string (e.g. `"scroll"`).
2. Identify its Wormhole chain ID (see [Wormhole chain IDs](https://docs.wormhole.com/wormhole/reference/constants)).
3. Add the mapping to the chain-ID lookup table in `fill_intent`'s proof
   validation block (see [#129](./129-proof-mismatch-fallback.md) §4).
4. Call `add_allowed_src_chain()` on the deployed contract.
5. Update this document with the new row in §2 and token addresses in §4.
6. Deploy and verify the source-chain `VortexDeposit` contract (see
   [#124](./124-proof-verification-interface.md) §5).

---

## 7. Relationship to Proof Verification

When `fill_intent` runs proof validation (Phase 2 and Phase 3 of the rollout
in [#124](./124-proof-verification-interface.md)), it maps `intent.src_chain`
to a Wormhole chain ID and compares it against `proof.src_chain_id`. The
mapping table lives in §4 of [129-proof-mismatch-fallback.md](./129-proof-mismatch-fallback.md)
and must be kept in sync with the canonical strings listed in §2 of this
document.

**Implemented** (issue #253): `IntentSettlement::src_chain_to_wormhole_id` in
`intent_settlement/src/lib.rs` is the single source of truth for this mapping.
An unmapped `src_chain` string fails closed with `Error::SrcChainNotSupported`.

---

*Closes #132*
