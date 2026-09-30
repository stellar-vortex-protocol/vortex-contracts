#!/usr/bin/env node

/**
 * Vortex Protocol — Reference Solver Bot
 *
 * A runnable example demonstrating the full operational loop:
 * 1. Startup eligibility check (is_solver_eligible)
 * 2. Poll for open intents (list_open_intents)
 * 3. Accept an intent (accept_intent)
 * 4. Fill the intent (fill_intent)
 *
 * This is a simplified example for integration testing against testnet.
 * Production solvers should:
 * - Use event streaming instead of polling
 * - Implement multi-chain liquidity management
 * - Add comprehensive error handling and retry logic
 * - Track solver bond health and rebalance proactively
 * - Implement dispute resolution handling
 *
 * Usage:
 *   VORTEX_CONTRACT_ID=<CONTRACT_ID> \
 *   SOLVER_ADDRESS=<SOLVER_ADDRESS> \
 *   SOLVER_SECRET_KEY=<SOLVER_SECRET_KEY> \
 *   HORIZON_URL=<HORIZON_URL> \
 *   node examples/reference-solver-bot.js
 */

const {
  Keypair,
  Server,
  TransactionBuilder,
  Networks,
  Address,
  xdr,
  Memo,
  Operation,
  BASE_FEE,
} = require('stellar-sdk');

// Configuration from environment
const CONTRACT_ID = process.env.VORTEX_CONTRACT_ID;
const SOLVER_ADDRESS = process.env.SOLVER_ADDRESS;
const SOLVER_SECRET_KEY = process.env.SOLVER_SECRET_KEY;
const HORIZON_URL = process.env.HORIZON_URL || 'https://horizon-testnet.stellar.org';
const NETWORK = process.env.NETWORK || 'testnet';
const RPC_URL = process.env.RPC_URL || 'https://soroban-testnet.stellar.org';

// Validate environment
function validateEnv() {
  const required = ['VORTEX_CONTRACT_ID', 'SOLVER_ADDRESS', 'SOLVER_SECRET_KEY'];
  const missing = required.filter((key) => !process.env[key]);

  if (missing.length > 0) {
    console.error(`Error: Missing required environment variables: ${missing.join(', ')}`);
    process.exit(1);
  }
}

// Initialize Horizon and Soroban clients
function initializeClients() {
  const horizon = new Server(HORIZON_URL);
  return { horizon };
}

// Step 1: Check if the solver is eligible to participate
async function checkSolverEligibility(horizon) {
  console.log('\n--- Step 1: Checking Solver Eligibility ---');

  try {
    // For this example, we'll simulate a check
    // In production, you'd call is_solver_eligible via Soroban RPC
    console.log(`Solver: ${SOLVER_ADDRESS}`);
    console.log(`Contract: ${CONTRACT_ID}`);

    console.log(
      '✓ Solver eligibility check passed (requires actual RPC call in production)'
    );
    return true;
  } catch (error) {
    console.error('✗ Solver eligibility check failed:', error.message);
    return false;
  }
}

// Step 2: Poll for open intents
async function pollForOpenIntents() {
  console.log('\n--- Step 2: Polling for Open Intents ---');

  try {
    // In production, this would call list_open_intents via Soroban RPC
    console.log(`Checking for open intents on contract ${CONTRACT_ID}...`);

    // Simulated response
    const openIntents = [
      {
        id: 'intent_001',
        user: 'GUSER...',
        srcChain: 'ethereum',
        srcToken: '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', // WETH
        srcAmount: '1000000000000000000', // 1 ETH in wei
        dstToken: 'CUSDC...',
        minDstAmount: '3500000000', // 3500 USDC
        deadline: Math.floor(Date.now() / 1000) + 1800,
      },
    ];

    console.log(`Found ${openIntents.length} open intent(s)`);
    openIntents.forEach((intent) => {
      console.log(`  - Intent ${intent.id}: ${intent.srcChain} → Stellar`);
    });

    return openIntents;
  } catch (error) {
    console.error('✗ Failed to poll for intents:', error.message);
    return [];
  }
}

// Step 3: Accept an intent
async function acceptIntent(intent, horizon) {
  console.log('\n--- Step 3: Accepting Intent ---');

  try {
    const intentId = intent.id;
    console.log(`Accepting intent: ${intentId}`);

    // In production, you would:
    // 1. Validate the intent (check user, token, amounts)
    // 2. Confirm you have liquidity on the source chain
    // 3. Build and submit the accept_intent transaction via stellar-cli or SDK
    //
    // Example with Stellar CLI:
    // stellar contract invoke \
    //   --id <CONTRACT_ID> \
    //   --source <SOLVER_SECRET_KEY> \
    //   --network testnet \
    //   -- accept_intent \
    //   --solver <SOLVER_ADDRESS> \
    //   --intent_id <INTENT_ID>

    console.log('✓ Intent accepted (requires actual transaction in production)');
    return {
      ...intent,
      accepted: true,
      acceptedAt: Date.now(),
      fillDeadline: Math.floor(Date.now() / 1000) + 300, // 5-minute fill window
    };
  } catch (error) {
    console.error('✗ Failed to accept intent:', error.message);
    return null;
  }
}

// Step 4: Fill an accepted intent
async function fillIntent(acceptedIntent, horizon) {
  console.log('\n--- Step 4: Filling Intent ---');

  try {
    console.log(`Filling intent: ${acceptedIntent.id}`);

    // In production, you would:
    // 1. Execute the cross-chain swap on the source chain
    //    (e.g. Uniswap on Ethereum for the above example)
    // 2. Wait for confirmation
    // 3. Transfer the destination tokens to the intent's user
    // 4. Submit the fill_intent transaction
    //
    // Example with Stellar CLI:
    // stellar contract invoke \
    //   --id <CONTRACT_ID> \
    //   --source <SOLVER_SECRET_KEY> \
    //   --network testnet \
    //   -- fill_intent \
    //   --solver <SOLVER_ADDRESS> \
    //   --intent_id <INTENT_ID> \
    //   --fill_amount <FILL_AMOUNT>

    const fillAmount = acceptedIntent.minDstAmount; // In production, usually more
    console.log(`  Source chain: ${acceptedIntent.srcChain}`);
    console.log(`  Source token: ${acceptedIntent.srcToken}`);
    console.log(`  Source amount: ${acceptedIntent.srcAmount}`);
    console.log(`  Fill amount: ${fillAmount}`);
    console.log('✓ Intent filled (requires actual transaction in production)');

    return {
      ...acceptedIntent,
      filled: true,
      filledAt: Date.now(),
      fillAmount,
    };
  } catch (error) {
    console.error('✗ Failed to fill intent:', error.message);
    return null;
  }
}

// Main loop: demonstrate the full operational cycle
async function main() {
  console.log('🚀 Vortex Protocol — Reference Solver Bot');
  console.log('=========================================\n');

  validateEnv();

  const { horizon } = initializeClients();

  try {
    // 1. Check eligibility
    const isEligible = await checkSolverEligibility(horizon);
    if (!isEligible) {
      console.error('❌ Solver is not eligible. Register first with register_solver.');
      process.exit(1);
    }

    // 2. Poll for open intents
    const openIntents = await pollForOpenIntents();
    if (openIntents.length === 0) {
      console.log('⏸️  No open intents found. Waiting for new intents...');
      return;
    }

    // 3. Accept the first intent
    const intentToAccept = openIntents[0];
    const acceptedIntent = await acceptIntent(intentToAccept, horizon);
    if (!acceptedIntent) {
      console.error('❌ Failed to accept intent.');
      process.exit(1);
    }

    // 4. Fill the accepted intent
    const filledIntent = await fillIntent(acceptedIntent, horizon);
    if (!filledIntent) {
      console.error('❌ Failed to fill intent.');
      process.exit(1);
    }

    // Summary
    console.log('\n--- Summary ---');
    console.log(`✓ Successfully completed operational loop`);
    console.log(`  Intent: ${filledIntent.id}`);
    console.log(`  Source chain: ${filledIntent.srcChain}`);
    console.log(`  Filled amount: ${filledIntent.fillAmount}`);
    console.log('\nℹ️  This is a simplified example. Production solvers should:');
    console.log('  - Use event streaming instead of polling');
    console.log('  - Implement comprehensive multi-chain liquidity management');
    console.log('  - Add retry logic and error recovery');
    console.log('  - Monitor bond health and handle slashes');
    console.log('  - Implement dispute resolution handling');
  } catch (error) {
    console.error('❌ Unexpected error:', error);
    process.exit(1);
  }
}

// Run the bot if executed directly
if (require.main === module) {
  main().catch((error) => {
    console.error('Fatal error:', error);
    process.exit(1);
  });
}

module.exports = { checkSolverEligibility, pollForOpenIntents, acceptIntent, fillIntent };
