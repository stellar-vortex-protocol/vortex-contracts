#!/usr/bin/env python3
"""
Issue #290: Generate updated Postman collection and markdown doc with all view functions.

This script:
1. Extracts all read-only view functions from intent_settlement/src/lib.rs
2. Creates collection entries for missing functions
3. Updates examples/view-calls.postman_collection.json
4. Updates examples/view-calls.md with complete function list
"""

import json
import re
import sys
from pathlib import Path


# Read-only view-function naming conventions used across the workspace.
# The first group is the classic get_/is_/list_ prefix; the rest are
# additional read-only accessors used by the newer contracts.
VIEW_FUNCTION_PATTERN = re.compile(
    r'pub\s+fn\s+('
    r'(?:get_|is_|list_)\w+'
    r'|pending_executable_at'
    r'|balance_of'
    r'|vested_amount'
    r'|claimable'
    r'|tier_for'
    r')\s*\('
)


def extract_view_functions(lib_rs_path):
    """Extract all read-only view functions from a contract's lib.rs."""
    with open(lib_rs_path, 'r') as f:
        content = f.read()

    matches = VIEW_FUNCTION_PATTERN.findall(content)

    # Extract just the function names and sort
    functions = sorted(list(set(matches)))
    return functions


def extract_all_view_functions(contracts):
    """Extract view functions for every (contract_name, lib_rs_path) pair.

    Returns a list of (contract_name, func_name) tuples, sorted by
    contract then function name.
    """
    results = []
    for contract_name, lib_rs_path in contracts:
        path = Path(lib_rs_path)
        if not path.exists():
            print(f"Warning: {path} not found, skipping {contract_name}", file=sys.stderr)
            continue
        funcs = extract_view_functions(str(path))
        print(f"  {contract_name}: {len(funcs)} view functions")
        for func_name in funcs:
            results.append((contract_name, func_name))
    return sorted(results)


def load_collection(collection_path):
    """Load the Postman collection."""
    with open(collection_path, 'r') as f:
        return json.load(f)


def save_collection(collection_path, collection):
    """Save the Postman collection."""
    with open(collection_path, 'w') as f:
        json.dump(collection, f, indent=2)


def create_request_template(contract_name, func_name):
    """Create a Postman request template for a view function."""
    # Derive variable name from contract + function name
    var_name = f"{contract_name}_{func_name}_tx_xdr"
    entry_name = f"{contract_name}_{func_name}"

    # Build description with example command (simplified)
    description = f"Build XDR for: stellar contract invoke --id {{{{contract_id}}}} --source {{{{source_account}}}} --network testnet -- {func_name}"

    # Add common parameters for functions that take arguments
    if func_name in ['get_intent', 'get_solver', 'get_reputation_score', 'get_solver_intents',
                     'get_token_stats', 'get_solver_routes', 'get_solver_bond', 'get_solver_bonds',
                     'get_bond_token_min', 'is_allowed_bond_token', 'is_dst_token_allowed',
                     'get_min_bond_multiplier', 'is_src_chain_allowed', 'list_intents_by_user',
                     'list_solvers', 'get_best_bid', 'get_effective_intent_state']:
        # These take parameters - adjust description
        if func_name == 'get_intent' or func_name == 'get_effective_intent_state' or func_name == 'get_best_bid':
            description = description.replace('-- ' + func_name, f"-- {func_name} --intent_id {{{{intent_id}}}}")
        elif func_name in ['get_solver', 'get_reputation_score', 'is_allowed_bond_token',
                          'get_solver_intents', 'get_solver_routes', 'is_dst_token_allowed',
                          'get_min_bond_multiplier']:
            description = description.replace('-- ' + func_name, f"-- {func_name} --solver {{{{solver_address}}}}")
        elif func_name in ['get_solver_bond', 'get_bond_token_min']:
            description = description.replace('-- ' + func_name, f"-- {func_name} --solver {{{{solver_address}}}} --token {{{{token_address}}}}")
        elif func_name == 'get_solver_bonds':
            description = description.replace('-- ' + func_name, f"-- {func_name} --solver {{{{solver_address}}}}")
        elif func_name == 'get_token_stats':
            description = description.replace('-- ' + func_name, f"-- {func_name} --token {{{{token_address}}}}")
        elif func_name == 'list_intents_by_user':
            description = description.replace('-- ' + func_name, f"-- {func_name} --user {{{{user_address}}}}")
        elif func_name == 'list_solvers':
            description = description.replace('-- ' + func_name, f"-- {func_name} --start {{{{start}}}} --limit {{{{limit}}}}")

    return {
        "name": entry_name,
        "request": {
            "method": "POST",
            "header": [{"key": "Content-Type", "value": "application/json"}],
            "url": "{{rpc_url}}",
            "body": {
                "mode": "raw",
                "raw": f'{{\n  "jsonrpc": "2.0",\n  "id": "{entry_name}",\n  "method": "simulateTransaction",\n  "params": {{ "transaction": "{{{{{var_name}}}}}" }}\n}}'
            },
            "description": description
        }
    }


def update_collection(collection_path, view_functions):
    """Update collection with missing view functions.

    ``view_functions`` is a list of (contract_name, func_name) tuples.
    """
    collection = load_collection(collection_path)

    # Get existing entry names
    existing_names = {item['name'] for item in collection['item']}

    # Find missing functions
    missing = [
        (contract_name, func_name)
        for contract_name, func_name in view_functions
        if f"{contract_name}_{func_name}" not in existing_names
    ]

    print(f"Found {len(existing_names)} existing requests in collection")
    print(f"Found {len(view_functions)} total view functions across contracts")
    print(f"Adding {len(missing)} missing functions...")

    # Add missing requests
    for contract_name, func_name in missing:
        request = create_request_template(contract_name, func_name)
        collection['item'].append(request)
        print(f"  + {contract_name}.{func_name}")

    # Sort items by name for consistency
    collection['item'] = sorted(collection['item'], key=lambda x: x['name'])

    save_collection(collection_path, collection)
    print(f"\nUpdated collection saved to {collection_path}")

    return missing


def update_markdown(markdown_path, view_functions):
    """Update markdown documentation with complete view function list.

    ``view_functions`` is a list of (contract_name, func_name) tuples.
    """
    # Read current markdown
    with open(markdown_path, 'r') as f:
        content = f.read()

    # Replace the function list section, grouped by contract
    function_list = "\n".join(
        [f"- `{contract_name}.{func}`" for contract_name, func in view_functions]
    )

    new_content = re.sub(
        r'(## Import\n\n`view-calls\.postman_collection\.json` contains one example Stellar RPC\n`simulateTransaction` request for every read-only view in\n`intent_settlement/src/lib\.rs`:\n\n)(.*?)(\n\n## Import)',
        f'\\1{function_list}\\3',
        content,
        count=1,
        flags=re.DOTALL
    )

    # If the above didn't match, try a different pattern
    if new_content == content:
        # Try to find and replace the list directly
        list_start = content.find("- `get_protocol_params`")
        list_end = content.find("\n\n##", list_start)
        if list_start != -1 and list_end != -1:
            before = content[:list_start]
            after = content[list_end:]
            new_content = before + function_list + after

    with open(markdown_path, 'w') as f:
        f.write(new_content)

    print(f"Updated markdown saved to {markdown_path}")


def main():
    repo_root = Path(__file__).parent.parent
    collection_path = repo_root / "examples" / "view-calls.postman_collection.json"
    markdown_path = repo_root / "examples" / "view-calls.md"

    # All contracts whose read-only view functions are documented.
    contracts = [
        ("intent_settlement", repo_root / "intent_settlement" / "src" / "lib.rs"),
        ("governor", repo_root / "governor" / "src" / "lib.rs"),
        ("backstop_vault", repo_root / "backstop_vault" / "src" / "lib.rs"),
        ("fee_router", repo_root / "fee_router" / "src" / "lib.rs"),
        ("treasury_streams", repo_root / "treasury_streams" / "src" / "lib.rs"),
        ("intent_periphery", repo_root / "intent_periphery" / "src" / "lib.rs"),
    ]

    if not collection_path.exists():
        print(f"Error: {collection_path} not found", file=sys.stderr)
        sys.exit(1)

    # Extract view functions across all contracts
    print("Extracting view functions...")
    view_functions = extract_all_view_functions(contracts)
    print(f"Found {len(view_functions)} view functions across {len(contracts)} contracts")

    # Update collection
    missing = update_collection(str(collection_path), view_functions)

    # Update markdown
    update_markdown(str(markdown_path), view_functions)

    if missing:
        print(f"\n✓ Added {len(missing)} missing view functions to collection and markdown")
        return 0
    else:
        print("\n✓ Collection is already in sync")
        return 0


if __name__ == "__main__":
    sys.exit(main())
