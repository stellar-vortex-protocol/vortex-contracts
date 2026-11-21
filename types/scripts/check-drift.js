#!/usr/bin/env node

/**
 * check-drift.js
 *
 * Verifies that TypeScript type definitions in src/index.ts match the Rust
 * struct/enum definitions in intent_settlement/src/lib.rs.
 *
 * Issue #138: Drift detection flags when Rust types change without the TS
 * types being updated, ensuring a single source of truth.
 *
 * Usage:
 *   npm run check-drift
 */

const fs = require("fs");
const path = require("path");

const RUST_FILE = path.resolve(
  __dirname,
  "../../intent_settlement/src/lib.rs"
);

// Additional contract crates whose Rust definitions must stay in sync with
// types/src/index.ts. Add `{ name, rustFile, patterns }` entries here.
const EXTRA_TARGETS = [
  {
    name: "solver_registry",
    rustFile: path.resolve(__dirname, "../../solver_registry/src/lib.rs"),
    patterns: [
      [/pub struct SolverRecord\s*\{/, "SolverRecord struct"],
      ...[
        "address",
        "bond_amount",
        "fills_completed",
        "fills_failed",
        "total_volume",
        "registered_at",
        "last_slash_time",
      ].map((f) => [new RegExp(`pub ${f}:`), `SolverRecord.${f}`]),
    ],
  },
];
const TS_FILE = path.resolve(__dirname, "../src/index.ts");

// Rust sources for the five newest contracts (issue #482).
const GOVERNOR_RUST_FILE = path.resolve(
  __dirname,
  "../../governor/src/lib.rs"
);
const BACKSTOP_VAULT_RUST_FILE = path.resolve(
  __dirname,
  "../../backstop_vault/src/lib.rs"
);
const FEE_ROUTER_RUST_FILE = path.resolve(
  __dirname,
  "../../fee_router/src/lib.rs"
);
const TREASURY_STREAMS_RUST_FILE = path.resolve(
  __dirname,
  "../../treasury_streams/src/lib.rs"
);
const INTENT_PERIPHERY_RUST_FILE = path.resolve(
  __dirname,
  "../../intent_periphery/src/lib.rs"
);

const checks = [];
let hasErrors = false;

/**
 * Check if a pattern exists in the Rust file
 */
function checkRustPattern(pattern, description, rustFile = RUST_FILE) {
  const content = fs.readFileSync(rustFile, "utf-8");
  const regex = typeof pattern === "string" ? new RegExp(pattern) : pattern;

  if (!regex.test(content)) {
    console.error(`❌ DRIFT: ${description}`);
    console.error(`   Pattern not found in ${rustFile}`);
    hasErrors = true;
  } else {
    console.log(`✓ ${description}`);
  }
}

/**
 * Check if a pattern exists in the TypeScript file
 */
function checkTsPattern(pattern, description) {
  const content = fs.readFileSync(TS_FILE, "utf-8");
  const regex = typeof pattern === "string" ? new RegExp(pattern) : pattern;

  if (!regex.test(content)) {
    console.error(`❌ DRIFT: ${description}`);
    console.error(`   Pattern not found in ${TS_FILE}`);
    hasErrors = true;
  } else {
    console.log(`✓ ${description}`);
  }
}

/**
 * Extract enum variants from TypeScript
 */
function extractTsEnumVariants(enumName) {
  const content = fs.readFileSync(TS_FILE, "utf-8");
  const pattern = new RegExp(
    `export enum ${enumName}\\s*\\{([^}]*)\\}`,
    "s"
  );
  const match = content.match(pattern);
  if (!match) return [];

  return match[1]
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("//"))
    .map((line) => line.split("=")[0].trim());
}

/**
 * Count occurrences of a pattern in Rust
 */
function countRustPattern(pattern, rustFile = RUST_FILE) {
  const content = fs.readFileSync(rustFile, "utf-8");
  const regex = typeof pattern === "string" ? new RegExp(pattern, "g") : pattern;
  const matches = content.match(regex);
  return matches ? matches.length : 0;
}

/**
 * Check that a Rust #[contracttype] struct exists in the given Rust file and
 * that a matching TypeScript interface exists in src/index.ts.
 */
function checkContractStruct(rustFile, structName, tsName = structName) {
  checkRustPattern(
    new RegExp(`pub struct ${structName}\\s*\\{`),
    `${structName} struct in Rust`,
    rustFile
  );
  checkTsPattern(
    new RegExp(`export interface ${tsName}\\b`),
    `${tsName} interface in TS`
  );
}

/**
 * Check that a Rust #[contracttype] enum exists in the given Rust file and
 * that a matching TypeScript enum exists in src/index.ts.
 */
function checkContractEnum(rustFile, enumName, tsName = enumName) {
  checkRustPattern(
    new RegExp(`pub enum ${enumName}\\s*\\{`),
    `${enumName} enum in Rust`,
    rustFile
  );
  checkTsPattern(
    new RegExp(`export enum ${tsName}\\b`),
    `${tsName} enum in TS`
  );
}

/**
 * Check that each field of a Rust struct is present in the Rust source and
 * has a corresponding property in the TypeScript interface.
 */
function checkStructFields(rustFile, structName, fields) {
  fields.forEach((field) => {
    checkRustPattern(
      new RegExp(`pub ${field}:`),
      `${structName}.${field} in Rust`,
      rustFile
    );
    checkTsPattern(
      new RegExp(`${field}[:\\?]`),
      `${structName}.${field} in TS`
    );
  });
}

/**
 * Check that each variant of a Rust enum is present in the Rust source and in
 * the corresponding TypeScript enum.
 */
function checkEnumVariants(rustFile, enumName, variants, tsName = enumName) {
  const tsVariants = extractTsEnumVariants(tsName);
  variants.forEach((variant) => {
    checkRustPattern(
      new RegExp(`\\b${variant}\\b`),
      `${enumName}.${variant} in Rust`,
      rustFile
    );
    if (!tsVariants.includes(variant)) {
      console.error(`❌ DRIFT: ${tsName}.${variant} missing in TS`);
      hasErrors = true;
    } else {
      console.log(`✓ ${tsName}.${variant}`);
    }
  });
}

console.log("🔍 Checking TypeScript/Rust type definition drift...\n");

// Check for main struct definitions
console.log("Struct Definitions:");
checkRustPattern(/pub struct IntentRecord\s*\{/, "IntentRecord struct in Rust");
checkTsPattern(/export interface IntentRecord/, "IntentRecord interface in TS");

checkRustPattern(
  /pub struct SolverRecord\s*\{/,
  "SolverRecord struct in Rust"
);
checkTsPattern(/export interface SolverRecord/, "SolverRecord interface in TS");

checkRustPattern(
  /pub struct ReputationSnapshot\s*\{/,
  "ReputationSnapshot struct in Rust"
);
checkTsPattern(
  /export interface ReputationSnapshot/,
  "ReputationSnapshot interface in TS"
);

// Check for main enum definitions
console.log("\nEnum Definitions:");
checkRustPattern(
  /pub enum IntentState\s*\{/,
  "IntentState enum in Rust"
);
checkTsPattern(/export enum IntentState/, "IntentState enum in TS");

checkRustPattern(
  /pub enum DisputeResolution\s*\{/,
  "DisputeResolution enum in Rust"
);
checkTsPattern(
  /export enum DisputeResolution/,
  "DisputeResolution enum in TS"
);

checkRustPattern(/pub enum Error\s*\{/, "Error enum in Rust");
checkTsPattern(/export enum ErrorCode/, "ErrorCode enum in TS");

// Check for key fields in IntentRecord
console.log("\nIntentRecord Fields:");
const intentFields = [
  "intent_id",
  "user",
  "src_chain",
  "src_token",
  "src_amount",
  "dst_token",
  "min_dst_amount",
  "solver",
  "state",
  "created_at",
  "deadline",
  "bond_token",
  "total_filled",
  "solver_tier",
];

intentFields.forEach((field) => {
  checkRustPattern(
    new RegExp(`pub ${field}:`),
    `IntentRecord.${field} in Rust`
  );
  checkTsPattern(
    new RegExp(`${field}[:\\?]`),
    `IntentRecord.${field} in TS`
  );
});

// Check for key fields in SolverRecord
console.log("\nSolverRecord Fields:");
const solverFields = [
  "address",
  "bond_amount",
  "fills_completed",
  "fills_failed",
  "total_volume",
  "is_active",
  "registered_at",
  "active_intents",
  "last_slash_time",
  "bond_tokens",
];

solverFields.forEach((field) => {
  checkRustPattern(
    new RegExp(`pub ${field}:`),
    `SolverRecord.${field} in Rust`
  );
  checkTsPattern(
    new RegExp(`${field}[:\\?]`),
    `SolverRecord.${field} in TS`
  );
});

// Check IntentState variants
console.log("\nIntentState Variants:");
const tsIntentStateVariants = extractTsEnumVariants("IntentState");
const requiredIntentStates = [
  "Open",
  "Accepted",
  "PartiallyFilled",
  "Filled",
  "Cancelled",
  "Expired",
  "Slashed",
  "Bidding",
  "Filling",
  "Disputed",
  "Resolved",
];

requiredIntentStates.forEach((variant) => {
  if (!tsIntentStateVariants.includes(variant)) {
    console.error(`❌ DRIFT: IntentState.${variant} missing in TS`);
    hasErrors = true;
  } else {
    console.log(`✓ IntentState.${variant}`);
  }
});

// Check Error variants (sample of important ones)
console.log("\nError Code Variants (sample):");
const importantErrors = [
  "AlreadyInitialized",
  "Unauthorized",
  "IntentNotFound",
  "SolverNotRegistered",
  "SolverBondTooLow",
  "SolverHasActiveIntents",
  "NotInitialized",
];

importantErrors.forEach((error) => {
  checkRustPattern(
    new RegExp(`${error}\\s*=\\s*\\d+`),
    `Error::${error} in Rust`
  );
  checkTsPattern(
    new RegExp(`${error}\\s*=\\s*\\d+`),
    `ErrorCode.${error} in TS`
  );
});

// Additional crates
EXTRA_TARGETS.forEach(({ name, rustFile, patterns }) => {
  console.log(`\n${name}:`);
  patterns.forEach(([pattern, description]) => {
    checkRustPattern(pattern, `${description} in Rust (${name})`, rustFile);
    // Fields must also appear in the TS definitions
    if (!description.includes("struct")) {
      const field = description.split(".").pop();
      checkTsPattern(new RegExp(`${field}[:\\?]`), `${description} in TS (${name})`);
    }
  });
});

// ---------------------------------------------------------------------------
// Governor (issue #482)
// ---------------------------------------------------------------------------
console.log("\nGovernor:");
checkContractStruct(GOVERNOR_RUST_FILE, "Proposal");
checkContractStruct(GOVERNOR_RUST_FILE, "GovernorConfig");
checkContractEnum(GOVERNOR_RUST_FILE, "ProposalState");
checkContractEnum(GOVERNOR_RUST_FILE, "VoteType");

console.log("\nGovernor.Proposal Fields:");
checkStructFields(GOVERNOR_RUST_FILE, "Proposal", [
  "id",
  "proposer",
  "description_hash",
  "start_time",
  "end_time",
  "for_votes",
  "against_votes",
  "abstain_votes",
  "executed",
  "cancelled",
]);

console.log("\nGovernor.GovernorConfig Fields:");
checkStructFields(GOVERNOR_RUST_FILE, "GovernorConfig", [
  "voting_delay",
  "voting_period",
  "quorum",
  "proposal_threshold",
  "timelock_delay",
]);

console.log("\nGovernor.ProposalState Variants:");
checkEnumVariants(GOVERNOR_RUST_FILE, "ProposalState", [
  "Pending",
  "Active",
  "Defeated",
  "Succeeded",
  "Queued",
  "Executed",
  "Cancelled",
]);

console.log("\nGovernor.VoteType Variants:");
checkEnumVariants(GOVERNOR_RUST_FILE, "VoteType", [
  "For",
  "Against",
  "Abstain",
]);

// ---------------------------------------------------------------------------
// Backstop Vault (issue #482)
// ---------------------------------------------------------------------------
console.log("\nBackstop Vault:");
checkContractStruct(BACKSTOP_VAULT_RUST_FILE, "VaultConfig");
checkContractStruct(BACKSTOP_VAULT_RUST_FILE, "DepositRecord");

console.log("\nBackstopVault.VaultConfig Fields:");
checkStructFields(BACKSTOP_VAULT_RUST_FILE, "VaultConfig", [
  "token",
  "cooldown_period",
  "min_deposit",
  "capacity",
]);

console.log("\nBackstopVault.DepositRecord Fields:");
checkStructFields(BACKSTOP_VAULT_RUST_FILE, "DepositRecord", [
  "depositor",
  "amount",
  "deposited_at",
  "unlock_at",
]);

// ---------------------------------------------------------------------------
// Fee Router (issue #482)
// ---------------------------------------------------------------------------
console.log("\nFee Router:");
checkContractStruct(FEE_ROUTER_RUST_FILE, "Sink");
checkContractStruct(FEE_ROUTER_RUST_FILE, "FeeConfig");

console.log("\nFeeRouter.Sink Fields:");
checkStructFields(FEE_ROUTER_RUST_FILE, "Sink", [
  "recipient",
  "share_bps",
]);

console.log("\nFeeRouter.FeeConfig Fields:");
checkStructFields(FEE_ROUTER_RUST_FILE, "FeeConfig", [
  "token",
  "total_bps",
  "sinks",
]);

// ---------------------------------------------------------------------------
// Treasury Streams (issue #482)
// ---------------------------------------------------------------------------
console.log("\nTreasury Streams:");
checkContractStruct(TREASURY_STREAMS_RUST_FILE, "Stream");

console.log("\nTreasuryStreams.Stream Fields:");
checkStructFields(TREASURY_STREAMS_RUST_FILE, "Stream", [
  "id",
  "recipient",
  "token",
  "rate_per_ledger",
  "start_ledger",
  "end_ledger",
  "withdrawn",
  "cancelled",
]);

// ---------------------------------------------------------------------------
// Intent Periphery (issue #482)
// ---------------------------------------------------------------------------
console.log("\nIntent Periphery:");
checkContractStruct(INTENT_PERIPHERY_RUST_FILE, "SwapParams");
checkContractStruct(INTENT_PERIPHERY_RUST_FILE, "SwapResult");

console.log("\nIntentPeriphery.SwapParams Fields:");
checkStructFields(INTENT_PERIPHERY_RUST_FILE, "SwapParams", [
  "src_token",
  "dst_token",
  "src_amount",
  "min_dst_amount",
  "deadline",
  "recipient",
]);

console.log("\nIntentPeriphery.SwapResult Fields:");
checkStructFields(INTENT_PERIPHERY_RUST_FILE, "SwapResult", [
  "intent_id",
  "dst_amount",
  "solver",
]);

// Every interface/enum exported from src/index.ts. Adding or removing a type
// requires updating this list, so removals are never silently accepted.
const EXPECTED_TS_TYPES = [
  "IntentState",
  "DisputeResolution",
  "IntentRecord",
  "SolverRecord",
  "ReputationSnapshot",
  "ErrorCode",
  "Checkpoint",
  "VoteType",
  "ProposalState",
  "Proposal",
  "GovernorConfig",
  "GovernorError",
  "Stream",
  "TreasuryStreamsDataKey",
  "TreasuryStreamsError",
  "Sink",
  "FeeRouterConfig",
  "FeeRouterError",
  "BackstopPosition",
  "BackstopVaultConfig",
  "BackstopVaultError",
  "BatchIntent",
  "BatchResult",
  "IntentPeripheryError",
];

// Compare the exported TS type names against EXPECTED_TS_TYPES, naming each
// missing or unlisted type.
function checkTsTypeSet() {
  const content = fs.readFileSync(TS_FILE, "utf-8");
  const actual = [...content.matchAll(/^export (?:interface|enum) (\w+)/gm)].map(
    (m) => m[1]
  );
  EXPECTED_TS_TYPES.forEach((name) => {
    if (!actual.includes(name)) {
      console.error(`❌ DRIFT: TS type ${name} missing from ${TS_FILE}`);
      hasErrors = true;
    }
  });
  actual.forEach((name) => {
    if (!EXPECTED_TS_TYPES.includes(name)) {
      console.error(`❌ DRIFT: TS type ${name} in ${TS_FILE} is not in EXPECTED_TS_TYPES`);
      hasErrors = true;
    }
  });
}
checkTsTypeSet();

// Summary
console.log("\n" + "─".repeat(60));
if (hasErrors) {
  console.error("❌ Type definition drift detected!");
  console.error(
    "TypeScript types do not match Rust struct/enum definitions."
  );
  process.exit(1);
} else {
  console.log("✅ No type definition drift detected.");
  console.log(
    "TypeScript types match Rust struct/enum definitions."
  );
  process.exit(0);
}
