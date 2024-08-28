use crate::state::{PoolState, Trove, TroveStatus};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct InitializeTrove<'info> {
    #[account()]
    pub pool_state: Box<Account<'info, PoolState>>,

    #[account(
        init,
        payer = creator,
        space = 8 + Trove::INIT_SPACE,
        seeds = [
            b"trove", 
            pool_state.key().as_ref(),
            creator.key().as_ref(),
        ],
        bump
    )]
    pub trove: Box<Account<'info, Trove>>,

    #[account(mut)]
    pub creator: Signer<'info>,

    pub system_program: Program<'info, System>,
}

pub fn initialize_trove_handler(ctx: Context<InitializeTrove>) -> Result<()> {
    let creator = ctx.accounts.creator.key();

    let trove = &mut ctx.accounts.trove;

    trove.init(ctx.accounts.pool_state.key(), creator, 0, 0);
    trove.status = TroveStatus::ClosedByOwner;

    Ok(())
}
