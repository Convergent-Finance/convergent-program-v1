use std::cmp::min;

use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
    token_2022::{transfer_checked, TransferChecked},
};
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;

use crate::{
    errors::{PriceFeedError, StabilityPoolError},
    events::{CVGTPaidToDepositor, CollGainWithdrawn, UserDepositChanged},
    state::{
        get_epoch_scales, CommunityIssuanceConfig, EpochScale, PoolState, PriceFeedState,
        StabilityPoolDeposit, StabilityPoolState, Trove,
    },
};

#[derive(Accounts)]
pub struct WithdrawFromSP<'info> {
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
        constraint = lowest_trove.key() == pool_state.trove_tail
    )]
    pub lowest_trove: Option<Box<Account<'info, Trove>>>,

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

    #[account(
        mut,
        seeds = [
            b"price_feed", 
            pool_state.cvgt.as_ref()
        ],
        bump
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

impl<'info> WithdrawFromSP<'info> {
    pub fn transfer_usv_out(&self, amount: u64) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        let pool_state_key = self.pool_state.key();
        let auth_seed = &self.pool_state.stability_pool_seeds(&pool_state_key);

        let cpi_accounts = TransferChecked {
            from: self.sp_usv_vault.to_account_info(),
            to: self.depositor_stablecoin_ata.to_account_info(),
            authority: self.stability_pool_state.to_account_info(),
            mint: self.stablecoin.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        transfer_checked(
            CpiContext::new(cpi_program, cpi_accounts).with_signer(&[auth_seed]),
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

pub fn withdraw_from_sp_handler(ctx: Context<WithdrawFromSP>, usv_amt: u64) -> Result<()> {
    let cvgt_issuance = ctx.accounts.issue_cvgt()?;

    let current_epoch_scale_key = &ctx.accounts.current_epoch_scale.key();
    let depositor = ctx.accounts.depositor.key;
    let sp_deposit = &mut ctx.accounts.stability_pool_deposit;
    let sp_state = &mut ctx.accounts.stability_pool_state;
    let pool_state = &mut ctx.accounts.pool_state;
    let lowest_trove_option = &ctx.accounts.lowest_trove;
    let current_epoch_scale = &mut ctx.accounts.current_epoch_scale;

    current_epoch_scale.update_g(sp_state, cvgt_issuance);

    if usv_amt > 0 {
        if lowest_trove_option.is_some() {
            let lowest_trove = lowest_trove_option.as_ref().unwrap().as_ref();
            let price = ctx.accounts.price_feed_state.fetch_price(
                &ctx.accounts.chainlink_program,
                &ctx.accounts.chainlink_feed,
                &ctx.accounts.jitosol_stake_pool,
                &ctx.accounts.pyth_feed_account,
            )?;
            let icr = lowest_trove.get_current_icr(pool_state, price);
            require!(icr >= pool_state.mcr, StabilityPoolError::TroveUnderColl);
        } else {
            require!(
                pool_state.trove_tail == Pubkey::default(),
                StabilityPoolError::InvalidLowestTrove
            );
        }
    }

    sp_deposit.require_user_has_deposit()?;

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
    let usv_to_withdraw = min(usv_amt, compounded_usv_deposit);
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

    if usv_to_withdraw > 0 {
        sp_state.decrease_usv(usv_to_withdraw);
    }

    // Update deposit
    let new_deposit = compounded_usv_deposit.checked_sub(usv_to_withdraw).unwrap();
    sp_deposit.update_deposit_and_snapshot(sp_state, current_epoch_scale, *depositor, new_deposit);
    sp_deposit.claimable_coll = sp_deposit
        .claimable_coll
        .checked_add(depositor_coll_gain.try_into().unwrap())
        .unwrap();
    sp_deposit.claimable_cvgt = sp_deposit.claimable_cvgt.checked_add(cvgt_gain).unwrap();
    sp_state.decrease_coll(depositor_coll_gain);

    // Transfer USV to user
    ctx.accounts.transfer_usv_out(usv_to_withdraw)?;

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
