use anchor_lang::prelude::*;

use crate::state::CommunityIssuanceConfig;

#[derive(Accounts)]
pub struct SetTimestamp<'info> {
    #[account(mut)]
    pub config: Account<'info, CommunityIssuanceConfig>,
}

pub fn set_timestamp_handler(ctx: Context<SetTimestamp>, new_timestamp: u64) -> Result<()> {
    let config = &mut ctx.accounts.config;

    assert!(config._is_dev);
    assert!(config._timestamp < new_timestamp);

    config._timestamp = new_timestamp;
    Ok(())
}
