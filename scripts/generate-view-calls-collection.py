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


def extract_view_functions(lib_rs_path):
    """Extract all pub fn get_*/is_*/list_* from lib.rs."""
    with open(lib_rs_path, 'r') as f:
        content = f.read()

    # Match "pub fn <name>(" where name starts with get_, is_, or list_
    pattern = r'pub\s+fn\s+((get_|is_|list_)\w+)\s*\('
    matches = re.findall(pattern, content)

    # Extract just the function names and sort
    functions = sorted(list(set([m[0] for m in matches])))
    return functions


def load_collection(collection_path):
    """Load the Postman collection."""
    with open(collection_path, 'r') as f:
        return json.load(f)


def save_collection(collection_path, collection):
    """Save the Postman collection."""
    with open(collection_path, 'w') as f:
        json.dump(collection, f, indent=2)


def create_request_template(func_name):
    """Create a Postman request template for a view function."""
    # Derive variable name from function name
    var_name = f"{func_name}_tx_xdr"

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
        "name": func_name,
        "request": {
            "method": "POST",
            "header": [{"key": "Content-Type", "value": "application/json"}],
            "url": "{{rpc_url}}",
            "body": {
                "mode": "raw",
                "raw": f'{{\n  "jsonrpc": "2.0",\n  "id": "{func_name}",\n  "method": "simulateTransaction",\n  "params": {{ "transaction": "{{{{{var_name}}}}}" }}\n}}'
            },
            "description": description
        }
    }


def update_collection(collection_path, view_functions):
    """Update collection with missing view functions."""
    collection = load_collection(collection_path)

    # Get existing function names
    existing_names = {item['name'] for item in collection['item']}

    # Find missing functions
    missing = [f for f in view_functions if f not in existing_names]

    print(f"Found {len(existing_names)} existing requests in collection")
    print(f"Found {len(view_functions)} total view functions in contract")
    print(f"Adding {len(missing)} missing functions...")

    # Add missing requests
    for func_name in missing:
        request = create_request_template(func_name)
        collection['item'].append(request)
        print(f"  + {func_name}")

    # Sort items by name for consistency
    collection['item'] = sorted(collection['item'], key=lambda x: x['name'])

    save_collection(collection_path, collection)
    print(f"\nUpdated collection saved to {collection_path}")

    return missing


def update_markdown(markdown_path, view_functions):
    """Update markdown documentation with complete view function list."""
    # Read current markdown
    with open(markdown_path, 'r') as f:
        content = f.read()

    # Replace the function list section
    function_list = "\n".join([f"- `{func}`" for func in sorted(view_functions)])

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
    lib_rs = repo_root / "intent_settlement" / "src" / "lib.rs"
    collection_path = repo_root / "examples" / "view-calls.postman_collection.json"
    markdown_path = repo_root / "examples" / "view-calls.md"

    if not lib_rs.exists():
        print(f"Error: {lib_rs} not found", file=sys.stderr)
        sys.exit(1)

    if not collection_path.exists():
        print(f"Error: {collection_path} not found", file=sys.stderr)
        sys.exit(1)

    # Extract view functions
    print(f"Extracting view functions from {lib_rs}...")
    view_functions = extract_view_functions(str(lib_rs))
    print(f"Found {len(view_functions)} view functions")

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
