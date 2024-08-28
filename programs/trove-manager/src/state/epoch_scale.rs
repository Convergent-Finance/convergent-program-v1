use anchor_lang::prelude::*;

use super::{StabilityPoolDeposit, StabilityPoolState};
use crate::{errors::StabilityPoolError, events::GUpdated, ID};

#[account]
#[derive(InitSpace, Default)]
pub struct EpochScale {
    pub sum: u128,
    pub g: u128,
}

impl EpochScale {
    pub fn update_g(&mut self, sp_state: &mut StabilityPoolState, cvgt_issuance: u64) {
        if sp_state.total_usv_deposits == 0 || cvgt_issuance == 0 {
            return;
        }

        let cvgt_per_unit_staked = sp_state
            .compute_cvgt_per_unit_staked(cvgt_issuance)
            .unwrap();

        let marginal_cvgt_gain = cvgt_per_unit_staked.checked_mul(sp_state.p).unwrap();
        self.g = self.g.checked_add(marginal_cvgt_gain).unwrap();
        emit!(GUpdated {
            g: self.g,
            epoch: sp_state.current_epoch,
            scale: sp_state.current_scale,
        });
    }

    pub fn deserialize(
        data: std::cell::RefMut<'_, &mut [u8]>,
        key: &Pubkey,
        cur_key: &Pubkey,
        current_epoch_scale: &EpochScale,
    ) -> Self {
        if key == cur_key {
            return current_epoch_scale.clone();
        }
        if data.len() == 0 {
            EpochScale::default()
        } else {
            EpochScale::try_deserialize(&mut data.as_ref()).expect("Error Deserializing Data")
        }
    }
}

pub fn get_epoch_scales(
    remaining_accounts: &[AccountInfo<'_>],
    current_epoch_scale_key: &Pubkey,
    current_epoch_scale: &EpochScale,
    sp_state: &Account<'_, StabilityPoolState>,
    sp_deposit: &StabilityPoolDeposit,
) -> Result<(EpochScale, EpochScale)> {
    let first_epoch_scale_acc = &remaining_accounts[0];
    let second_epoch_scale_acc = &remaining_accounts[1];

    let (expected_key, _) = Pubkey::find_program_address(
        &[
            b"epoch-scale",
            sp_state.key().as_ref(),
            sp_deposit.snapshots_epoch.to_le_bytes().as_ref(),
            sp_deposit.snapshots_scale.to_le_bytes().as_ref(),
        ],
        &ID,
    );
    require!(
        first_epoch_scale_acc.key == &expected_key,
        StabilityPoolError::InvalidEpochScale
    );

    let (expected_key, _) = Pubkey::find_program_address(
        &[
            b"epoch-scale",
            sp_state.key().as_ref(),
            sp_deposit.snapshots_epoch.to_le_bytes().as_ref(),
            (sp_deposit.snapshots_scale + 1).to_le_bytes().as_ref(),
        ],
        &ID,
    );
    require!(
        second_epoch_scale_acc.key == &expected_key,
        StabilityPoolError::InvalidEpochScale
    );

    let first_data = first_epoch_scale_acc.try_borrow_mut_data()?;
    let second_data = second_epoch_scale_acc.try_borrow_mut_data()?;

    let first_epoch_scale = EpochScale::deserialize(
        first_data,
        first_epoch_scale_acc.key,
        current_epoch_scale_key,
        current_epoch_scale,
    );
    let second_epoch_scale = EpochScale::deserialize(
        second_data,
        second_epoch_scale_acc.key,
        current_epoch_scale_key,
        current_epoch_scale,
    );

    Ok((first_epoch_scale, second_epoch_scale))
}
