use anchor_lang::prelude::*;
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;

use crate::{
    errors::PriceFeedError,
    state::{PoolState, PriceFeedState},
};

#[derive(Accounts)]
pub struct FetchPrice<'info> {
    #[account(mut)]
    pub pool_state: Box<Account<'info, PoolState>>,

    #[account(
        mut,
        seeds = [
            b"price_feed", 
            pool_state.cvgt.as_ref()
        ],
        bump = price_feed_state.bump
    )]
    pub price_feed_state: Box<Account<'info, PriceFeedState>>,

    #[account(
        constraint = pyth_feed_account.key() == price_feed_state.pyth_feed_account @ PriceFeedError::PythWrongFeed
    )]
    pub pyth_feed_account: Box<Account<'info, PriceUpdateV2>>,

    #[account(
        constraint = chainlink_feed.key == &price_feed_state.chainlink_feed @ PriceFeedError::ChainlinkWrongFeed
    )]
    /// CHECK: This is the Chainlink feed account
    pub chainlink_feed: AccountInfo<'info>,

    /// CHECK: This account will check by StakeProgram
    #[account(mut)]
    pub jitosol_stake_pool: UncheckedAccount<'info>,

    /// CHECK: This is the Chainlink program library
    pub chainlink_program: AccountInfo<'info>,
}

pub fn fetch_price_handler(ctx: Context<FetchPrice>) -> Result<()> {
    let price = ctx.accounts.price_feed_state.fetch_price(
        &ctx.accounts.chainlink_program,
        &ctx.accounts.chainlink_feed,
        &ctx.accounts.jitosol_stake_pool,
        &ctx.accounts.pyth_feed_account,
    )?;

    msg!("Price {}", price);
    Ok(())
}
