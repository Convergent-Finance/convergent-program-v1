use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
    token_2022::{burn, transfer_checked, Burn, TransferChecked},
};
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;

use crate::{
    errors::PriceFeedError,
    state::{PoolState, PriceFeedState, Trove, TroveStatus},
    utils::require_sufficient_usv_balance,
};

#[derive(Accounts)]
pub struct CloseTrove<'info> {
    #[account(mut)]
    pub pool_state: Box<Account<'info, PoolState>>,

    #[account(
        mut,
        seeds = [
            b"trove", 
            pool_state.key().as_ref(),
            borrower.key().as_ref(),
        ],
        bump
    )]
    pub trove: Box<Account<'info, Trove>>,

    #[account(mut)]
    pub next_trove: Option<Box<Account<'info, Trove>>>,

    #[account(mut)]
    pub prev_trove: Option<Box<Account<'info, Trove>>>,

    #[account(
        init_if_needed,
        payer = borrower,
        associated_token::mint = stablecoin,
        associated_token::authority = borrower
    )]
    pub borrower_stablecoin_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = stablecoin.key() == pool_state.stablecoin
    )]
    pub stablecoin: Box<Account<'info, Mint>>,

    #[account(
        constraint = collateral.key() == pool_state.collateral
    )]
    pub collateral: Box<Account<'info, Mint>>,

    /// CHECK: This account is not read or written
    #[account(
        seeds = [
            b"token-authority",
            pool_state.key().as_ref()
        ],
        bump
    )]
    pub token_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        associated_token::mint = collateral,
        associated_token::authority = borrower,
    )]
    user_coll_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = collateral,
        associated_token::authority = token_authority
    )]
    pub collateral_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = stablecoin,
        associated_token::authority = token_authority
    )]
    pub gas_compensation_vault: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub borrower: Signer<'info>,

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

    #[account(
        constraint = jitosol_stake_pool.key == &price_feed_state.jitosol_stake_pool @ PriceFeedError::StakingPoolWrong
    )]
    /// CHECK: This is the Jito staking pool
    pub jitosol_stake_pool: AccountInfo<'info>,

    #[account(
        constraint = chainlink_program.key() == chainlink_solana::ID
    )]
    /// CHECK: This is the Chainlink program library
    pub chainlink_program: AccountInfo<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> CloseTrove<'info> {
    pub fn transfer_coll_out_ctx(&self) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
        let cpi_accounts = TransferChecked {
            from: self.collateral_vault.to_account_info(),
            to: self.user_coll_ata.to_account_info(),
            authority: self.token_authority.to_account_info(),
            mint: self.collateral.to_account_info(),
        };
        let cpi_program = self.token_program.to_account_info();
        CpiContext::new(cpi_program, cpi_accounts)
    }

    pub fn burn_stablecoin_from_user_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Burn<'info>> {
        let cpi_accounts = Burn {
            mint: self.stablecoin.to_account_info(),
            from: self.borrower_stablecoin_ata.to_account_info(),
            authority: self.borrower.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        CpiContext::new(cpi_program, cpi_accounts)
    }

    pub fn burn_stablecoin_from_gas_compensation_ctx(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, Burn<'info>> {
        let cpi_accounts = Burn {
            mint: self.stablecoin.to_account_info(),
            from: self.gas_compensation_vault.to_account_info(),
            authority: self.token_authority.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        CpiContext::new(cpi_program, cpi_accounts)
    }
}

pub fn close_trove_handler(ctx: Context<CloseTrove>) -> Result<()> {
    let price = ctx.accounts.price_feed_state.fetch_price(
        &ctx.accounts.chainlink_program,
        &ctx.accounts.chainlink_feed,
        &ctx.accounts.jitosol_stake_pool,
        &ctx.accounts.pyth_feed_account,
    )?;
    let trove = &mut ctx.accounts.trove;
    let trove_id = trove.key();
    let next_trove = &mut ctx.accounts.next_trove;
    let prev_trove = &mut ctx.accounts.prev_trove;
    let pool_state = &mut ctx.accounts.pool_state;

    trove.require_trove_active()?;
    pool_state.require_not_in_recovery_mode(price)?;

    pool_state.apply_pending_reward(trove)?;

    let coll = trove.coll;
    let debt = trove.debt;

    require_sufficient_usv_balance(
        ctx.accounts.borrower_stablecoin_ata.amount,
        debt.checked_sub(pool_state.gas_compensation).unwrap(),
    )?;

    let new_tcr = pool_state.get_new_tcr_from_trove_change(coll, false, debt, false, price);
    pool_state.require_new_tcr_is_above_ccr(new_tcr)?;

    // Remove stake
    trove.remove_stake(pool_state);

    // Update trove data
    trove.close_trove(pool_state, TroveStatus::ClosedByOwner)?;

    // Update pool state
    pool_state.decrease_active_debt(debt);
    pool_state.decrease_active_coll(coll);

    // Remove trove from sorted troves
    trove.remove_sorted(trove_id, prev_trove, next_trove, pool_state)?;

    // Move tokens
    move_tokens_from_close(ctx, coll, debt)?;

    Ok(())
}

pub fn move_tokens_from_close(ctx: Context<CloseTrove>, coll: u64, debt: u64) -> Result<()> {
    let pool_state = &ctx.accounts.pool_state;
    let pool_state_key = pool_state.key();
    let authority_seed = &pool_state.token_auth_seeds(&pool_state_key);

    burn(
        ctx.accounts.burn_stablecoin_from_user_ctx(),
        debt.checked_sub(pool_state.gas_compensation).unwrap(),
    )?;
    burn(
        ctx.accounts
            .burn_stablecoin_from_gas_compensation_ctx()
            .with_signer(&[&authority_seed[..]]),
        pool_state.gas_compensation,
    )?;
    transfer_checked(
        ctx.accounts
            .transfer_coll_out_ctx()
            .with_signer(&[&authority_seed[..]]),
        coll,
        ctx.accounts.collateral.decimals,
    )?;
    Ok(())
}
