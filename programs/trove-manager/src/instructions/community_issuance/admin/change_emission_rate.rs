use anchor_lang::prelude::*;

use crate::{
    constants::MAX_EMISSION_RATE, errors::CommunityIssuanceError, events::EmissionRateChanged,
    state::CommunityIssuanceConfig,
};

#[derive(Accounts)]
pub struct ChangeEmissionRate<'info> {
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

pub fn change_emission_rate_handler(ctx: Context<ChangeEmissionRate>, new_rate: u64) -> Result<()> {
    let config = &mut ctx.accounts.config;

    require!(
        new_rate < MAX_EMISSION_RATE,
        CommunityIssuanceError::ExceedMax
    );

    config.emission_rate = new_rate;

    emit!(EmissionRateChanged {
        token: config.cvgt,
        new_rate,
    });
    Ok(())
}
