use anchor_lang::prelude::*;

use crate::state::{CVGTStakingPoolState, PoolState};

#[derive(Accounts)]
pub struct ConfigPoolState<'info> {
    #[account(mut)]
    pub pool_state: Box<Account<'info, PoolState>>,

    #[account(
        mut,
        constraint = creator.key() == pool_state.creator
    )]
    pub creator: Signer<'info>,

    #[account(
        mut,
        seeds = [
            b"staking-state",
            pool_state.cvgt.as_ref(),
        ],
        bump,
    )]
    pub cvgt_staking_state: Account<'info, CVGTStakingPoolState>,
}

pub fn config_pool_state_handler(ctx: Context<ConfigPoolState>) -> Result<()> {
    let pool_state = &mut ctx.accounts.pool_state;
    let cvgt_staking_state_key = ctx.accounts.cvgt_staking_state.key();
    pool_state.cvgt_staking_state = cvgt_staking_state_key;
    Ok(())
}
