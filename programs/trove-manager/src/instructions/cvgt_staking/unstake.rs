use std::cmp::min;

use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
    token_2022::{transfer_checked, TransferChecked},
};

use crate::{
    errors::CVGTStakingError,
    events::{StakeChanged, StakingGainsWithdrawn, TotalCVGTStakedUpdated},
    state::{CVGTStakingInfo, CVGTStakingPoolState},
};

#[derive(Accounts)]
pub struct Unstake<'info> {
    #[account(
        mut,
        seeds = [
            b"staking-state",
            cvgt.key().as_ref()
        ],
        bump
    )]
    pub pool_state: Box<Account<'info, CVGTStakingPoolState>>,

    #[account(
        mut,
        seeds = [
            b"info",
            pool_state.key().as_ref(),
            user.key().as_ref()
        ],
        bump
    )]
    pub staking_info: Box<Account<'info, CVGTStakingInfo>>,

    #[account(
        mut,
        constraint = cvgt.key() == pool_state.cvgt
    )]
    pub cvgt: Box<Account<'info, Mint>>,

    #[account(
        mut,
        constraint = usv.key() == pool_state.usv
    )]
    pub usv: Box<Account<'info, Mint>>,

    #[account(
        mut,
        constraint = collateral.key() == pool_state.collateral
    )]
    pub collateral: Box<Account<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = pool_state.cvgt,
        associated_token::authority = pool_state
    )]
    pub cvgt_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = pool_state.collateral,
        associated_token::authority = pool_state
    )]
    pub coll_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = pool_state.usv,
        associated_token::authority = pool_state
    )]
    pub usv_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = cvgt,
        associated_token::authority = user
    )]
    pub cvgt_user_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = collateral,
        associated_token::authority = user
    )]
    pub coll_user_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = usv,
        associated_token::authority = user
    )]
    pub usv_user_ata: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> Unstake<'info> {
    pub fn transfer_cvgt(&self, amount: u64) -> Result<()> {
        let authority_seed = &self.pool_state.seeds();

        let cpi_accounts = TransferChecked {
            from: self.cvgt_vault.to_account_info(),
            to: self.cvgt_user_ata.to_account_info(),
            authority: self.pool_state.to_account_info(),
            mint: self.cvgt.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        transfer_checked(
            cpi_ctx.with_signer(&[&authority_seed[..]]),
            amount,
            self.cvgt.decimals,
        )
    }

    pub fn transfer_coll_out(&self, amount: u64) -> Result<()> {
        let authority_seed = &self.pool_state.seeds();

        let cpi_accounts = TransferChecked {
            from: self.coll_vault.to_account_info(),
            to: self.coll_user_ata.to_account_info(),
            authority: self.pool_state.to_account_info(),
            mint: self.collateral.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        transfer_checked(
            cpi_ctx.with_signer(&[&authority_seed[..]]),
            amount,
            self.collateral.decimals,
        )
    }

    pub fn transfer_usv_out(&self, amount: u64) -> Result<()> {
        let authority_seed = &self.pool_state.seeds();

        let cpi_accounts = TransferChecked {
            from: self.usv_vault.to_account_info(),
            to: self.usv_user_ata.to_account_info(),
            authority: self.pool_state.to_account_info(),
            mint: self.usv.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);

        transfer_checked(
            cpi_ctx.with_signer(&[&authority_seed[..]]),
            amount,
            self.usv.decimals,
        )
    }
}

pub fn unstake_handler(ctx: Context<Unstake>, cvgt_amt: u64) -> Result<()> {
    let staking_info = &mut ctx.accounts.staking_info;
    let pool_state = &mut ctx.accounts.pool_state;
    let user_key = &ctx.accounts.user.key();

    let current_stake = staking_info.balance;
    require!(current_stake > 0, CVGTStakingError::UserNotHasStake);

    // Grab any accumulated Coll and USV gains from the current stake
    let coll_gain = staking_info.get_pending_coll_gain(pool_state);
    let usv_gain = staking_info.get_pending_usv_gain(pool_state);

    staking_info.update_snapshot(user_key, pool_state);

    if cvgt_amt > 0 {
        let cvgt_to_withdraw = min(cvgt_amt, current_stake);
        let new_stake = current_stake.checked_sub(cvgt_to_withdraw).unwrap();

        // Decrease user's stake and total CVGT staked
        staking_info.balance = new_stake;
        pool_state.total_cvgt_staked = pool_state
            .total_cvgt_staked
            .checked_sub(cvgt_to_withdraw)
            .unwrap();
        emit!(TotalCVGTStakedUpdated {
            total_cvgt_staked: pool_state.total_cvgt_staked
        });

        // Transfer unstaked CVGT to user
        ctx.accounts.transfer_cvgt(cvgt_to_withdraw)?;

        emit!(StakeChanged {
            staker: *user_key,
            new_stake
        });
    }

    emit!(StakingGainsWithdrawn {
        staker: *user_key,
        usv_gain,
        coll_gain,
    });

    // Send accumulated USV and Coll gains to the caller
    ctx.accounts.transfer_usv_out(usv_gain)?;
    ctx.accounts.transfer_coll_out(coll_gain)?;
    Ok(())
}
