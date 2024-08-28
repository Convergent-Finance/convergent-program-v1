use crate::{
    errors::{BorrowerOpsError, PriceFeedError},
    events::{Operation, TroveUpdated, USVBorrowingFeePaid},
    state::{CVGTStakingPoolState, CommunityIssuanceConfig, PoolState, PriceFeedState, Trove},
    utils::{
        require_new_icr_is_above_old_icr, require_no_coll_withdrawal, require_non_zero_adjustment,
        require_non_zero_debt_change, require_sufficient_usv_balance, require_user_accepts_fee,
        require_valid_borrow_max_fee_percentage,
    },
};
use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
    token_2022::{burn, mint_to, transfer_checked, Burn, MintTo, TransferChecked},
};
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;
use solana_program::program::invoke;

#[derive(Accounts)]
pub struct AdjustTrove<'info> {
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
    pub cur_next_trove: Option<Box<Account<'info, Trove>>>,

    #[account(mut)]
    pub cur_prev_trove: Option<Box<Account<'info, Trove>>>,

    #[account(mut)]
    pub new_next_trove: Option<Box<Account<'info, Trove>>>,

    #[account(mut)]
    pub new_prev_trove: Option<Box<Account<'info, Trove>>>,

    #[account(
        mut,
        associated_token::mint = collateral,
        associated_token::authority = borrower,
    )]
    user_coll_ata: Box<Account<'info, TokenAccount>>,

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
        associated_token::authority = token_authority
    )]
    pub collateral_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = stablecoin,
        associated_token::authority = borrower
    )]
    pub borrower_stablecoin_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = stablecoin,
        associated_token::authority = cvgt_staking_state
    )]
    pub stablecoin_cvgt_staking_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        constraint = stablecoin.key() == pool_state.stablecoin
    )]
    pub stablecoin: Box<Account<'info, Mint>>,

    #[account(
        mut,
        constraint = collateral.key() == pool_state.collateral
    )]
    pub collateral: Box<Account<'info, Mint>>,

    #[account(mut)]
    pub borrower: Signer<'info>,

    #[account(
        seeds = [
            b"community-issuance",
            pool_state.cvgt.as_ref()
        ],
        bump
    )]
    pub community_issuance_config: Box<Account<'info, CommunityIssuanceConfig>>,

    #[account(
        mut,
        constraint = cvgt_staking_state.key() == pool_state.cvgt_staking_state
    )]
    /// CHECK: At the beginning, protocol fee will be sent to Multisig
    /// Only after CVGT token release, fee will be sent to CVGTStakingPool
    pub cvgt_staking_state: AccountInfo<'info>,

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
    #[account(
        mut,
        constraint = jitosol_stake_pool.key() == price_feed_state.jitosol_stake_pool @ PriceFeedError::JitoSolStakePoolWrong
    )]
    pub jitosol_stake_pool: UncheckedAccount<'info>,
    /// CHECK: This account will check by StakeProgram
    #[account(mut)]
    pub jitosol_stake_withdraw_authority: UncheckedAccount<'info>,
    /// CHECK: This account will check by StakeProgram
    #[account(mut)]
    pub reserve_stake_account: UncheckedAccount<'info>,
    /// CHECK: This account will check by StakeProgram
    #[account(mut)]
    pub manager_fee: UncheckedAccount<'info>,
    /// CHECK: This account will check by StakeProgram
    #[account(mut)]
    pub referrer_fee: UncheckedAccount<'info>,
    /// CHECK: This account will check by StakeProgram
    #[account(
        mut,
        constraint = stake_program.key() == spl_stake_pool::ID
    )]
    pub stake_program: UncheckedAccount<'info>,
    #[account(
        constraint = chainlink_program.key() == chainlink_solana::ID
    )]
    /// CHECK: This is the Chainlink program library
    pub chainlink_program: AccountInfo<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> AdjustTrove<'info> {
    pub fn transfer_coll_in_ctx(&self) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
        let cpi_accounts = TransferChecked {
            from: self.user_coll_ata.to_account_info(),
            to: self.collateral_vault.to_account_info(),
            authority: self.borrower.to_account_info(),
            mint: self.collateral.to_account_info(),
        };
        let cpi_program = self.token_program.to_account_info();
        CpiContext::new(cpi_program, cpi_accounts)
    }

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

    pub fn mint_stablecoin_ctx(&self) -> CpiContext<'_, '_, '_, 'info, MintTo<'info>> {
        let cpi_accounts = MintTo {
            mint: self.stablecoin.to_account_info(),
            to: self.borrower_stablecoin_ata.to_account_info(),
            authority: self.token_authority.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        CpiContext::new(cpi_program, cpi_accounts)
    }

    pub fn burn_stablecoin_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Burn<'info>> {
        let cpi_accounts = Burn {
            mint: self.stablecoin.to_account_info(),
            from: self.borrower_stablecoin_ata.to_account_info(),
            authority: self.borrower.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        CpiContext::new(cpi_program, cpi_accounts)
    }

    pub fn mint_stablecoin_to_staking_pool_ctx(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, MintTo<'info>> {
        let cpi_accounts = MintTo {
            mint: self.stablecoin.to_account_info(),
            to: self.stablecoin_cvgt_staking_vault.to_account_info(),
            authority: self.token_authority.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        CpiContext::new(cpi_program, cpi_accounts)
    }

    pub fn increase_f_usv(&self, usv_fee: u64) -> Result<()> {
        let cvgt_staking_account = &self.cvgt_staking_state;
        let mut data = cvgt_staking_account.try_borrow_mut_data()?;
        let mut cvgt_staking = CVGTStakingPoolState::try_deserialize(&mut data.as_ref())
            .expect("Error Deserializing Data");
        cvgt_staking.increase_f_usv(usv_fee);
        cvgt_staking.try_serialize(&mut data.as_mut())?;
        Ok(())
    }

    pub fn stake_sol_for_jitosol(&mut self, amt: u64) -> Result<u64> {
        let token_program = &self.token_program;
        let system_program = &self.system_program;
        let collateral_vault = &mut self.collateral_vault;
        let borrower = &self.borrower;
        let collateral = &self.collateral;
        let stake_program = &self.stake_program;
        let jitosol_stake_pool = &self.jitosol_stake_pool;
        let reserve_stake_account = &self.reserve_stake_account;
        let jitosol_stake_withdraw_authority = &self.jitosol_stake_withdraw_authority;
        let manager_fee = &self.manager_fee;
        let referrer_fee = &self.referrer_fee;

        let balance_before = collateral_vault.amount;
        let ix = spl_stake_pool::instruction::deposit_sol(
            stake_program.key,
            jitosol_stake_pool.key,
            jitosol_stake_withdraw_authority.key,
            reserve_stake_account.key,
            borrower.key,
            &collateral_vault.key(),
            manager_fee.key,
            referrer_fee.key,
            &collateral.key(),
            token_program.key,
            amt,
        );

        invoke(
            &ix,
            &[
                jitosol_stake_pool.to_account_info(),
                jitosol_stake_withdraw_authority.to_account_info(),
                reserve_stake_account.to_account_info(),
                borrower.to_account_info(),
                collateral_vault.to_account_info(),
                manager_fee.to_account_info(),
                referrer_fee.to_account_info(),
                collateral.to_account_info(),
                system_program.to_account_info(),
                token_program.to_account_info(),
            ],
        )?;
        collateral_vault.reload()?;
        let balance_after = collateral_vault.amount;

        Ok(balance_after.checked_sub(balance_before).unwrap())
    }
}

pub fn adjust_trove_handler(
    ctx: Context<AdjustTrove>,
    max_fee_percentage: u64,
    is_lamport: bool,
    coll_change: u64,
    is_coll_increase: bool,
    usv_change: u64,
    is_debt_increase: bool,
) -> Result<()> {
    let coll_change = if is_lamport {
        require!(is_coll_increase, BorrowerOpsError::IsLamportNotSupported);
        ctx.accounts.stake_sol_for_jitosol(coll_change)?
    } else {
        coll_change
    };

    let borrower = &ctx.accounts.borrower.key();

    let price = ctx.accounts.price_feed_state.fetch_price(
        &ctx.accounts.chainlink_program,
        &ctx.accounts.chainlink_feed,
        &ctx.accounts.jitosol_stake_pool,
        &ctx.accounts.pyth_feed_account,
    )?;

    let cur_next_trove = &mut ctx.accounts.cur_next_trove;
    let cur_prev_trove = &mut ctx.accounts.cur_prev_trove;
    let new_next_trove = &mut ctx.accounts.new_next_trove;
    let new_prev_trove = &mut ctx.accounts.new_prev_trove;
    let community_issuance_config = &ctx.accounts.community_issuance_config;
    let trove = &mut ctx.accounts.trove;
    let pool_state = &mut ctx.accounts.pool_state;
    let trove_key = trove.key();

    // Check is recovery
    let is_recovery_mode = pool_state.check_recovery_mode(price);

    if is_debt_increase {
        require_valid_borrow_max_fee_percentage(max_fee_percentage, is_recovery_mode)?;
        require_non_zero_debt_change(usv_change)?;
    }
    require_non_zero_adjustment(coll_change, usv_change)?;
    trove.require_trove_active()?;

    pool_state.apply_pending_reward(trove)?;

    let mut net_debt_change = usv_change;
    let mut usv_fee = 0;

    if is_debt_increase && !is_recovery_mode {
        usv_fee = trigger_borrowing_fee(pool_state, usv_change, max_fee_percentage)?;
        net_debt_change = net_debt_change.checked_add(usv_fee).unwrap();
    }

    let debt = trove.debt;
    let coll = trove.coll;

    let old_icr = trove.get_icr(price);
    let new_icr = trove.get_new_icr_from_trove_change(
        coll_change,
        is_coll_increase,
        usv_change,
        is_debt_increase,
        price,
    );
    if !is_coll_increase {
        require!(
            coll_change <= coll,
            BorrowerOpsError::CollateralWithdrawExceedBalance
        );
    }

    require_valid_adjustment_in_current_mode(
        pool_state,
        is_recovery_mode,
        coll_change,
        is_coll_increase,
        net_debt_change,
        is_debt_increase,
        price,
        old_icr,
        new_icr,
    )?;

    if !is_debt_increase && usv_change > 0 {
        pool_state.require_at_least_min_net_debt(
            pool_state
                .get_net_debt(debt)
                .checked_sub(net_debt_change)
                .unwrap(),
        )?;
        pool_state.require_valid_usv_repayment(debt, net_debt_change)?;

        require_sufficient_usv_balance(
            ctx.accounts.borrower_stablecoin_ata.amount,
            net_debt_change,
        )?;
    }

    let new_nicr = trove.get_new_norminal_icr_from_trove_change(
        coll_change,
        is_coll_increase,
        net_debt_change,
        is_debt_increase,
    );
    let (new_coll, new_debt) = trove.update_from_adjustment(
        coll_change,
        is_coll_increase,
        net_debt_change,
        is_debt_increase,
    );
    let stake = trove.update_stake_and_total_stakes(pool_state);

    trove.re_insert(
        trove_key,
        new_nicr,
        cur_prev_trove,
        cur_next_trove,
        new_prev_trove,
        new_next_trove,
        pool_state,
    )?;

    emit!(TroveUpdated {
        borrower: borrower.key(),
        debt: new_debt,
        coll: new_coll,
        stake,
        operation: Operation::AdjustTrove
    });

    emit!(USVBorrowingFeePaid {
        borrower: borrower.key(),
        usv_fee
    });

    update_pool_state(
        pool_state,
        coll_change,
        is_coll_increase,
        usv_change,
        is_debt_increase,
        net_debt_change,
    );

    let pool_config_key = pool_state.key();
    let token_auth_bump = pool_state.token_auth_bump;
    let authority_seed = &[
        &b"token-authority"[..],
        pool_config_key.as_ref(),
        token_auth_bump.as_ref(),
    ];

    mint_to(
        ctx.accounts
            .mint_stablecoin_to_staking_pool_ctx()
            .with_signer(&[&authority_seed[..]]),
        usv_fee,
    )?;
    if community_issuance_config.enable_emission {
        ctx.accounts.increase_f_usv(usv_fee)?;
    }

    move_tokens_from_adjust(
        ctx,
        coll_change,
        is_lamport,
        is_coll_increase,
        usv_change,
        is_debt_increase,
    )
}

fn update_pool_state(
    pool_state: &mut PoolState,
    coll_change: u64,
    is_coll_increase: bool,
    usv_change: u64,
    is_debt_increase: bool,
    net_debt_change: u64,
) {
    if is_debt_increase {
        pool_state.increase_active_debt(net_debt_change);
    } else {
        pool_state.decrease_active_debt(usv_change);
    }

    if is_coll_increase {
        pool_state.increase_active_coll(coll_change);
    } else {
        pool_state.decrease_active_coll(coll_change);
    }
}

fn move_tokens_from_adjust(
    ctx: Context<AdjustTrove>,
    coll_change: u64,
    is_lamport: bool,
    is_coll_increase: bool,
    usv_change: u64,
    is_debt_increase: bool,
) -> Result<()> {
    let pool_state = &ctx.accounts.pool_state;
    let pool_state_key = pool_state.key();
    let authority_seed = &pool_state.token_auth_seeds(&pool_state_key);

    if is_debt_increase {
        mint_to(
            ctx.accounts
                .mint_stablecoin_ctx()
                .with_signer(&[&authority_seed[..]]),
            usv_change,
        )?;
    } else {
        burn(ctx.accounts.burn_stablecoin_ctx(), usv_change)?;
    }

    if is_coll_increase {
        if !is_lamport {
            transfer_checked(
                ctx.accounts.transfer_coll_in_ctx(),
                coll_change,
                ctx.accounts.collateral.decimals,
            )?;
        }
    } else {
        transfer_checked(
            ctx.accounts
                .transfer_coll_out_ctx()
                .with_signer(&[&authority_seed[..]]),
            coll_change,
            ctx.accounts.collateral.decimals,
        )?;
    }
    Ok(())
}

fn trigger_borrowing_fee(
    pool_state: &mut PoolState,
    usv_amt: u64,
    max_fee_percentage: u64,
) -> Result<u64> {
    pool_state.decay_base_rate_from_borrowing()?;
    let usv_fee = pool_state.get_borrowing_fee(usv_amt);

    require_user_accepts_fee(usv_fee, usv_amt, max_fee_percentage)?;
    Ok(usv_fee)
}

fn require_valid_adjustment_in_current_mode(
    pool_state: &PoolState,
    is_recovery_mode: bool,
    coll_change: u64,
    is_coll_increase: bool,
    net_debt_change: u64,
    is_debt_increase: bool,
    price: u64,
    old_icr: u64,
    new_icr: u64,
) -> Result<()> {
    if is_recovery_mode {
        require_no_coll_withdrawal(coll_change, is_coll_increase)?;
        if is_debt_increase {
            pool_state.require_icr_is_above_ccr(new_icr)?;
            require_new_icr_is_above_old_icr(new_icr, old_icr)?;
        }
    } else {
        pool_state.require_icr_is_above_mcr(new_icr)?;
        let new_tcr = pool_state.get_new_tcr_from_trove_change(
            coll_change,
            is_coll_increase,
            net_debt_change,
            is_debt_increase,
            price,
        );
        pool_state.require_new_tcr_is_above_ccr(new_tcr)?;
    }
    Ok(())
}
