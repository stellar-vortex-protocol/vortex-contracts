# Reference Indexer

`reference-indexer.js` is a dependency-light reference implementation of the
Vortex `intent_settlement` indexer. It reconstructs the full on-chain state of
the contract from emitted events alone (zero contract reads), then prints a
JSON snapshot of every indexed intent and solver to stdout.

## Usage

```bash
node indexer/reference-indexer.js
```

Configuration is via environment variables:

| Variable       | Default                 | Description                                          |
|----------------|-------------------------|------------------------------------------------------|
| `CONTRACT_ID`  | `CEXAMPLECONTRACT`      | Soroban contract ID to index                         |
| `START_LEDGER` | `0`                     | Ledger to start from when no cursor exists yet       |
| `CURSOR_FILE`  | `.indexer-cursor.json`  | Path to the cursor file (see below)                  |

## Cursor

To avoid re-fetching the full ledger range on every run, the indexer persists
the highest processed ledger to a small JSON cursor file (`.indexer-cursor.json`
in the current working directory by default). On the next run it resumes from
that ledger instead of starting over at `START_LEDGER`. Set `CURSOR_FILE` to
store the cursor at a different path.

The cursor file has the shape:

```json
{
  "lastProcessedLedger": 310
}
```

Delete the cursor file to force a full re-index from `START_LEDGER`.
