use anchor_lang::prelude::*;

use crate::{
    constants::{DECIMAL_PRECISION, SCALE_FACTOR},
    events::{EpochUpdated, PUpdated, SUpdated, ScaleUpdated, StabilityPoolUSVBalanceUpdated},
};

use super::{EpochScale, LiquidationTotals};

#[account]
#[derive(InitSpace)]
pub struct StabilityPoolState {
    // Addresses
    pub cvgt: Pubkey,

    // State
    pub total_collateral: u64,
    pub total_usv_deposits: u64,
    pub p: u128,
    pub current_scale: u128,
    pub current_epoch: u128,
    pub last_cvgt_error: u128,
    pub last_coll_error_offset: u128,
    pub last_usv_error_offset: u128,
}

impl StabilityPoolState {
    pub fn init(&mut self, cvgt: Pubkey) {
        self.cvgt = cvgt;
        self.p = DECIMAL_PRECISION.into();
        self.total_collateral = 0;
        self.total_usv_deposits = 0;
        self.current_scale = 0;
        self.current_epoch = 0;
        self.last_cvgt_error = 0;
    }

    pub fn compute_cvgt_per_unit_staked(&mut self, cvgt_issuance: u64) -> Option<u128> {
        let cvgt_numerator = (cvgt_issuance as u128)
            .checked_mul(DECIMAL_PRECISION.into())?
            .checked_add(self.last_cvgt_error)?;
        let cvgt_per_unit_staked = cvgt_numerator.checked_div(self.total_usv_deposits.into())?;
        self.last_cvgt_error = cvgt_numerator
            .checked_sub(cvgt_per_unit_staked.checked_mul(self.total_usv_deposits.into())?)?;
        Some(cvgt_per_unit_staked)
    }

    pub fn increase_usv(&mut self, amount: u64) {
        self.total_usv_deposits = self.total_usv_deposits.checked_add(amount).unwrap();
        emit!(StabilityPoolUSVBalanceUpdated {
            new_balance: self.total_usv_deposits
        });
    }

    pub fn decrease_usv(&mut self, amount: u64) {
        let new_total_usv_deposits = self.total_usv_deposits.checked_sub(amount).unwrap();
        self.total_usv_deposits = new_total_usv_deposits;
        emit!(StabilityPoolUSVBalanceUpdated {
            new_balance: self.total_usv_deposits
        });
    }

    pub fn decrease_coll(&mut self, amount: u64) {
        if amount == 0 {
            return;
        }
        let new_coll = self.total_collateral.checked_sub(amount).unwrap();
        self.total_collateral = new_coll;
    }

    pub fn increase_coll(&mut self, amount: u64) {
        let new_coll = self.total_collateral.checked_add(amount).unwrap();
        self.total_collateral = new_coll;
    }

    pub fn compute_rewards_per_unit_staked(
        &mut self,
        coll_to_add: u64,
        debt_to_offset: u64,
    ) -> Option<(u64, u64)> {
        let coll_numerator = (coll_to_add as u128)
            .checked_mul(DECIMAL_PRECISION.into())?
            .checked_add(self.last_coll_error_offset)?;
        let total_usv_deposits = self.total_usv_deposits;

        assert!(debt_to_offset <= total_usv_deposits);

        let usv_loss_per_unit_staked = if debt_to_offset == total_usv_deposits {
            self.last_coll_error_offset = 0;
            DECIMAL_PRECISION
        } else {
            let usv_loss_numerator = (debt_to_offset as u128)
                .checked_mul(DECIMAL_PRECISION.into())?
                .checked_sub(self.last_usv_error_offset)?;
            /*
             * Add 1 to make error in quotient positive. We want "slightly too much" USV loss,
             * which ensures the error in any given compoundedUSVDeposit favors the Stability Pool.
             */
            let usv_loss_per_unit_staked = usv_loss_numerator
                .checked_div(total_usv_deposits.into())?
                .checked_add(1)?;
            self.last_usv_error_offset = usv_loss_per_unit_staked
                .checked_mul(total_usv_deposits.into())?
                .checked_sub(usv_loss_numerator)?;
            u64::try_from(usv_loss_per_unit_staked).unwrap()
        };

        let coll_gain_per_unit_staked =
            u64::try_from(coll_numerator.checked_div(total_usv_deposits.into())?).unwrap();
        self.last_coll_error_offset = coll_numerator.checked_sub(
            (coll_gain_per_unit_staked as u128).checked_mul(total_usv_deposits.into())?,
        )?;
        Some((coll_gain_per_unit_staked, usv_loss_per_unit_staked))
    }

    pub fn update_reward_sum_and_product(
        &mut self,
        current_epoch_scale: &mut EpochScale,
        coll_gain_per_unit_staked: u64,
        usv_loss_per_unit_staked: u64,
    ) -> Option<()> {
        assert!(usv_loss_per_unit_staked <= DECIMAL_PRECISION);
        /*
         * The newProductFactor is the factor by which to change all deposits, due to the depletion of Stability Pool USV in the liquidation.
         * We make the product factor 0 if there was a pool-emptying. Otherwise, it is (1 - USVLossPerUnitStaked)
         */
        let new_product_factor = DECIMAL_PRECISION.checked_sub(usv_loss_per_unit_staked)?;

        /*
         * Calculate the new S first, before we update P.
         * The Coll gain for any given depositor from a liquidation depends on the value of their deposit
         * (and the value of totalDeposits) prior to the Stability being depleted by the debt in the liquidation.
         *
         * Since S corresponds to Coll gain, and P to deposit loss, we update S first.
         */
        let marginal_coll_gain = (coll_gain_per_unit_staked as u128).checked_mul(self.p)?;
        let new_s = current_epoch_scale.sum.checked_add(marginal_coll_gain)?;
        current_epoch_scale.sum = new_s;
        emit!(SUpdated {
            s: new_s,
            epoch: self.current_epoch,
            scale: self.current_scale
        });

        // If the Stability Pool was emptied, increment the epoch, and reset the scale and product P
        let new_p = if new_product_factor == 0 {
            self.current_epoch = self.current_epoch.checked_add(1)?;
            emit!(EpochUpdated {
                current_epoch: self.current_epoch
            });
            self.current_scale = 0;
            emit!(ScaleUpdated {
                current_scale: self.current_scale
            });
            DECIMAL_PRECISION.into()
        // If multiplying P by a non-zero product factor would reduce P below the scale boundary, increment the scale
        } else if self
            .p
            .checked_mul(new_product_factor.into())?
            .checked_div(DECIMAL_PRECISION.into())?
            < SCALE_FACTOR.into()
        {
            self.current_scale = self.current_scale.checked_add(1)?;
            emit!(ScaleUpdated {
                current_scale: self.current_scale
            });
            self.p
                .checked_mul(new_product_factor.into())?
                .checked_mul(SCALE_FACTOR.into())?
                .checked_div(DECIMAL_PRECISION.into())?
        } else {
            self.p
                .checked_mul(new_product_factor.into())?
                .checked_div(DECIMAL_PRECISION.into())?
        };

        assert!(new_p > 0);
        self.p = new_p;

        emit!(PUpdated { p: self.p });
        Some(())
    }

    pub fn offset(
        &mut self,
        current_epoch_scale: &mut EpochScale,
        totals: &LiquidationTotals,
        cvgt_issuance: u64,
    ) {
        current_epoch_scale.update_g(self, cvgt_issuance);
        let (coll_gain_per_unit_staked, usv_loss_per_unit_staked) = self
            .compute_rewards_per_unit_staked(
                totals.total_coll_to_send_to_sp,
                totals.total_debt_to_offset,
            )
            .unwrap();
        self.update_reward_sum_and_product(
            current_epoch_scale,
            coll_gain_per_unit_staked,
            usv_loss_per_unit_staked,
        );
    }
}
