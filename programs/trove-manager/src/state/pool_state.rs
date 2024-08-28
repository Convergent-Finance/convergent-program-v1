use crate::{
    constants::{
        BORROWING_FEE_FLOOR, DECIMAL_PRECISION, MAX_BORROWING_FEE, MINUTE_DECAY_FACTOR,
        REDEMPTION_FEE_FLOOR, SECOND_IN_ONE_MINUTE,
    },
    errors::BorrowerOpsError,
    events::{BaseRateUpdated, LastFeeOpTimeUpdated, SystemSnapshotsUpdated},
    math::{compute_cr, dec_pow},
    utils::get_current_timestamp,
    ID,
};
use anchor_lang::prelude::*;
use std::cmp;

use super::{LiquidationTotals, LiquidationValues, StabilityPoolState, Trove};

#[account]
#[derive(InitSpace)]
pub struct PoolState {
    pub creator: Pubkey,
    pub stablecoin: Pubkey,
    pub collateral: Pubkey,
    pub cvgt: Pubkey,

    pub cvgt_staking_state: Pubkey,

    pub mcr: u64,
    pub ccr: u64,
    pub min_net_debt: u64,
    pub gas_compensation: u64,
    pub coll_gas_comp_percent_divisor: u64,

    pub last_fee_operation_time: u64,
    pub base_rate: u64,

    // for redistribution rewards calculation
    pub total_stakes: u64,
    pub total_stakes_snapshot: u64,
    pub total_coll_snapshot: u64,
    pub l_coll: u128,
    pub l_usv_debt: u128,
    pub last_coll_error_redistribution: u128,
    pub last_usv_debt_error_redistribution: u128,

    pub total_surplus: u64,

    // Default Pool
    pub liquidated_coll: u64,
    pub closed_debt: u64,

    // Active Pool
    pub active_coll: u64,
    pub active_debt: u64,

    // SortedTroves data
    // note: troveOwners array is removed. number of trove is keep tracked by trove_size
    pub trove_size: u64,
    pub trove_head: Pubkey,
    pub trove_tail: Pubkey,

    // Bumps
    pub token_auth_bump: [u8; 1],
    pub stability_pool_bump: [u8; 1],
    pub bump: [u8; 1],
}

impl PoolState {
    pub fn init(
        &mut self,
        creator: Pubkey,
        stablecoin: Pubkey,
        collateral: Pubkey,
        cvgt: Pubkey,
        cvgt_staking_state: Pubkey,
        mcr: u64,
        ccr: u64,
        min_net_debt: u64,
        gas_compensation: u64,
        coll_gas_comp_percent_divisor: u64,
        token_auth_bump: [u8; 1],
        bump: [u8; 1],
        stability_pool_bump: [u8; 1],
    ) {
        self.creator = creator;
        self.stablecoin = stablecoin;
        self.collateral = collateral;
        self.cvgt = cvgt;
        self.cvgt_staking_state = cvgt_staking_state;
        self.mcr = mcr;
        self.ccr = ccr;
        self.min_net_debt = min_net_debt;
        self.gas_compensation = gas_compensation;
        self.coll_gas_comp_percent_divisor = coll_gas_comp_percent_divisor;
        self.token_auth_bump = token_auth_bump;
        self.bump = bump;
        self.stability_pool_bump = stability_pool_bump;
        self.active_coll = 0;
        self.liquidated_coll = 0;
        self.active_debt = 0;
        self.closed_debt = 0;
        self.last_fee_operation_time = 0;
        self.base_rate = 0;
        self.total_coll_snapshot = 0;
        self.total_stakes = 0;
        self.total_stakes_snapshot = 0;
        self.l_coll = 0;
        self.l_usv_debt = 0;
        self.trove_size = 0;
        self.total_surplus = 0;
        self.trove_head = Pubkey::default();
        self.trove_tail = Pubkey::default();
    }

    pub fn require_at_least_min_net_debt(&self, net_debt: u64) -> Result<()> {
        require_gte!(
            net_debt,
            self.min_net_debt,
            BorrowerOpsError::DebtLessThanMin
        );
        Ok(())
    }

    pub fn require_not_in_recovery_mode(&self, price: u64) -> Result<()> {
        require!(
            !self.check_recovery_mode(price),
            BorrowerOpsError::InRecoveryMode
        );
        Ok(())
    }

    pub fn require_more_than_one_trove_in_system(&self) -> Result<()> {
        require!(self.trove_size > 1, BorrowerOpsError::OnlyOneTrove);
        Ok(())
    }

    pub fn stability_pool_seeds<'a, 'b: 'a>(&'a self, key: &'b Pubkey) -> [&[u8]; 3] {
        [
            &b"stability"[..],
            key.as_ref(),
            self.stability_pool_bump.as_ref(),
        ]
    }

    pub fn token_auth_seeds<'a, 'b: 'a>(&'a self, key: &'b Pubkey) -> [&[u8]; 3] {
        [
            &b"token-authority"[..],
            key.as_ref(),
            self.token_auth_bump.as_ref(),
        ]
    }

    pub fn seeds(&self) -> [&[u8]; 3] {
        [&b"state"[..], self.cvgt.as_ref(), self.bump.as_ref()]
    }

    pub fn key(&self) -> Pubkey {
        Pubkey::create_program_address(&self.seeds(), &ID).unwrap()
    }

    pub fn get_composit_debt(&self, debt: u64) -> u64 {
        debt.checked_add(self.gas_compensation).unwrap()
    }

    pub fn get_net_debt(&self, debt: u64) -> u64 {
        debt.checked_sub(self.gas_compensation).unwrap()
    }

    pub fn get_coll_gas_compensation(&self, entire_coll: u64) -> u64 {
        entire_coll / self.coll_gas_comp_percent_divisor
    }

    pub fn get_entire_coll(&self) -> u64 {
        self.active_coll.checked_add(self.liquidated_coll).unwrap()
    }

    pub fn get_entire_debt(&self) -> u64 {
        self.active_debt.checked_add(self.closed_debt).unwrap()
    }

    pub fn get_tcr(&self, price: u64) -> u64 {
        let entire_coll = self.get_entire_coll();
        let entire_debt = self.get_entire_debt();
        compute_cr(entire_coll, entire_debt, price).unwrap()
    }

    pub fn check_recovery_mode(&self, price: u64) -> bool {
        let tcr = self.get_tcr(price);
        tcr < self.ccr
    }

    pub fn check_potential_recovery_mode(
        &self,
        entire_system_coll: u64,
        entire_system_debt: u64,
        price: u64,
    ) -> bool {
        let tcr = compute_cr(entire_system_coll, entire_system_debt, price).unwrap();
        tcr < self.ccr
    }

    pub fn minutes_passed_since_last_fee_op(&self) -> u64 {
        let cur_timestamp = get_current_timestamp();
        cur_timestamp
            .checked_sub(self.last_fee_operation_time)
            .unwrap()
            .checked_div(SECOND_IN_ONE_MINUTE.into())
            .unwrap()
    }

    pub fn calc_decayed_base_rate(&self) -> u64 {
        let minutes_passed = self.minutes_passed_since_last_fee_op();
        let decay_factor = dec_pow(MINUTE_DECAY_FACTOR, minutes_passed).unwrap();
        u64::try_from(
            (self.base_rate as u128)
                .checked_mul(decay_factor.into())
                .unwrap()
                .checked_div(DECIMAL_PRECISION.into())
                .unwrap(),
        )
        .unwrap()
    }

    pub fn get_borrowing_rate(&self) -> u64 {
        calc_borrowing_rate(self.base_rate)
    }

    pub fn get_borrowing_fee(&self, usv_debt: u64) -> u64 {
        calc_borrowing_fee(self.get_borrowing_rate(), usv_debt)
    }

    pub fn get_redemption_fee(&self, coll_drawn: u64) -> Result<u64> {
        calc_redemption_fee(self.get_redemption_rate(), coll_drawn)
    }

    pub fn get_redemption_rate(&self) -> u64 {
        calc_redemption_rate(self.base_rate)
    }

    pub fn get_new_tcr_from_trove_change(
        &self,
        coll_change: u64,
        is_coll_increase: bool,
        debt_change: u64,
        is_debt_increase: bool,
        price: u64,
    ) -> u64 {
        let mut total_coll = self.get_entire_coll();
        let mut total_debt = self.get_entire_debt();

        total_coll = if is_coll_increase {
            total_coll.checked_add(coll_change).unwrap()
        } else {
            total_coll.checked_sub(coll_change).unwrap()
        };

        total_debt = if is_debt_increase {
            total_debt.checked_add(debt_change).unwrap()
        } else {
            total_debt.checked_sub(debt_change).unwrap()
        };

        compute_cr(total_coll, total_debt, price).unwrap()
    }

    pub fn get_capped_offset_vals(
        &self,
        entire_trove_debt: u64,
        entire_trove_coll: u64,
        price: u64,
    ) -> Result<LiquidationValues> {
        let mut single_liquidation: LiquidationValues = Default::default();
        single_liquidation.entire_trove_debt = entire_trove_debt;
        single_liquidation.entire_trove_coll = entire_trove_coll;
        let capped_coll_portion = u64::try_from(
            (entire_trove_debt as u128)
                .checked_mul(self.mcr.into())
                .unwrap()
                .checked_div(price.into())
                .unwrap(),
        )
        .unwrap();

        single_liquidation.coll_gas_compensation =
            self.get_coll_gas_compensation(capped_coll_portion);
        single_liquidation.usv_gas_compensation = self.gas_compensation;

        single_liquidation.debt_to_offset = entire_trove_debt;
        single_liquidation.coll_to_send_to_sp = capped_coll_portion
            .checked_sub(single_liquidation.coll_gas_compensation)
            .unwrap();
        single_liquidation.coll_surplus =
            entire_trove_coll.checked_sub(capped_coll_portion).unwrap();
        single_liquidation.debt_to_redistribute = 0;
        single_liquidation.coll_to_redistribute = 0;
        Ok(single_liquidation)
    }

    pub fn require_icr_is_above_mcr(&self, new_icr: u64) -> Result<()> {
        require!(new_icr >= self.mcr, BorrowerOpsError::ICRLowerThanMCR);
        Ok(())
    }

    pub fn require_icr_is_above_ccr(&self, new_icr: u64) -> Result<()> {
        require!(new_icr >= self.ccr, BorrowerOpsError::ICRLowerThanCCR);
        Ok(())
    }

    pub fn require_new_tcr_is_above_ccr(&self, new_tcr: u64) -> Result<()> {
        require!(new_tcr >= self.ccr, BorrowerOpsError::TCRLowerThanCCR);
        Ok(())
    }

    pub fn require_tcr_over_mcr(&self, price: u64) -> Result<()> {
        require!(
            self.get_tcr(price) >= self.mcr,
            BorrowerOpsError::TCRUnderMCR
        );
        Ok(())
    }

    pub fn require_valid_usv_repayment(
        &self,
        current_debt: u64,
        debt_repayment: u64,
    ) -> Result<()> {
        require!(
            debt_repayment <= current_debt.checked_sub(self.gas_compensation).unwrap(),
            BorrowerOpsError::InvalidUSVRepayment
        );
        Ok(())
    }

    pub fn increase_active_debt(&mut self, amount: u64) {
        self.active_debt = self.active_debt.checked_add(amount).unwrap();
    }

    pub fn decrease_active_debt(&mut self, amount: u64) {
        self.active_debt = self.active_debt.checked_sub(amount).unwrap();
    }

    pub fn increase_active_coll(&mut self, amount: u64) {
        self.active_coll = self.active_coll.checked_add(amount).unwrap();
    }

    pub fn decrease_active_coll(&mut self, amount: u64) {
        self.active_coll = self.active_coll.checked_sub(amount).unwrap();
    }

    pub fn increase_liquidated_coll(&mut self, amount: u64) {
        self.liquidated_coll = self.liquidated_coll.checked_add(amount).unwrap();
    }

    pub fn decrease_liquidated_coll(&mut self, amount: u64) {
        self.liquidated_coll = self.liquidated_coll.checked_sub(amount).unwrap();
    }

    pub fn increase_closed_debt(&mut self, amount: u64) {
        self.closed_debt = self.closed_debt.checked_add(amount).unwrap();
    }

    pub fn decrease_closed_debt(&mut self, amount: u64) {
        self.closed_debt = self.closed_debt.checked_sub(amount).unwrap();
    }

    pub fn increase_total_surplus(&mut self, amount: u64) {
        self.total_surplus = self.total_surplus.checked_add(amount).unwrap()
    }

    pub fn decrease_total_surplus(&mut self, amount: u64) {
        self.total_surplus = self.total_surplus.checked_sub(amount).unwrap()
    }

    pub fn decay_base_rate_from_borrowing(&mut self) -> Result<()> {
        let decayed_base_rate = self.calc_decayed_base_rate();
        require!(
            decayed_base_rate <= DECIMAL_PRECISION,
            BorrowerOpsError::Calculation
        );

        self.base_rate = decayed_base_rate;
        emit!(BaseRateUpdated {
            base_rate: self.base_rate
        });

        self.update_last_fee_op_time();
        Ok(())
    }

    /*
     * This function has two impacts on the baseRate state variable:
     * 1) decays the baseRate based on time passed since last redemption or USV borrowing operation.
     * then,
     * 2) increases the baseRate based on the amount redeemed, as a proportion of total supply
     */
    pub fn update_base_fee_rate_from_redemption(
        &mut self,
        coll_drawn: u64,
        price: u64,
        total_usv_supply: u64,
    ) -> Result<u64> {
        let decayed_base_rate = self.calc_decayed_base_rate();

        let redeemed_usv_fraction = u64::try_from(
            u128::from(coll_drawn)
                .checked_mul(price.into())
                .unwrap()
                .checked_div(total_usv_supply.into())
                .unwrap(),
        )
        .unwrap();

        let beta = 2;
        let mut new_base_rate = decayed_base_rate
            .checked_add(redeemed_usv_fraction.checked_div(beta).unwrap())
            .unwrap();
        new_base_rate = cmp::min(new_base_rate, DECIMAL_PRECISION);

        assert!(new_base_rate > 0);

        self.base_rate = new_base_rate;
        emit!(BaseRateUpdated {
            base_rate: new_base_rate
        });

        self.update_last_fee_op_time();

        Ok(new_base_rate)
    }

    pub fn update_last_fee_op_time(&mut self) {
        let cur_timestamp = get_current_timestamp();
        let time_passed = cur_timestamp
            .checked_sub(self.last_fee_operation_time)
            .unwrap();
        if time_passed >= SECOND_IN_ONE_MINUTE.into() {
            self.last_fee_operation_time = cur_timestamp;
            emit!(LastFeeOpTimeUpdated {
                last_fee_op_time: cur_timestamp
            });
        }
    }

    pub fn move_pending_trove_rewards_to_active(&mut self, usv_amt: u64, coll_amt: u64) {
        self.increase_active_debt(usv_amt);
        self.decrease_closed_debt(usv_amt);
        self.decrease_liquidated_coll(coll_amt);
        self.increase_active_coll(coll_amt);
    }

    pub fn move_coll_debt_from_liquidate(
        &mut self,
        sp_state: &mut StabilityPoolState,
        totals: &LiquidationTotals,
    ) {
        self.decrease_active_coll(totals.total_coll_gas_compensation);

        self.decrease_active_debt(totals.total_debt_to_offset);
        sp_state.decrease_usv(totals.total_debt_to_offset);

        self.decrease_active_coll(totals.total_coll_to_send_to_sp);
        sp_state.increase_coll(totals.total_coll_to_send_to_sp);
    }

    pub fn apply_pending_reward(&mut self, trove: &mut Trove) -> Result<()> {
        if trove.has_pending_rewards(self) {
            trove.require_trove_active()?;

            let pending_coll_reward = trove.get_pending_coll_reward(self);
            let pending_debt_reward = trove.get_pending_debt_reward(self);

            trove.coll = trove.coll.checked_add(pending_coll_reward).unwrap();
            trove.debt = trove.debt.checked_add(pending_debt_reward).unwrap();

            trove.update_reward_snapshot(self);

            self.move_pending_trove_rewards_to_active(pending_debt_reward, pending_coll_reward);
            // emit event
        }
        Ok(())
    }

    pub fn redistribute_debt_and_coll(&mut self, debt: u64, coll: u64) {
        if debt == 0 {
            return;
        }
        let coll_numerator = (coll as u128)
            .checked_mul(DECIMAL_PRECISION.into())
            .unwrap()
            .checked_add(self.last_coll_error_redistribution.into())
            .unwrap();
        let usv_debt_numerator = (debt as u128)
            .checked_mul(DECIMAL_PRECISION.into())
            .unwrap()
            .checked_add(self.last_usv_debt_error_redistribution.into())
            .unwrap();

        let coll_reward_per_unit_staked = coll_numerator
            .checked_div(self.total_stakes.into())
            .unwrap();
        let usv_debt_reward_per_unit_staked = usv_debt_numerator
            .checked_div(self.total_stakes.into())
            .unwrap();

        self.last_coll_error_redistribution = coll_numerator
            .checked_sub(
                coll_reward_per_unit_staked
                    .checked_mul(self.total_stakes.into())
                    .unwrap(),
            )
            .unwrap();
        self.last_usv_debt_error_redistribution = usv_debt_numerator
            .checked_sub(
                usv_debt_reward_per_unit_staked
                    .checked_mul(self.total_stakes.into())
                    .unwrap(),
            )
            .unwrap();

        self.l_coll = self
            .l_coll
            .checked_add(coll_reward_per_unit_staked)
            .unwrap();
        self.l_usv_debt = self
            .l_usv_debt
            .checked_add(usv_debt_reward_per_unit_staked)
            .unwrap();

        // TODO: emit l_terms_updated events (self.l_coll, self.l_usv_debt)

        self.decrease_active_debt(debt);
        self.increase_closed_debt(debt);
        self.decrease_active_coll(coll);
        self.increase_liquidated_coll(coll);
    }

    pub fn update_system_snapshots_exclude_coll_remainder(&mut self, coll_remainder: u64) {
        self.total_stakes_snapshot = self.total_stakes;

        let active_coll = self.active_coll;
        let liquidated_coll = self.liquidated_coll;
        self.total_coll_snapshot = active_coll
            .checked_sub(coll_remainder)
            .unwrap()
            .checked_add(liquidated_coll)
            .unwrap();

        emit!(SystemSnapshotsUpdated {
            total_stakes_snapshot: self.total_stakes_snapshot,
            total_coll_snapshot: self.total_coll_snapshot
        });
    }
}

fn calc_borrowing_fee(borrowing_rate: u64, usv_debt: u64) -> u64 {
    u64::try_from(
        (borrowing_rate as u128)
            .checked_mul(usv_debt.into())
            .unwrap()
            .checked_div(DECIMAL_PRECISION.into())
            .unwrap(),
    )
    .unwrap()
}

pub fn calc_borrowing_rate(base_rate: u64) -> u64 {
    cmp::min(
        BORROWING_FEE_FLOOR.checked_add(base_rate).unwrap(),
        MAX_BORROWING_FEE,
    )
}

pub fn calc_redemption_fee(redemption_rate: u64, coll_drawn: u64) -> Result<u64> {
    let redemption_fee = u64::try_from(
        (redemption_rate as u128)
            .checked_mul(coll_drawn.into())
            .unwrap()
            .checked_div(DECIMAL_PRECISION.into())
            .unwrap(),
    )
    .unwrap();

    require!(
        redemption_fee < coll_drawn,
        BorrowerOpsError::FeeEatUpAllColl
    );
    Ok(redemption_fee)
}

pub fn calc_redemption_rate(base_rate: u64) -> u64 {
    cmp::min(
        REDEMPTION_FEE_FLOOR.checked_add(base_rate).unwrap(),
        DECIMAL_PRECISION,
    )
}
