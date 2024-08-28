use anchor_lang::prelude::*;

use crate::{errors::PriceFeedError, state::PriceFeedState};

#[derive(Accounts)]
pub struct DevChangePrice<'info> {
    #[account(mut)]
    pub price_feed_state: Account<'info, PriceFeedState>,
}

pub fn dev_change_price_handler(ctx: Context<DevChangePrice>, new_price: u64) -> Result<()> {
    let price_feed_state = &mut ctx.accounts.price_feed_state;
    require!(price_feed_state._is_dev, PriceFeedError::OnlyDevMode);
    price_feed_state._dev_price = new_price;
    Ok(())
}
