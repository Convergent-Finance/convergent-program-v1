use anchor_lang::prelude::*;

use crate::{
    constants::DECIMAL_PRECISION,
    events::{FCollUpdated, FUSVUpdated},
};

#[account]
#[derive(InitSpace)]
pub struct CVGTStakingPoolState {
    pub usv: Pubkey,
    pub collateral: Pubkey,
    pub cvgt: Pubkey,

    pub f_usv: u64,
    pub f_coll: u64,
    pub total_cvgt_staked: u64,

    pub bump: [u8; 1],
}

impl CVGTStakingPoolState {
    pub fn seeds(&self) -> [&[u8]; 3] {
        [
            &b"staking-state"[..],
            &self.cvgt.as_ref(),
            &self.bump.as_ref(),
        ]
    }

    pub fn increase_f_coll(&mut self, coll_fee: u64) {
        let coll_fee_per_cvgt_staked = if self.total_cvgt_staked > 0 {
            u64::try_from(
                (coll_fee as u128)
                    .checked_mul(DECIMAL_PRECISION.into())
                    .unwrap()
                    .checked_div(self.total_cvgt_staked.into())
                    .unwrap(),
            )
            .unwrap()
        } else {
            0
        };

        self.f_coll = self.f_coll.checked_add(coll_fee_per_cvgt_staked).unwrap();
        emit!(FCollUpdated {
            f_coll: self.f_coll
        });
    }

    pub fn increase_f_usv(&mut self, usv_fee: u64) {
        let usv_fee_per_cvgt_staked = if self.total_cvgt_staked > 0 {
            u64::try_from(
                (usv_fee as u128)
                    .checked_mul(DECIMAL_PRECISION.into())
                    .unwrap()
                    .checked_div(self.total_cvgt_staked.into())
                    .unwrap(),
            )
            .unwrap()
        } else {
            0
        };

        self.f_usv = self.f_usv.checked_add(usv_fee_per_cvgt_staked).unwrap();
        emit!(FUSVUpdated { f_usv: self.f_usv });
    }
}
