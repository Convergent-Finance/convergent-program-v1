use anchor_lang::prelude::*;

use crate::{
    constants::{DECIMAL_PRECISION, SCALE_FACTOR},
    errors::StabilityPoolError,
    events::DepositSnapshotUpdated,
};

use super::{EpochScale, StabilityPoolState};

#[account]
#[derive(InitSpace, Default)]
pub struct StabilityPoolDeposit {
    pub initial_value: u64,
    pub snapshots_s: u128,
    pub snapshots_p: u128,
    pub snapshots_g: u128,
    pub snapshots_scale: u128,
    pub snapshots_epoch: u128,
    pub claimable_coll: u64,
    pub claimable_cvgt: u64,
}

impl StabilityPoolDeposit {
    pub fn get_depositor_coll_gain(
        &self,
        first_epoch_scale: &EpochScale,
        second_epoch_scale: &EpochScale,
    ) -> Option<u64> {
        if self.initial_value == 0 {
            return Some(0);
        }

        // Get coll gain from snapshots
        let first_portion = first_epoch_scale.sum.checked_sub(self.snapshots_s)?;
        let second_portion = second_epoch_scale.sum.checked_div(SCALE_FACTOR.into())?;

        let coll_gain = (self.initial_value as u128)
            .checked_mul(first_portion.checked_add(second_portion)?.into())?
            .checked_div(self.snapshots_p)?
            .checked_div(DECIMAL_PRECISION.into())?;
        Some(u64::try_from(coll_gain).unwrap())
    }

    pub fn get_compounded_usv_deposit(&self, sp_state: &StabilityPoolState) -> Option<u64> {
        if self.initial_value == 0 {
            return Some(0);
        }

        // Get compounded stake from snapshots
        // If stake was made before a pool-emptying event, then it has been fully cancelled with debt -- so, return 0
        if self.snapshots_epoch < sp_state.current_epoch {
            return Some(0);
        }

        let scale_diff = sp_state.current_scale.checked_sub(self.snapshots_scale)?;

        /* Compute the compounded stake. If a scale change in P was made during the stake's lifetime,
         * account for it. If more than one scale change was made, then the stake has decreased by a factor of
         * at least 1e-9 -- so return 0.
         */

        // Consider to change the snapshots to be u128 instead of u64
        let compounded_stake = u64::try_from(if scale_diff == 0 {
            (self.initial_value as u128)
                .checked_mul(sp_state.p.into())?
                .checked_div(self.snapshots_p.into())?
        } else if scale_diff == 1 {
            (self.initial_value as u128)
                .checked_mul(sp_state.p.into())?
                .checked_div(self.snapshots_p.into())?
                .checked_div(SCALE_FACTOR.into())?
        } else {
            0u128
        })
        .unwrap();

        /*
         * If compounded deposit is less than a billionth of the initial deposit, return 0.
         *
         * NOTE: originally, this line was in place to stop rounding errors making the deposit too large. However, the error
         * corrections should ensure the error in P "favors the Pool", i.e. any given compounded deposit should slightly less
         * than it's theoretical value.
         *
         * Thus it's unclear whether this line is still really needed.
         */
        if compounded_stake < self.initial_value.checked_div(1_000_000_000)? {
            return Some(0);
        }
        Some(compounded_stake)
    }

    pub fn get_cvgt_gain(
        &self,
        first_epoch_scale: &EpochScale,
        second_epoch_scale: &EpochScale,
    ) -> Option<u64> {
        if self.initial_value == 0 {
            return Some(0);
        }
        self.get_cvgt_gain_from_snapshots(first_epoch_scale, second_epoch_scale)
    }

    pub fn get_cvgt_gain_from_snapshots(
        &self,
        first_epoch_scale: &EpochScale,
        second_epoch_scale: &EpochScale,
    ) -> Option<u64> {
        let first_portion = first_epoch_scale.g.checked_sub(self.snapshots_g)?;
        let second_portion = second_epoch_scale.g.checked_div(SCALE_FACTOR.into())?;
        let cvgt_gain = (self.initial_value as u128)
            .checked_mul(first_portion.checked_add(second_portion)?)?
            .checked_div(self.snapshots_p)?
            .checked_div(DECIMAL_PRECISION.into())?;

        Some(u64::try_from(cvgt_gain).unwrap())
    }

    pub fn require_user_has_deposit(&self) -> Result<()> {
        require!(self.initial_value > 0, StabilityPoolError::ZeroDeposit);
        Ok(())
    }

    pub fn update_deposit_and_snapshot(
        &mut self,
        sp_state: &StabilityPoolState,
        current_epoch_scale: &EpochScale,
        depositor: Pubkey,
        new_value: u64,
    ) {
        self.initial_value = new_value;
        if new_value == 0 {
            self.snapshots_p = 0;
            self.snapshots_s = 0;
            self.snapshots_g = 0;
            self.snapshots_scale = 0;
            self.snapshots_epoch = 0;
            emit!(DepositSnapshotUpdated {
                depositor,
                p: 0,
                s: 0,
                g: 0
            });
        } else {
            self.snapshots_p = sp_state.p;
            self.snapshots_s = current_epoch_scale.sum;
            self.snapshots_g = current_epoch_scale.g;
            self.snapshots_scale = sp_state.current_scale;
            self.snapshots_epoch = sp_state.current_epoch;
            emit!(DepositSnapshotUpdated {
                depositor,
                p: sp_state.p,
                s: current_epoch_scale.sum,
                g: current_epoch_scale.g
            });
        }
    }
}
