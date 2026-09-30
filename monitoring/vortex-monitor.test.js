#!/usr/bin/env node
/**
 * vortex-monitor.test.js — Test suite for the monitoring & alerting tool
 *
 * Demonstrates alert detection with simulated events and polling scenarios.
 *
 * Usage:
 *   node monitoring/vortex-monitor.test.js
 */

"use strict";

// ---------------------------------------------------------------------------
// Test Suite
// ---------------------------------------------------------------------------

const tests = [];
let passCount = 0;
let failCount = 0;

function test(name, fn) {
  tests.push({ name, fn });
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(`Assertion failed: ${message}`);
  }
}

function assertEqual(actual, expected, message) {
  if (actual !== expected) {
    throw new Error(`Expected ${expected}, got ${actual}: ${message}`);
  }
}

// ---------------------------------------------------------------------------
// Test Helpers
// ---------------------------------------------------------------------------

/**
 * Mock EventListener for testing alert logic.
 */
class MockEventListener {
  constructor() {
    this.eventBuffer = {};
    this.alerts = [];
  }

  recordEventInWindow(eventType) {
    const now = Date.now();
    if (!this.eventBuffer[eventType]) {
      this.eventBuffer[eventType] = [];
    }
    this.eventBuffer[eventType].push(now);
    this.eventBuffer[eventType] = this.eventBuffer[eventType].filter(
      (ts) => now - ts < 2 * 60 * 60 * 1000
    );
  }

  getEventCountInWindow(eventType, windowMinutes) {
    if (!this.eventBuffer[eventType]) {
      return 0;
    }
    const now = Date.now();
    const windowMs = windowMinutes * 60 * 1000;
    return this.eventBuffer[eventType].filter((ts) => now - ts <= windowMs).length;
  }

  checkSlashRate(slashCountThreshold = 5, slashWindowMinutes = 10) {
    const slashCount = this.getEventCountInWindow("solver_slashed", slashWindowMinutes);
    if (slashCount >= slashCountThreshold) {
      this.alerts.push({
        severity: "P2",
        signal: "unusual_slash_rate",
        count: slashCount,
      });
      return true;
    }
    return false;
  }

  checkMassSolverExit(threshold = 3) {
    const deregCount = this.getEventCountInWindow("solver_deregistered", 60);
    if (deregCount >= threshold) {
      this.alerts.push({
        severity: "P2",
        signal: "mass_solver_exit",
        count: deregCount,
      });
      return true;
    }
    return false;
  }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test("Event buffer: record and retrieve events", () => {
  const listener = new MockEventListener();

  listener.recordEventInWindow("solver_slashed");
  listener.recordEventInWindow("solver_slashed");
  listener.recordEventInWindow("solver_slashed");

  const count = listener.getEventCountInWindow("solver_slashed", 10);
  assertEqual(count, 3, "Should record 3 slashes");
});

test("Slash rate alert: trigger when threshold exceeded", () => {
  const listener = new MockEventListener();

  // Record 6 slash events
  for (let i = 0; i < 6; i++) {
    listener.recordEventInWindow("solver_slashed");
  }

  const triggered = listener.checkSlashRate(5, 10);
  assert(triggered, "Should trigger P2 alert when slash count >= threshold");
  assertEqual(listener.alerts.length, 1, "Should have recorded one alert");
  assertEqual(listener.alerts[0].signal, "unusual_slash_rate", "Alert should be for unusual_slash_rate");
});

test("Slash rate alert: no trigger when below threshold", () => {
  const listener = new MockEventListener();

  // Record 3 slash events (below threshold of 5)
  for (let i = 0; i < 3; i++) {
    listener.recordEventInWindow("solver_slashed");
  }

  const triggered = listener.checkSlashRate(5, 10);
  assert(!triggered, "Should not trigger alert when slash count < threshold");
  assertEqual(listener.alerts.length, 0, "Should not have recorded any alert");
});

test("Mass solver exit: trigger when threshold exceeded", () => {
  const listener = new MockEventListener();

  // Record 4 deregistration events
  for (let i = 0; i < 4; i++) {
    listener.recordEventInWindow("solver_deregistered");
  }

  const triggered = listener.checkMassSolverExit(3);
  assert(triggered, "Should trigger P2 alert when deregistration count >= threshold");
  assertEqual(listener.alerts.length, 1, "Should have recorded one alert");
  assertEqual(listener.alerts[0].signal, "mass_solver_exit", "Alert should be for mass_solver_exit");
});

test("Mass solver exit: no trigger when below threshold", () => {
  const listener = new MockEventListener();

  // Record 2 deregistration events (below threshold of 3)
  for (let i = 0; i < 2; i++) {
    listener.recordEventInWindow("solver_deregistered");
  }

  const triggered = listener.checkMassSolverExit(3);
  assert(!triggered, "Should not trigger alert when deregistration count < threshold");
  assertEqual(listener.alerts.length, 0, "Should not have recorded any alert");
});

test("Event buffer: clean old entries", () => {
  const listener = new MockEventListener();

  // Manually inject an old event (3+ hours old)
  const threeHoursAgo = Date.now() - 3 * 60 * 60 * 1000;
  listener.eventBuffer["solver_slashed"] = [threeHoursAgo];

  // Record a new event
  listener.recordEventInWindow("solver_slashed");

  // The old event should be cleaned, only the new one remains
  const count = listener.getEventCountInWindow("solver_slashed", 10);
  assertEqual(count, 1, "Should only count recent events within time window");
});

test("Multiple signals: independent tracking", () => {
  const listener = new MockEventListener();

  // Record different event types
  for (let i = 0; i < 6; i++) {
    listener.recordEventInWindow("solver_slashed");
  }
  for (let i = 0; i < 2; i++) {
    listener.recordEventInWindow("solver_deregistered");
  }

  const slashCount = listener.getEventCountInWindow("solver_slashed", 10);
  const deregCount = listener.getEventCountInWindow("solver_deregistered", 10);

  assertEqual(slashCount, 6, "Slash count should be independent");
  assertEqual(deregCount, 2, "Deregistration count should be independent");
});

// ---------------------------------------------------------------------------
// Test Runner
// ---------------------------------------------------------------------------

async function runTests() {
  console.log("=== Vortex Monitor Test Suite ===\n");

  for (const { name, fn } of tests) {
    try {
      await fn();
      console.log(`✓ ${name}`);
      passCount++;
    } catch (err) {
      console.error(`✗ ${name}`);
      console.error(`  ${err.message}`);
      failCount++;
    }
  }

  console.log(`\n${passCount} passed, ${failCount} failed`);

  if (failCount > 0) {
    process.exit(1);
  }
}

runTests();
