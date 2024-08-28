use anchor_lang::prelude::*;

use solana_program::pubkey;

pub const SECOND_IN_ONE_MINUTE: u32 = 60;
pub const ONE_HUNDERED_PERCENT: u64 = 1_000_000_000;
pub const DECIMAL_PRECISION: u64 = 1_000_000_000;
pub const NICR_PRECISION: u64 = 100_000_000_000;

/// Half-life of 12h. 12h = 720 min
/// (1/2) = d^720 => d = (1/2)^(1/720)
pub const MINUTE_DECAY_FACTOR: u64 = 999_037_759;
pub const REDEMPTION_FEE_FLOOR: u64 = DECIMAL_PRECISION / 1000 * 5; // 0.5%
pub const MAX_BORROWING_FEE: u64 = DECIMAL_PRECISION / 100 * 5; // 5%
pub const BORROWING_FEE_FLOOR: u64 = DECIMAL_PRECISION / 1000 * 5; // 0.5%

pub const SCALE_FACTOR: u64 = 100_000;

// Price feed
pub const TIMEOUT: i64 = 14400;
pub const MAX_CONFIDENCE_RATE: u64 = 5_000_000; // 5%
pub const FEED_DECIMAL_PRECISION: u64 = 100_000_000;
pub const TARGET_DECIMAL_PRECISION: u64 = 1_000_000_000;
pub const MAX_PRICE_DIFFERENCE_BETWEEN_ORACLES: u64 = 5_000_000; // 5%

// Community Issuance
pub const MAX_EMISSION_RATE: u64 = 10_000_000_000;

// Deployer
pub const DEPLOYER: Pubkey = pubkey!("FeXpuNQFuEg8q5KdimHkogXiCuMKfa8PwbeYKJSbqiVo");

// Treasury
pub const TREASURY_VAULT: Pubkey = pubkey!("7QzstxNuABJa9KEGpPohQCsdkwGaggtnKY31J7LpEry8");
