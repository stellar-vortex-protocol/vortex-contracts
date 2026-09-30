/**
 * Basic Usage Example
 *
 * Demonstrates how to use the Vortex SDK for common operations:
 * - Initialize the client
 * - Check solver eligibility
 * - Query intents
 * - Submit and accept intents
 * - Fill intents
 */

import { VortexClient, SubmitIntentOptions, AcceptIntentOptions, FillIntentOptions } from '../src';
import { Server, Keypair, Networks } from 'stellar-sdk';

async function main() {
  // Initialize Stellar clients
  const horizon = new Server('https://horizon-testnet.stellar.org');

  // Create SDK client
  const client = new VortexClient({
    contractId: 'CAKJ2RNHMBT74VJLGVLZ6SCOBRQMDQ6PU5CCYKP4IJMVKFAACJKBRMM',
    horizon,
    rpcUrl: 'https://soroban-testnet.stellar.org',
    networkPassphrase: Networks.TESTNET_NETWORK_PASSPHRASE,
  });

  // Load keypairs from environment
  const userKeypair = Keypair.fromSecret(process.env.USER_SECRET_KEY!);
  const solverKeypair = Keypair.fromSecret(process.env.SOLVER_SECRET_KEY!);

  console.log('=== Vortex Protocol SDK Example ===\n');

  // 1. Check if solver is eligible
  console.log('1. Checking solver eligibility...');
  const isEligible = await client.isSolverEligible(solverKeypair.publicKey());
  console.log(`   Solver eligible: ${isEligible}\n`);

  // 2. List open intents
  console.log('2. Listing open intents...');
  const openIntents = await client.listOpenIntents({ limit: 5 });
  console.log(`   Found ${openIntents.length} open intents\n`);

  // 3. Get protocol stats
  console.log('3. Fetching protocol health...');
  const health = await client.getProtocolHealth();
  console.log(`   Total intents: ${health.total_intents}`);
  console.log(`   Total solvers: ${health.total_solvers}`);
  console.log(`   Total TVL: ${health.total_tvl}\n`);

  // 4. Submit an intent (user operation)
  console.log('4. Submitting intent...');
  const submitOptions: SubmitIntentOptions = {
    user: userKeypair.publicKey(),
    src_chain: 'ethereum',
    src_token: '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2', // WETH on Ethereum
    src_amount: 1_000_000_000_000_000_000n, // 1 ETH in wei
    dst_token: 'CUSDC...', // USDC on Stellar (placeholder)
    min_dst_amount: 3_500_000_000n, // 3500 USDC minimum
  };

  try {
    // Note: This will throw in current reference implementation
    const intentId = await client.submitIntent(submitOptions, userKeypair);
    console.log(`   Intent submitted: ${intentId}\n`);

    // 5. Solver accepts the intent
    console.log('5. Accepting intent...');
    const acceptOptions: AcceptIntentOptions = {
      solver: solverKeypair.publicKey(),
      intent_id: intentId,
    };

    await client.acceptIntent(acceptOptions, solverKeypair);
    console.log(`   Intent accepted\n`);

    // 6. Solver fills the intent
    console.log('6. Filling intent...');
    const fillOptions: FillIntentOptions = {
      solver: solverKeypair.publicKey(),
      intent_id: intentId,
      fill_amount: 3_500_000_000n, // Deliver minimum amount
    };

    await client.fillIntent(fillOptions, solverKeypair);
    console.log(`   Intent filled\n`);

    // 7. Query the filled intent
    console.log('7. Querying filled intent...');
    const filledIntent = await client.getIntent(intentId);
    if (filledIntent) {
      console.log(`   Intent state: ${filledIntent.state}`);
      console.log(`   Filled by: ${filledIntent.accepted_by}\n`);
    }
  } catch (error) {
    console.log(`   (Skipped - Reference implementation not yet complete)\n`);
  }

  // 8. Query solver record
  console.log('8. Querying solver record...');
  const solver = await client.getSolver(solverKeypair.publicKey());
  if (solver) {
    console.log(`   Solver bond: ${solver.bond_amount} stroops`);
    console.log(`   Fills completed: ${solver.fills_completed}`);
    console.log(`   Active intents: ${solver.active_intents}\n`);
  }

  // 9. Get fee schedule
  console.log('9. Fetching fee schedule...');
  const feeSchedule = await client.getFeeSchedule(solverKeypair.publicKey());
  console.log(`   Base fee: ${feeSchedule.base_fee_bps} bps`);
  console.log(`   Effective fee: ${feeSchedule.effective_fee_bps} bps`);
  if (feeSchedule.tiers.length > 0) {
    console.log(`   Applied tier: Volume >= ${feeSchedule.tiers[0].min_volume}\n`);
  }

  // 10. Check if contract is paused
  console.log('10. Checking contract status...');
  const isPaused = await client.isPaused();
  console.log(`    Contract paused: ${isPaused}\n`);

  console.log('=== Example Complete ===');
}

// Run example
main().catch((error) => {
  console.error('Error:', error);
  process.exit(1);
});
