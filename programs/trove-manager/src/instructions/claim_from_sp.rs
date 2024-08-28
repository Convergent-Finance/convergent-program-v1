use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
    token_2022::{transfer_checked, TransferChecked},
};

use crate::state::{CommunityIssuanceConfig, PoolState, StabilityPoolDeposit, StabilityPoolState};

#[derive(Accounts)]
pub struct ClaimFromSP<'info> {
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
        mut,
        seeds = [
            b"sp-deposit",
            stability_pool_state.key().as_ref(),
            depositor.key().as_ref(),
        ],
        bump
    )]
    pub stability_pool_deposit: Box<Account<'info, StabilityPoolDeposit>>,

    #[account(
        constraint = collateral.key() == pool_state.collateral
    )]
    pub collateral: Box<Account<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = collateral,
        associated_token::authority = stability_pool_state
    )]
    pub sp_coll_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = collateral,
        associated_token::authority = depositor,
    )]
    pub depositor_coll_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = cvgt,
        associated_token::authority = depositor,
    )]
    pub depositor_cvgt_ata: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub depositor: Signer<'info>,

    /// CHECK: This account will check by CommunityIssuanceProgram
    #[account(
        mut,
        constraint = cvgt.key() == pool_state.cvgt
    )]
    pub cvgt: Box<Account<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = community_issuance_config.cvgt,
        associated_token::authority = community_issuance_config
    )]
    pub community_issuance_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        seeds = [
            b"community-issuance",
            pool_state.cvgt.as_ref()
        ],
        bump
    )]
    pub community_issuance_config: Box<Account<'info, CommunityIssuanceConfig>>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> ClaimFromSP<'info> {
    pub fn send_cvgt_to_depositor(&self, amount: u64) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        let cpi_accounts = TransferChecked {
            from: self.community_issuance_vault.to_account_info(),
            to: self.depositor_cvgt_ata.to_account_info(),
            authority: self.community_issuance_config.to_account_info(),
            mint: self.cvgt.to_account_info(),
        };
        let cpi_program = self.token_program.to_account_info();
        let cpi_context = CpiContext::new(cpi_program, cpi_accounts);
        let auth_seed = &self.community_issuance_config.seeds();

        transfer_checked(
            cpi_context.with_signer(&[auth_seed]),
            amount,
            self.cvgt.decimals,
        )?;

        Ok(())
    }

    pub fn transfer_coll_out(&self, amount: u64) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        let pool_state_key = self.pool_state.key();
        let auth_seed = &self.pool_state.stability_pool_seeds(&pool_state_key);

        let cpi_accounts = TransferChecked {
            from: self.sp_coll_vault.to_account_info(),
            to: self.depositor_coll_ata.to_account_info(),
            authority: self.stability_pool_state.to_account_info(),
            mint: self.collateral.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        transfer_checked(
            CpiContext::new(cpi_program, cpi_accounts).with_signer(&[auth_seed]),
            amount,
            self.collateral.decimals,
        )
    }
}

pub fn claim_from_sp_handler(ctx: Context<ClaimFromSP>) -> Result<()> {
    let sp_deposit = &mut ctx.accounts.stability_pool_deposit;
    let claimable_coll = sp_deposit.claimable_coll;
    let claimable_cvgt = sp_deposit.claimable_cvgt;
    sp_deposit.claimable_coll = 0;
    sp_deposit.claimable_cvgt = 0;

    // Transfer CVGT to user
    ctx.accounts.send_cvgt_to_depositor(claimable_cvgt)?;
    // Transfer Coll to user
    ctx.accounts.transfer_coll_out(claimable_coll)?;

    Ok(())
}
