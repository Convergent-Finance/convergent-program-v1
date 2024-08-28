use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
    token_2022::{transfer_checked, TransferChecked},
};

use crate::{
    errors::StabilityPoolError,
    events::{CVGTPaidToDepositor, CollGainWithdrawn, UserDepositChanged},
    state::{
        get_epoch_scales, CommunityIssuanceConfig, EpochScale, PoolState, StabilityPoolDeposit,
        StabilityPoolState,
    },
};

#[derive(Accounts)]
pub struct ProvideToSP<'info> {
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
        init_if_needed,
        payer = depositor,
        space = 8 + StabilityPoolDeposit::INIT_SPACE,
        seeds = [
            b"sp-deposit",
            stability_pool_state.key().as_ref(),
            depositor.key().as_ref(),
        ],
        bump
    )]
    pub stability_pool_deposit: Box<Account<'info, StabilityPoolDeposit>>,

    #[account(
        mut,
        seeds = [
            b"epoch-scale",
            stability_pool_state.key().as_ref(),
            stability_pool_state.current_epoch.to_le_bytes().as_ref(),
            stability_pool_state.current_scale.to_le_bytes().as_ref(),
        ],
        bump
    )]
    pub current_epoch_scale: Box<Account<'info, EpochScale>>,

    #[account(
        mut,
        constraint = stablecoin.key() == pool_state.stablecoin
    )]
    pub stablecoin: Box<Account<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = stablecoin,
        associated_token::authority = stability_pool_state
    )]
    pub sp_usv_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = stablecoin,
        associated_token::authority = depositor
    )]
    pub depositor_stablecoin_ata: Box<Account<'info, TokenAccount>>,

    #[account(mut)]
    pub depositor: Signer<'info>,

    /// CHECK: This account will check by CommunityIssuanceProgram
    #[account(
        constraint = cvgt.key() == stability_pool_state.cvgt
    )]
    pub cvgt: UncheckedAccount<'info>,

    #[account(
        mut,
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

impl<'info> ProvideToSP<'info> {
    pub fn transfer_usv_in(&self, amount: u64) -> Result<()> {
        let cpi_accounts = TransferChecked {
            from: self.depositor_stablecoin_ata.to_account_info(),
            to: self.sp_usv_vault.to_account_info(),
            authority: self.depositor.to_account_info(),
            mint: self.stablecoin.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        transfer_checked(
            CpiContext::new(cpi_program, cpi_accounts),
            amount,
            self.stablecoin.decimals,
        )
    }

    pub fn issue_cvgt(&mut self) -> Result<u64> {
        let config = &mut self.community_issuance_config;

        let issued = config.issue_token()?;
        Ok(issued)
    }
}

pub fn provide_to_sp_handler(ctx: Context<ProvideToSP>, usv_amt: u64) -> Result<()> {
    let cvgt_issuance = ctx.accounts.issue_cvgt()?;

    let current_epoch_scale_key = &ctx.accounts.current_epoch_scale.key();
    let depositor = ctx.accounts.depositor.key;
    let sp_deposit = &mut ctx.accounts.stability_pool_deposit;
    let sp_state = &mut ctx.accounts.stability_pool_state;
    let current_epoch_scale = &mut ctx.accounts.current_epoch_scale;

    require!(usv_amt > 0, StabilityPoolError::ZeroAmount);

    current_epoch_scale.update_g(sp_state, cvgt_issuance);

    let (first_epoch_scale, second_epoch_scale) = get_epoch_scales(
        &ctx.remaining_accounts,
        current_epoch_scale_key,
        &current_epoch_scale,
        sp_state,
        sp_deposit,
    )?;

    let depositor_coll_gain = sp_deposit
        .get_depositor_coll_gain(&first_epoch_scale, &second_epoch_scale)
        .unwrap();
    let compounded_usv_deposit = sp_deposit.get_compounded_usv_deposit(sp_state).unwrap();
    let usv_loss = sp_deposit
        .initial_value
        .checked_sub(compounded_usv_deposit)
        .unwrap();

    // First pay out any CVGT gains
    let cvgt_gain = sp_deposit
        .get_cvgt_gain(&first_epoch_scale, &second_epoch_scale)
        .unwrap();
    emit!(CVGTPaidToDepositor {
        depositor: *depositor,
        cvgt_gain
    });

    sp_state.increase_usv(usv_amt);

    let new_deposit = compounded_usv_deposit.checked_add(usv_amt).unwrap();
    sp_deposit.update_deposit_and_snapshot(sp_state, current_epoch_scale, *depositor, new_deposit);
    sp_deposit.claimable_coll = sp_deposit
        .claimable_coll
        .checked_add(depositor_coll_gain.try_into().unwrap())
        .unwrap();
    sp_deposit.claimable_cvgt = sp_deposit.claimable_cvgt.checked_add(cvgt_gain).unwrap();
    sp_state.decrease_coll(depositor_coll_gain);

    // Transfer USV to pool
    ctx.accounts.transfer_usv_in(usv_amt)?;

    emit!(UserDepositChanged {
        depositor: *depositor,
        new_deposit
    });

    emit!(CollGainWithdrawn {
        depositor: *depositor,
        coll: depositor_coll_gain,
        usv_loss
    });

    Ok(())
}
