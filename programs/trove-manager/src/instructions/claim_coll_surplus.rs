use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
    token_2022::{transfer_checked, TransferChecked},
};

use crate::state::{PoolState, Trove};

#[derive(Accounts)]
pub struct ClaimCollSurplus<'info> {
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

    #[account(mut)]
    pub borrower: Signer<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> ClaimCollSurplus<'info> {
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
}

pub fn claim_coll_surplus_handler(ctx: Context<ClaimCollSurplus>) -> Result<()> {
    let pool_state = &mut ctx.accounts.pool_state;
    let trove = &mut ctx.accounts.trove;

    let amount_to_send = trove.clear_surplus();
    if amount_to_send > 0 {
        pool_state.decrease_total_surplus(amount_to_send);
        move_token(ctx, amount_to_send)?;
    }
    Ok(())
}

pub fn move_token(ctx: Context<ClaimCollSurplus>, coll: u64) -> Result<()> {
    let pool_state = &ctx.accounts.pool_state;
    let pool_state_key = pool_state.key();
    let authority_seed = &pool_state.token_auth_seeds(&pool_state_key);

    transfer_checked(
        ctx.accounts
            .transfer_coll_out_ctx()
            .with_signer(&[&authority_seed[..]]),
        coll,
        ctx.accounts.collateral.decimals,
    )?;
    Ok(())
}
