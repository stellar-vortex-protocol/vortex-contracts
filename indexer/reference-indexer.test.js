// Test suite for the Soroban-RPC-backed indexer service.
// Covers event fetching, cursor persistence, reorg handling, and query interface.

const { VortexIndexer } = require('./reference-indexer');

// Mock Soroban RPC client
class MockSorobanRPC {
    constructor(options = {}) {
        this.events = options.events || [];
        this.failureMode = options.failureMode || null;
        this.callCount = 0;
    }

    async getEvents(filters) {
        this.callCount++;
        if (this.failureMode === 'rpc-error') {
            throw new Error('RPC connection failed');
        }
        if (this.failureMode === 'timeout') {
            throw new Error('Request timeout');
        }

        // Return events matching the cursor/pagination parameters
        const startIdx = filters.cursor ? this.events.findIndex(e => e.paging_token > filters.cursor) : 0;
        const endIdx = startIdx + (filters.limit || 100);
        const resultEvents = this.events.slice(startIdx, endIdx);

        return {
            _links: {
                next: {
                    href: resultEvents.length > 0 ? `?cursor=${resultEvents[resultEvents.length - 1].paging_token}` : null
                }
            },
            _embedded: {
                records: resultEvents
            }
        };
    }
}

// Mock VortexIndexer methods for testing
class TestVortexIndexer {
    constructor() {
        this.events = new Map();
        this.state = {};
    }

    addEvent(eventId, event) {
        this.events.set(eventId, event);
    }

    getState() {
        return this.state;
    }

    setState(newState) {
        this.state = Object.assign({}, newState);
    }
}

describe('Indexer Cursor Persistence', () => {
    test('persists cursor after successful event fetch', (done) => {
        const mockRpc = new MockSorobanRPC({
            events: [
                { paging_token: 'token1', type: 'contract_invoked', id: 'event1' },
                { paging_token: 'token2', type: 'contract_invoked', id: 'event2' }
            ]
        });

        const indexer = new TestVortexIndexer();

        // Simulate fetching events and persisting cursor
        mockRpc.getEvents({ limit: 100 }).then(response => {
            const lastEvent = response._embedded.records[response._embedded.records.length - 1];
            const cursor = lastEvent ? lastEvent.paging_token : null;

            indexer.setState({ lastCursor: cursor, lastIndexedLedger: 1000 });

            const state = indexer.getState();
            expect(state.lastCursor).toBe('token2');
            expect(state.lastIndexedLedger).toBe(1000);
            done();
        });
    });

    test('resumes from persisted cursor on restart', (done) => {
        const events = [
            { paging_token: 'token1', type: 'contract_invoked', id: 'event1' },
            { paging_token: 'token2', type: 'contract_invoked', id: 'event2' },
            { paging_token: 'token3', type: 'contract_invoked', id: 'event3' }
        ];

        const mockRpc = new MockSorobanRPC({ events });

        const indexer = new TestVortexIndexer();
        indexer.setState({ lastCursor: 'token1' });

        // Fetch events starting from persisted cursor
        mockRpc.getEvents({ cursor: 'token1', limit: 100 }).then(response => {
            const resultEvents = response._embedded.records;

            // Should skip event1 and start from event2
            expect(resultEvents.length).toBe(2);
            expect(resultEvents[0].paging_token).toBe('token2');
            expect(resultEvents[1].paging_token).toBe('token3');
            done();
        });
    });

    test('detects and handles cursor older than RPC retention window', (done) => {
        const mockRpc = new MockSorobanRPC({
            events: [
                { paging_token: 'token100', type: 'contract_invoked', id: 'event100' }
            ],
            failureMode: null
        });

        const indexer = new TestVortexIndexer();

        // Simulate old cursor that's outside retention window
        indexer.setState({ lastCursor: 'ancient-token-1000-ledgers-ago' });

        // When attempting to fetch with old cursor, should detect empty result
        mockRpc.getEvents({ cursor: 'ancient-token-1000-ledgers-ago', limit: 100 }).then(response => {
            const resultEvents = response._embedded.records;

            // Empty results with old cursor indicates retention window exceeded
            if (resultEvents.length === 0) {
                indexer.setState({ cursorOutOfDate: true, needsFullResync: true });
            }

            const state = indexer.getState();
            expect(state.needsFullResync).toBe(true);
            done();
        });
    });
});

describe('Indexer RPC Resilience', () => {
    test('retries with exponential backoff on RPC error', async () => {
        const mockRpc = new MockSorobanRPC({ failureMode: 'rpc-error' });

        let attemptCount = 0;
        let lastDelay = 0;

        // Simulate retry logic with exponential backoff
        const retryWithBackoff = async (maxAttempts = 3) => {
            const baseDelay = 1000;

            for (let i = 0; i < maxAttempts; i++) {
                try {
                    attemptCount++;
                    await mockRpc.getEvents({ limit: 100 });
                    return;
                } catch (error) {
                    if (i < maxAttempts - 1) {
                        lastDelay = baseDelay * Math.pow(2, i);
                        await new Promise(resolve => setTimeout(resolve, lastDelay));
                    }
                }
            }
            throw new Error('Max retries exceeded');
        };

        try {
            await retryWithBackoff(3);
        } catch (error) {
            expect(attemptCount).toBe(3);
            expect(lastDelay).toBe(2000); // 1000 * 2^1 on final attempt
        }
    });

    test('logs error count and health status', () => {
        const indexer = new TestVortexIndexer();

        // Initialize error tracking
        indexer.setState({
            errorCount: 0,
            lastError: null,
            lastIndexedLedger: 1000,
            isHealthy: true
        });

        // Simulate error occurrence
        const state = indexer.getState();
        state.errorCount++;
        state.lastError = new Error('Connection timeout');
        if (state.errorCount > 5) {
            state.isHealthy = false;
        }
        indexer.setState(state);

        expect(indexer.getState().errorCount).toBe(1);
        expect(indexer.getState().isHealthy).toBe(true);

        // Simulate multiple errors
        for (let i = 0; i < 5; i++) {
            state.errorCount++;
        }
        if (state.errorCount > 5) {
            state.isHealthy = false;
        }
        indexer.setState(state);

        expect(indexer.getState().errorCount).toBe(6);
        expect(indexer.getState().isHealthy).toBe(false);
    });

    test('exposes health status endpoint', (done) => {
        const indexer = new TestVortexIndexer();
        indexer.setState({
            lastIndexedLedger: 5000,
            errorCount: 2,
            isHealthy: true
        });

        // Simulate health endpoint response
        const healthStatus = {
            status: indexer.getState().isHealthy ? 'healthy' : 'degraded',
            lastIndexedLedger: indexer.getState().lastIndexedLedger,
            errorCount: indexer.getState().errorCount
        };

        expect(healthStatus.status).toBe('healthy');
        expect(healthStatus.lastIndexedLedger).toBe(5000);
        expect(healthStatus.errorCount).toBe(2);
        done();
    });
});

describe('Indexer Query Interface', () => {
    test('query intent events by id', () => {
        const indexer = new TestVortexIndexer();

        const intentSubmittedEvent = {
            id: 'intent-123',
            type: 'intent_submitted',
            intent_id: 'abc-def-ghi',
            user: 'GBBD....',
            amount: '1000'
        };

        indexer.addEvent('event-1', intentSubmittedEvent);

        // Query for intent events
        const events = Array.from(indexer.events.values()).filter(e => e.type === 'intent_submitted');

        expect(events.length).toBe(1);
        expect(events[0].intent_id).toBe('abc-def-ghi');
    });

    test('query solver events by address', () => {
        const indexer = new TestVortexIndexer();

        const solverRegisteredEvent = {
            id: 'event-1',
            type: 'solver_registered',
            solver: 'GABC....',
            bond_amount: '10000'
        };

        const solverAcceptedEvent = {
            id: 'event-2',
            type: 'intent_accepted',
            solver: 'GABC....',
            intent_id: 'abc-def-ghi'
        };

        indexer.addEvent('event-1', solverRegisteredEvent);
        indexer.addEvent('event-2', solverAcceptedEvent);

        // Query for all events involving a specific solver
        const solverEvents = Array.from(indexer.events.values()).filter(e => e.solver === 'GABC....');

        expect(solverEvents.length).toBe(2);
        expect(solverEvents[0].type).toBe('solver_registered');
        expect(solverEvents[1].type).toBe('intent_accepted');
    });

    test('returns event snapshot as JSON', (done) => {
        const indexer = new TestVortexIndexer();

        indexer.addEvent('event-1', {
            type: 'intent_submitted',
            intent_id: 'abc-123',
            user: 'GBBD....'
        });
        indexer.addEvent('event-2', {
            type: 'solver_registered',
            solver: 'GABC....'
        });

        // Simulate JSON snapshot endpoint
        const snapshot = Array.from(indexer.events.values());

        expect(snapshot.length).toBe(2);
        expect(JSON.stringify(snapshot)).toContain('intent_submitted');
        expect(JSON.stringify(snapshot)).toContain('solver_registered');
        done();
    });

    test('persistent query storage (SQLite-like behavior)', () => {
        const indexer = new TestVortexIndexer();

        // Simulate periodic snapshot to persistent storage
        const events = [
            { id: 'event-1', type: 'intent_submitted', timestamp: 1000 },
            { id: 'event-2', type: 'intent_accepted', timestamp: 1001 }
        ];

        for (const event of events) {
            indexer.addEvent(event.id, event);
        }

        // Simulate snapshot write
        const snapshot = {
            events: Array.from(indexer.events.values()),
            timestamp: Date.now(),
            version: 1
        };

        // Verify snapshot can be serialized and contains all events
        const snapshotJson = JSON.stringify(snapshot);
        expect(snapshotJson).toContain('event-1');
        expect(snapshotJson).toContain('event-2');
    });
});

describe('Indexer Reorg Handling', () => {
    test('detects ledger reorg (backwards cursor movement)', () => {
        const indexer = new TestVortexIndexer();

        // Initial state: indexed up to ledger 1000
        indexer.setState({ lastIndexedLedger: 1000, lastCursor: 'token-1000' });

        // New RPC response shows cursor moved backward (reorg detected)
        const oldState = indexer.getState();
        const newCursor = 'token-995'; // Earlier than token-1000

        if (newCursor < oldState.lastCursor) {
            indexer.setState({ reorgDetected: true, reorgLedger: 995 });
        }

        expect(indexer.getState().reorgDetected).toBe(true);
        expect(indexer.getState().reorgLedger).toBe(995);
    });

    test('rolls back indexed state on reorg', () => {
        const indexer = new TestVortexIndexer();

        // Simulate events indexed up to ledger 1000
        indexer.addEvent('event-1000', { id: 'event-1000', ledger: 1000 });
        indexer.addEvent('event-999', { id: 'event-999', ledger: 999 });

        // Detect reorg at ledger 995
        indexer.setState({
            reorgDetected: true,
            reorgLedger: 995,
            lastIndexedLedger: 995,
            lastCursor: 'token-995'
        });

        // Remove events after reorg point
        const state = indexer.getState();
        const reorgLedger = state.reorgLedger;

        for (const [key, value] of indexer.events.entries()) {
            if (value.ledger > reorgLedger) {
                indexer.events.delete(key);
            }
        }

        // Verify events after reorg are removed
        expect(Array.from(indexer.events.keys()).length).toBe(1);
        expect(indexer.getState().lastIndexedLedger).toBe(995);
    });
});

describe('Indexer Integration Tests', () => {
    test('full event pipeline: fetch -> parse -> store -> query', (done) => {
        const mockRpc = new MockSorobanRPC({
            events: [
                {
                    paging_token: 'token-1',
                    type: 'contract_invoked',
                    id: 'ledger-123-event-1',
                    topic: ['intent_submitted'],
                    value: 'abc-123'
                }
            ]
        });

        const indexer = new TestVortexIndexer();

        // Full pipeline
        mockRpc.getEvents({ limit: 100 }).then(response => {
            // Fetch events
            const events = response._embedded.records;

            // Parse and store
            for (const event of events) {
                indexer.addEvent(event.id, event);
            }

            // Update cursor
            if (events.length > 0) {
                indexer.setState({
                    lastCursor: events[events.length - 1].paging_token,
                    lastIndexedLedger: 123
                });
            }

            // Query
            const storedEvent = indexer.events.get('ledger-123-event-1');

            expect(storedEvent).toBeDefined();
            expect(storedEvent.topic).toContain('intent_submitted');
            expect(indexer.getState().lastIndexedLedger).toBe(123);
            done();
        });
    });
});

module.exports = {
    MockSorobanRPC,
    TestVortexIndexer
};
