use anchor_lang::prelude::*;

use crate::{
    errors::CommunityIssuanceError, events::EmissionEnabled, state::CommunityIssuanceConfig,
    utils::get_current_timestamp_with_config,
};

#[derive(Accounts)]
pub struct EnableEmission<'info> {
    #[account(
        mut,
        seeds = [
            b"community-issuance",
            config.cvgt.as_ref()
        ],
        bump
    )]
    pub config: Account<'info, CommunityIssuanceConfig>,

    #[account(
        mut,
        constraint = authority.key() == config.authority @ CommunityIssuanceError::InvalidSigner
    )]
    pub authority: Signer<'info>,
}

pub fn enable_emission_handler(ctx: Context<EnableEmission>) -> Result<()> {
    let config = &mut ctx.accounts.config;
    config.last_reward_timestamp = get_current_timestamp_with_config(&config)?;
    config.enable_emission = true;
    emit!(EmissionEnabled { token: config.cvgt });
    Ok(())
}
