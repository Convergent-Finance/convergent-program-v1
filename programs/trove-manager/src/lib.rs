use anchor_lang::prelude::{borsh::BorshDeserialize, *};

declare_id!("CVGT8oK6mDzdr3cfXABSGZLuhAgLY5LBz1qpvQiCmwqQ");

mod constants;
mod errors;
mod events;
mod instructions;
mod math;
mod state;
mod utils;

use instructions::*;

#[program]
pub mod trove_manager {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        mcr: u64,
        ccr: u64,
        min_net_debt: u64,
        gas_compensation: u64,
        coll_gas_comp_percent_divisor: u64,
    ) -> Result<()> {
        initialize_handler(
            ctx,
            mcr,
            ccr,
            min_net_debt,
            gas_compensation,
            coll_gas_comp_percent_divisor,
        )
    }

    pub fn initialize_trove(ctx: Context<InitializeTrove>) -> Result<()> {
        initialize_trove_handler(ctx)
    }

    pub fn initialize_price_feed(ctx: Context<InitializePriceFeed>, is_dev: bool) -> Result<()> {
        initialize_price_feed_handler(ctx, is_dev)
    }

    #[allow(unused_variables)]
    pub fn initialize_current_epoch_scale(ctx: Context<InitializeEpochScale>) -> Result<()> {
        Ok(())
    }

    pub fn dev_change_price(ctx: Context<DevChangePrice>, new_price: u64) -> Result<()> {
        dev_change_price_handler(ctx, new_price)
    }

    pub fn open_trove(
        ctx: Context<OpenTrove>,
        max_fee_percentage: u64,
        is_lamport: bool,
        coll_amt: u64,
        usv_amt: u64,
    ) -> Result<()> {
        open_trove_handler(ctx, max_fee_percentage, is_lamport, coll_amt, usv_amt)
    }

    pub fn add_coll(
        ctx: Context<AdjustTrove>,
        max_fee_percentage: u64,
        is_lamport: bool,
        coll_amt: u64,
    ) -> Result<()> {
        adjust_trove_handler(
            ctx,
            max_fee_percentage,
            is_lamport,
            coll_amt,
            true,
            0,
            false,
        )
    }

    pub fn withdraw_coll(
        ctx: Context<AdjustTrove>,
        max_fee_percentage: u64,
        coll_amt: u64,
    ) -> Result<()> {
        adjust_trove_handler(ctx, max_fee_percentage, false, coll_amt, false, 0, false)
    }

    pub fn withdraw_usv(
        ctx: Context<AdjustTrove>,
        max_fee_percentage: u64,
        usv_amt: u64,
    ) -> Result<()> {
        adjust_trove_handler(ctx, max_fee_percentage, false, 0, false, usv_amt, true)
    }

    pub fn repay_usv(
        ctx: Context<AdjustTrove>,
        max_fee_percentage: u64,
        usv_amt: u64,
    ) -> Result<()> {
        adjust_trove_handler(ctx, max_fee_percentage, false, 0, false, usv_amt, false)
    }

    pub fn adjust_trove(
        ctx: Context<AdjustTrove>,
        max_fee_percentage: u64,
        is_lamport: bool,
        coll_change: u64,
        is_coll_increase: bool,
        usv_change: u64,
        is_debt_increase: bool,
    ) -> Result<()> {
        adjust_trove_handler(
            ctx,
            max_fee_percentage,
            is_lamport,
            coll_change,
            is_coll_increase,
            usv_change,
            is_debt_increase,
        )
    }

    pub fn close_trove(ctx: Context<CloseTrove>) -> Result<()> {
        close_trove_handler(ctx)
    }

    pub fn liquidate_trove(ctx: Context<LiquidateTrove>) -> Result<()> {
        liquidate_trove_handler(ctx)
    }

    pub fn batch_liquidate_troves(ctx: Context<BatchLiquidateTroves>) -> Result<()> {
        batch_liquidate_troves_handler(ctx)
    }

    pub fn redeem_collateral(
        ctx: Context<RedeemCollateral>,
        max_fee_percentage: u64,
        usv_amt: u64,
    ) -> Result<()> {
        redeem_collateral_handler(ctx, max_fee_percentage, usv_amt)
    }

    pub fn claim_coll_surplus(ctx: Context<ClaimCollSurplus>) -> Result<()> {
        claim_coll_surplus_handler(ctx)
    }

    // Stability Pool
    pub fn provide_to_sp(ctx: Context<ProvideToSP>, usv_amt: u64) -> Result<()> {
        provide_to_sp_handler(ctx, usv_amt)
    }

    pub fn withdraw_from_sp(ctx: Context<WithdrawFromSP>, usv_amt: u64) -> Result<()> {
        withdraw_from_sp_handler(ctx, usv_amt)
    }

    pub fn claim_from_sp(ctx: Context<ClaimFromSP>) -> Result<()> {
        claim_from_sp_handler(ctx)
    }

    // Admin
    pub fn config_pool_state(ctx: Context<ConfigPoolState>) -> Result<()> {
        config_pool_state_handler(ctx)
    }

    pub fn fetch_price(ctx: Context<FetchPrice>) -> Result<()> {
        fetch_price_handler(ctx)
    }

    // Community Issuance
    pub fn initialize_community_issuance(
        ctx: Context<InitializeCommunityIssuance>,
        emission_rate: u64,
        is_dev: bool,
    ) -> Result<()> {
        initialize_community_issuance_handler(ctx, emission_rate, is_dev)
    }

    pub fn change_authority(ctx: Context<ChangeAuthority>) -> Result<()> {
        change_authority_handler(ctx)
    }

    pub fn change_emission_rate(ctx: Context<ChangeEmissionRate>, new_rate: u64) -> Result<()> {
        change_emission_rate_handler(ctx, new_rate)
    }

    pub fn enable_emission(ctx: Context<EnableEmission>) -> Result<()> {
        enable_emission_handler(ctx)
    }

    pub fn dev_set_timestamp(ctx: Context<SetTimestamp>, new_timestamp: u64) -> Result<()> {
        set_timestamp_handler(ctx, new_timestamp)
    }

    // CVGT Staking
    pub fn initialize_cvgt_staking(ctx: Context<InitializeCVGTStaking>) -> Result<()> {
        initialize_cvgt_staking_handler(ctx)
    }

    pub fn stake(ctx: Context<Stake>, cvgt_amt: u64) -> Result<()> {
        stake_handler(ctx, cvgt_amt)
    }

    pub fn unstake(ctx: Context<Unstake>, cvgt_amt: u64) -> Result<()> {
        unstake_handler(ctx, cvgt_amt)
    }
}
