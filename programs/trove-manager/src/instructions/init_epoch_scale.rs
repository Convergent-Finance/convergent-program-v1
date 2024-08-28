use anchor_lang::prelude::*;

use crate::state::{EpochScale, PoolState, StabilityPoolState};

#[derive(Accounts)]
pub struct InitializeEpochScale<'info> {
    #[account()]
    pub pool_state: Box<Account<'info, PoolState>>,

    #[account(
        mut,
        seeds = [
            b"stability", 
            pool_state.key().as_ref(),
        ],
        bump
    )]
    pub stability_pool_state: Box<Account<'info, StabilityPoolState>>,

    #[account(
        init,
        payer = signer,
        space = 8 + EpochScale::INIT_SPACE,
        seeds = [
            b"epoch-scale",
            stability_pool_state.key().as_ref(),
            stability_pool_state.current_epoch.to_le_bytes().as_ref(),
            stability_pool_state.current_scale.to_le_bytes().as_ref(),
        ],
        bump
    )]
    pub current_epoch_scale: Box<Account<'info, EpochScale>>,

    #[account(mut)]
    pub signer: Signer<'info>,
    pub system_program: Program<'info, System>,
}
