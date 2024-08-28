use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};

use crate::{constants::DEPLOYER, state::CommunityIssuanceConfig};

#[derive(Accounts)]
pub struct InitializeCommunityIssuance<'info> {
    #[account(
        init,
        space = 8 + CommunityIssuanceConfig::INIT_SPACE,
        payer = creator,
        seeds = [
            b"community-issuance",
            cvgt.key().as_ref()
        ],
        bump
    )]
    pub config: Account<'info, CommunityIssuanceConfig>,

    #[account(
        init_if_needed,
        payer = creator,
        associated_token::mint = cvgt,
        associated_token::authority = config
    )]
    pub token_vault: Box<Account<'info, TokenAccount>>,

    #[account()]
    pub cvgt: Account<'info, Mint>,

    #[account(
        mut,
        constraint = creator.key() == DEPLOYER
    )]
    pub creator: Signer<'info>,

    /// CHECK: This account is not read or written
    pub stability_pool: UncheckedAccount<'info>,

    /// CHECK: This account is not read or written
    pub authority: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn initialize_community_issuance_handler(
    ctx: Context<InitializeCommunityIssuance>,
    emission_rate: u64,
    is_dev: bool,
) -> Result<()> {
    let creator = ctx.accounts.creator.key();
    let authority = ctx.accounts.authority.key();
    let cvgt = ctx.accounts.cvgt.key();
    let stability_pool = ctx.accounts.stability_pool.key();
    let bump = ctx.bumps.config;

    let config = &mut ctx.accounts.config;

    **config = CommunityIssuanceConfig {
        creator,
        cvgt,
        authority,
        stability_pool,
        enable_emission: false,
        last_reward_timestamp: 0,
        total_cvgt_issued: 0,
        emission_rate,
        bump: [bump],
        _is_dev: is_dev,
        _timestamp: 0,
    };

    Ok(())
}
