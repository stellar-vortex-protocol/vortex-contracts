//! Reference Implementation: Numeric Chain ID Validation
//!
//! This module demonstrates how to add numeric chain ID validation to the contract
//! as an alternative to (or complement to) the current string-based `src_chain`.
//!
//! Usage:
//! ```
//! use numeric_chain_ids::{ChainId, WormholeId, validate_chain_id, chain_id_to_name};
//!
//! // Validate a numeric chain ID
//! if validate_chain_id(1) {
//!     println!("Chain ID 1 (Ethereum) is supported");
//! }
//!
//! // Map to Wormhole ID
//! let wormhole_id = ChainId::Ethereum.to_wormhole_id();
//! assert_eq!(wormhole_id, WormholeId::Ethereum);
//!
//! // Map to human-readable name
//! assert_eq!(chain_id_to_name(1), "ethereum");
//! ```

use soroban_sdk::{contracttype, env::panic_with_error, Env};

/// Supported blockchain networks by their standard chain IDs
///
/// Matches EVM chain IDs and Wormhole network identifiers for consistency
/// across bridges and RPC providers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChainId {
    /// Ethereum mainnet (EVM chain ID 1)
    Ethereum = 1,

    /// Solana mainnet (Wormhole chain ID 1, remapped to 900 for clarity)
    Solana = 900,

    /// Polygon mainnet (EVM chain ID 137)
    Polygon = 137,

    /// BNB Chain mainnet (EVM chain ID 56)
    BnbChain = 56,

    /// Avalanche C-Chain (EVM chain ID 43114)
    Avalanche = 43114,

    /// Base mainnet (EVM chain ID 8453)
    Base = 8453,

    /// Arbitrum One (EVM chain ID 42161)
    Arbitrum = 42161,

    /// Optimism mainnet (EVM chain ID 10)
    Optimism = 10,

    // Add more chains as needed
}

impl ChainId {
    /// Convert a raw u32 to ChainId if supported
    pub fn from_u32(id: u32) -> Option<Self> {
        match id {
            1 => Some(ChainId::Ethereum),
            900 => Some(ChainId::Solana),
            137 => Some(ChainId::Polygon),
            56 => Some(ChainId::BnbChain),
            43114 => Some(ChainId::Avalanche),
            8453 => Some(ChainId::Base),
            42161 => Some(ChainId::Arbitrum),
            10 => Some(ChainId::Optimism),
            _ => None,
        }
    }

    /// Convert to Wormhole chain ID for cross-chain proof verification
    pub fn to_wormhole_id(self) -> WormholeId {
        match self {
            ChainId::Ethereum => WormholeId::Ethereum,
            ChainId::Solana => WormholeId::Solana,
            ChainId::Polygon => WormholeId::Polygon,
            ChainId::BnbChain => WormholeId::BnbChain,
            ChainId::Avalanche => WormholeId::Avalanche,
            ChainId::Base => WormholeId::Base,
            ChainId::Arbitrum => WormholeId::Arbitrum,
            ChainId::Optimism => WormholeId::Optimism,
        }
    }

    /// Get human-readable chain name
    pub fn name(self) -> &'static str {
        match self {
            ChainId::Ethereum => "ethereum",
            ChainId::Solana => "solana",
            ChainId::Polygon => "polygon",
            ChainId::BnbChain => "bsc",
            ChainId::Avalanche => "avalanche",
            ChainId::Base => "base",
            ChainId::Arbitrum => "arbitrum",
            ChainId::Optimism => "optimism",
        }
    }
}

/// Wormhole standard chain IDs
/// Reference: https://docs.wormhole.com/wormhole/reference/glossary#chain-id
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u16)]
pub enum WormholeId {
    Solana = 1,
    Ethereum = 2,
    Polygon = 5,
    Avalanche = 6,
    BnbChain = 4,
    Arbitrum = 23,
    Optimism = 24,
    Base = 30,
}

impl WormholeId {
    /// Map Wormhole chain ID back to standard chain ID
    pub fn to_chain_id(self) -> ChainId {
        match self {
            WormholeId::Ethereum => ChainId::Ethereum,
            WormholeId::Solana => ChainId::Solana,
            WormholeId::Polygon => ChainId::Polygon,
            WormholeId::BnbChain => ChainId::BnbChain,
            WormholeId::Avalanche => ChainId::Avalanche,
            WormholeId::Base => ChainId::Base,
            WormholeId::Arbitrum => ChainId::Arbitrum,
            WormholeId::Optimism => ChainId::Optimism,
        }
    }
}

/// Validate a raw chain ID and return ChainId if supported
pub fn validate_chain_id(chain_id: u32) -> bool {
    ChainId::from_u32(chain_id).is_some()
}

/// Convert a raw chain ID to human-readable name
pub fn chain_id_to_name(chain_id: u32) -> &'static str {
    ChainId::from_u32(chain_id)
        .map(|c| c.name())
        .unwrap_or("unknown")
}

/// Convert a raw chain ID to Wormhole chain ID
pub fn chain_id_to_wormhole_id(chain_id: u32) -> Option<WormholeId> {
    ChainId::from_u32(chain_id).map(|c| c.to_wormhole_id())
}

// ─── Integration with Soroban Contract ────────────────────────────────────

/// Storage key for allowed chain IDs
#[derive(Clone)]
#[contracttype]
pub enum DataKeyChainIds {
    AllowedChainIds,        // Set<u32> of allowed chain IDs
    ChainIdAllowlistEnabled, // bool: whether to enforce the allowlist
}

/// Enable or disable the numeric chain ID allowlist
pub fn set_chain_id_allowlist_enabled(env: &Env, enabled: bool) {
    env.storage()
        .instance()
        .set(&DataKeyChainIds::ChainIdAllowlistEnabled, &enabled);
}

/// Check if numeric chain ID allowlist is enabled
pub fn is_chain_id_allowlist_enabled(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKeyChainIds::ChainIdAllowlistEnabled)
        .unwrap_or(false)
}

/// Add a chain ID to the allowlist (admin-only)
pub fn add_allowed_chain_id(env: &Env, chain_id: u32) {
    if !validate_chain_id(chain_id) {
        panic_with_error!(env, 1001i64); // InvalidChainId
    }

    let mut allowed: soroban_sdk::Vec<u32> = env
        .storage()
        .instance()
        .get(&DataKeyChainIds::AllowedChainIds)
        .unwrap_or_else(|| soroban_sdk::Vec::new(env));

    // Avoid duplicates
    if !allowed.contains(&chain_id) {
        allowed.push_back(chain_id);
    }

    env.storage()
        .instance()
        .set(&DataKeyChainIds::AllowedChainIds, &allowed);
}

/// Remove a chain ID from the allowlist (admin-only)
pub fn remove_allowed_chain_id(env: &Env, chain_id: u32) {
    let allowed: soroban_sdk::Vec<u32> = env
        .storage()
        .instance()
        .get(&DataKeyChainIds::AllowedChainIds)
        .unwrap_or_else(|| soroban_sdk::Vec::new(env));

    let filtered: soroban_sdk::Vec<u32> = allowed
        .into_iter()
        .filter(|&id| id != chain_id)
        .collect();

    env.storage()
        .instance()
        .set(&DataKeyChainIds::AllowedChainIds, &filtered);
}

/// Check if a chain ID is in the allowlist
pub fn is_chain_id_allowed(env: &Env, chain_id: u32) -> bool {
    if !is_chain_id_allowlist_enabled(env) {
        // Allowlist disabled: allow any valid chain ID
        return validate_chain_id(chain_id);
    }

    let allowed: soroban_sdk::Vec<u32> = env
        .storage()
        .instance()
        .get(&DataKeyChainIds::AllowedChainIds)
        .unwrap_or_else(|| soroban_sdk::Vec::new(env));

    allowed.contains(&chain_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_id_validation() {
        assert!(validate_chain_id(1));    // Ethereum
        assert!(validate_chain_id(8453)); // Base
        assert!(!validate_chain_id(999)); // Invalid
    }

    #[test]
    fn test_chain_id_to_name() {
        assert_eq!(chain_id_to_name(1), "ethereum");
        assert_eq!(chain_id_to_name(8453), "base");
        assert_eq!(chain_id_to_name(999), "unknown");
    }

    #[test]
    fn test_wormhole_mapping() {
        let chain_id = ChainId::Ethereum;
        let wormhole_id = chain_id.to_wormhole_id();
        assert_eq!(wormhole_id, WormholeId::Ethereum);

        let back = wormhole_id.to_chain_id();
        assert_eq!(back, ChainId::Ethereum);
    }

    #[test]
    fn test_from_u32() {
        assert_eq!(ChainId::from_u32(1), Some(ChainId::Ethereum));
        assert_eq!(ChainId::from_u32(8453), Some(ChainId::Base));
        assert_eq!(ChainId::from_u32(999), None);
    }
}

#[cfg(all(test, target_env = "env"))]
mod soroban_tests {
    use super::*;
    use soroban_sdk::env::ContractEnv;

    #[test]
    fn test_allowlist_operations() {
        let env = Env::default();

        // Initially allowlist is disabled
        assert!(!is_chain_id_allowlist_enabled(&env));
        assert!(is_chain_id_allowed(&env, 1));    // Allow any valid chain
        assert!(!is_chain_id_allowed(&env, 999)); // But reject invalid chain

        // Enable allowlist
        set_chain_id_allowlist_enabled(&env, true);
        assert!(is_chain_id_allowlist_enabled(&env));
        assert!(!is_chain_id_allowed(&env, 1)); // Now Ethereum not allowed (empty list)

        // Add Ethereum to allowlist
        add_allowed_chain_id(&env, 1);
        assert!(is_chain_id_allowed(&env, 1));
        assert!(!is_chain_id_allowed(&env, 8453)); // Base still not allowed

        // Add Base
        add_allowed_chain_id(&env, 8453);
        assert!(is_chain_id_allowed(&env, 8453));

        // Remove Ethereum
        remove_allowed_chain_id(&env, 1);
        assert!(!is_chain_id_allowed(&env, 1));
        assert!(is_chain_id_allowed(&env, 8453)); // Base still allowed
    }
}
