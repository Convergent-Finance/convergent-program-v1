use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token};

use crate::{constants::DEPLOYER, state::CVGTStakingPoolState};

#[derive(Accounts)]
pub struct InitializeCVGTStaking<'info> {
    #[account(
        init,
        space = 8 + CVGTStakingPoolState::INIT_SPACE,
        payer = creator,
        seeds = [
            b"staking-state",
            cvgt.key().as_ref()
        ],
        bump
    )]
    pub pool_state: Account<'info, CVGTStakingPoolState>,

    #[account()]
    pub cvgt: Account<'info, Mint>,

    #[account()]
    pub usv: Account<'info, Mint>,

    #[account()]
    pub collateral: Account<'info, Mint>,

    #[account(
        mut,
        constraint = creator.key() == DEPLOYER
    )]
    pub creator: Signer<'info>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn initialize_cvgt_staking_handler(ctx: Context<InitializeCVGTStaking>) -> Result<()> {
    let cvgt = ctx.accounts.cvgt.key();
    let usv = ctx.accounts.usv.key();
    let collateral = ctx.accounts.collateral.key();
    let bump = ctx.bumps.pool_state;

    let pool_state = &mut ctx.accounts.pool_state;

    **pool_state = CVGTStakingPoolState {
        cvgt,
        collateral,
        usv,
        total_cvgt_staked: 0,
        f_coll: 0,
        f_usv: 0,
        bump: [bump],
    };
    Ok(())
}
