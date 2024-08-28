use anchor_lang::prelude::*;

use crate::events::TotalTokenIssuedUpdated;

#[account]
#[derive(InitSpace)]
pub struct CommunityIssuanceConfig {
    pub creator: Pubkey,
    pub cvgt: Pubkey,
    pub authority: Pubkey,
    pub stability_pool: Pubkey,

    pub enable_emission: bool,
    pub total_cvgt_issued: u64,
    pub last_reward_timestamp: u64,
    pub emission_rate: u64,

    pub _is_dev: bool,
    pub _timestamp: u64,

    pub bump: [u8; 1],
}

impl CommunityIssuanceConfig {
    pub fn seeds(&self) -> [&[u8]; 3] {
        [
            &b"community-issuance"[..],
            self.cvgt.as_ref(),
            self.bump.as_ref(),
        ]
    }

    pub fn get_current_timestamp(&self) -> Result<u64> {
        if self._is_dev {
            return Ok(self._timestamp);
        }
        Ok(Clock::get()?.unix_timestamp.try_into().unwrap())
    }

    pub fn issue_token(&mut self) -> Result<u64> {
        let current_timestamp = self.get_current_timestamp()?;
        let amount = compute_emission_amount(self, current_timestamp).unwrap();

        self.last_reward_timestamp = current_timestamp;
        self.total_cvgt_issued = self.total_cvgt_issued.checked_add(amount).unwrap();

        emit!(TotalTokenIssuedUpdated {
            token: self.cvgt,
            total_cvgt_issued: self.total_cvgt_issued,
        });
        Ok(amount)
    }
}

fn compute_emission_amount(
    config: &CommunityIssuanceConfig,
    current_timestamp: u64,
) -> Option<u64> {
    if !config.enable_emission {
        return Some(0);
    }
    let last_reward_timestamp = config.last_reward_timestamp;

    let amount = current_timestamp
        .checked_sub(last_reward_timestamp)?
        .checked_mul(config.emission_rate)?;
    Some(amount)
}
