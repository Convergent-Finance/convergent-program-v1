use anchor_lang::prelude::*;
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;

use crate::{
    constants::DEPLOYER,
    errors::PriceFeedError,
    state::{PriceFeedState, Status},
    utils::{is_pyth_broken, is_pyth_frozen},
};

#[derive(Accounts)]
pub struct InitializePriceFeed<'info> {
    #[account(
        init,
        payer = creator,
        space = 8 + PriceFeedState::INIT_SPACE,
        seeds = [
            b"price_feed", 
            cvgt.key().as_ref()
        ],
        bump
    )]
    pub price_feed_state: Account<'info, PriceFeedState>,

    #[account()]
    pub pyth_feed_account: Account<'info, PriceUpdateV2>,

    #[account(
        mut,
        constraint = creator.key() == DEPLOYER
    )]
    pub creator: Signer<'info>,

    /// CHECK: CVGT Token
    pub cvgt: AccountInfo<'info>,

    /// CHECK: We're reading data from this specified chainlink feed
    pub chainlink_feed: AccountInfo<'info>,
    /// CHECK: This is the JitoSol Staking Pool
    pub jitosol_stake_pool: AccountInfo<'info>,

    pub system_program: Program<'info, System>,
}

pub fn initialize_price_feed_handler(
    ctx: Context<InitializePriceFeed>,
    is_dev: bool,
) -> Result<()> {
    let creator = ctx.accounts.creator.key();
    let chainlink_feed = ctx.accounts.chainlink_feed.key();
    let jitosol_stake_pool = ctx.accounts.jitosol_stake_pool.key();
    let price_info = &ctx.accounts.pyth_feed_account;
    let pyth_price_message = price_info
        .get_price_unchecked(&price_info.price_message.feed_id)
        .unwrap();

    let is_pyth_working =
        !is_pyth_broken(&pyth_price_message) && !is_pyth_frozen(&pyth_price_message);
    require!(is_pyth_working, PriceFeedError::InitializePythNotWorking);

    let price_feed_info = &mut ctx.accounts.price_feed_state;
    **price_feed_info = PriceFeedState {
        creator,
        chainlink_feed,
        jitosol_stake_pool,
        pyth_feed_account: ctx.accounts.pyth_feed_account.key(),
        bump: ctx.bumps.price_feed_state,
        status: Status::PythWorking,
        last_good_price: price_info.price_message.price.try_into().unwrap(),
        _is_dev: is_dev,
        _dev_price: 130_000_000_000,
    };

    Ok(())
}
