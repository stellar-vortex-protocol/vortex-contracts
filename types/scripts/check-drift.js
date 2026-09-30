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
const TS_FILE = path.resolve(__dirname, "../src/index.ts");

const checks = [];
let hasErrors = false;

/**
 * Check if a pattern exists in the Rust file
 */
function checkRustPattern(pattern, description) {
  const content = fs.readFileSync(RUST_FILE, "utf-8");
  const regex = typeof pattern === "string" ? new RegExp(pattern) : pattern;

  if (!regex.test(content)) {
    console.error(`❌ DRIFT: ${description}`);
    console.error(`   Pattern not found in ${RUST_FILE}`);
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
function countRustPattern(pattern) {
  const content = fs.readFileSync(RUST_FILE, "utf-8");
  const regex = typeof pattern === "string" ? new RegExp(pattern, "g") : pattern;
  const matches = content.match(regex);
  return matches ? matches.length : 0;
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

// Summary
console.log("\n" + "─".repeat(60));
if (hasErrors) {
  console.error("❌ Type definition drift detected!");
  process.exit(1);
} else {
  console.log("✅ No type definition drift detected.");
  console.log(
    "TypeScript types match Rust struct/enum definitions."
  );
  process.exit(0);
}
