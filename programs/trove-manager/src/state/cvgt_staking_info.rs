use anchor_lang::prelude::*;

use crate::{constants::DECIMAL_PRECISION, events::StakerSnapshotsUpdated};

use super::CVGTStakingPoolState;

#[account]
#[derive(InitSpace)]
pub struct CVGTStakingInfo {
    pub balance: u64,
    pub f_coll_snapshot: u64,
    pub f_usv_snapshot: u64,
}

impl CVGTStakingInfo {
    pub fn get_pending_coll_gain(&self, pool_state: &CVGTStakingPoolState) -> u64 {
        let f_coll_snapshot = self.f_coll_snapshot;
        let coll_gain = (self.balance as u128)
            .checked_mul(
                pool_state
                    .f_coll
                    .checked_sub(f_coll_snapshot)
                    .unwrap()
                    .into(),
            )
            .unwrap()
            .checked_div(DECIMAL_PRECISION.into())
            .unwrap();
        u64::try_from(coll_gain).unwrap()
    }

    pub fn get_pending_usv_gain(&self, pool_state: &CVGTStakingPoolState) -> u64 {
        let f_usv_snapshot = self.f_usv_snapshot;
        let usv_gain = (self.balance as u128)
            .checked_mul(pool_state.f_usv.checked_sub(f_usv_snapshot).unwrap().into())
            .unwrap()
            .checked_div(DECIMAL_PRECISION.into())
            .unwrap();
        u64::try_from(usv_gain).unwrap()
    }

    pub fn update_snapshot(&mut self, user_key: &Pubkey, pool_state: &CVGTStakingPoolState) {
        self.f_coll_snapshot = pool_state.f_coll;
        self.f_usv_snapshot = pool_state.f_usv;
        emit!(StakerSnapshotsUpdated {
            user: *user_key,
            f_coll: self.f_coll_snapshot,
            f_usv: self.f_usv_snapshot
        });
    }
}
