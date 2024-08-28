use crate::{
    constants::ONE_HUNDERED_PERCENT,
    errors::{BorrowerOpsError, PriceFeedError},
    events::{Liquidation, Operation, TroveLiquidated, TroveUpdated},
    math::compute_cr,
    state::{
        CommunityIssuanceConfig, EpochScale, LiquidationTotals, LiquidationValues, PoolState,
        PriceFeedState, StabilityPoolState, Trove, TroveStatus,
    },
};
use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
    token_2022::{burn, mint_to, transfer_checked, Burn, MintTo, TransferChecked},
};
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;

#[derive(Accounts)]
pub struct LiquidateTrove<'info> {
    #[account(mut)]
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
            b"trove", 
            pool_state.key().as_ref(),
            borrower.key().as_ref(),
        ],
        bump,
    )]
    pub trove: Box<Account<'info, Trove>>,

    #[account(
        mut,
        constraint = prev_trove.key() == trove.prev
    )]
    pub prev_trove: Option<Box<Account<'info, Trove>>>,

    #[account(
        mut,
        constraint = next_trove.key() == trove.next
    )]
    pub next_trove: Option<Box<Account<'info, Trove>>>,

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
        associated_token::mint = collateral,
        associated_token::authority = liquidator,
    )]
    liquidator_coll_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = stablecoin,
        associated_token::authority = liquidator,
    )]
    liquidator_stablecoin_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = collateral,
        associated_token::authority = token_authority
    )]
    pub collateral_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = stablecoin,
        associated_token::authority = stability_pool_state
    )]
    pub sp_usv_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = collateral,
        associated_token::authority = stability_pool_state
    )]
    pub sp_coll_vault: Box<Account<'info, TokenAccount>>,

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

    /// CHECK: mock
    #[account(mut)]
    pub borrower: AccountInfo<'info>,

    #[account(mut)]
    pub liquidator: Signer<'info>,

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

impl<'info> LiquidateTrove<'info> {
    pub fn send_gas_compensation(&mut self, usv_amt: u64, coll_amt: u64) -> Result<()> {
        let pool_state = &self.pool_state;
        let pool_state_key = pool_state.key();
        let authority_seed = &pool_state.token_auth_seeds(&pool_state_key);

        // mint usv
        if usv_amt > 0 {
            let cpi_accounts = MintTo {
                mint: self.stablecoin.to_account_info(),
                to: self.liquidator_stablecoin_ata.to_account_info(),
                authority: self.token_authority.to_account_info(),
            };
            let cpi_program = self.token_program.to_account_info();
            mint_to(
                CpiContext::new(cpi_program, cpi_accounts).with_signer(&[&authority_seed[..]]),
                usv_amt,
            )?;
        }

        // transfer coll
        if coll_amt > 0 {
            let cpi_accounts = TransferChecked {
                from: self.collateral_vault.to_account_info(),
                to: self.liquidator_coll_ata.to_account_info(),
                authority: self.token_authority.to_account_info(),
                mint: self.collateral.to_account_info(),
            };
            let cpi_program = self.token_program.to_account_info();

            transfer_checked(
                CpiContext::new(cpi_program, cpi_accounts).with_signer(&[&authority_seed[..]]),
                coll_amt,
                self.collateral.decimals,
            )?;
        }

        Ok(())
    }

    pub fn burn_usv_from_stability_pool(&mut self, usv_amt: u64) -> Result<()> {
        let pool_state = &self.pool_state;
        let pool_state_key = pool_state.key();

        let auth_seed = &pool_state.stability_pool_seeds(&pool_state_key);

        let cpi_accounts = Burn {
            mint: self.stablecoin.to_account_info(),
            from: self.sp_usv_vault.to_account_info(),
            authority: self.stability_pool_state.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();
        burn(
            CpiContext::new(cpi_program, cpi_accounts).with_signer(&[&auth_seed[..]]),
            usv_amt,
        )?;
        Ok(())
    }

    pub fn transfer_coll_from_active_pool_to_sp(&mut self, coll_amt: u64) -> Result<()> {
        let pool_state = &self.pool_state;
        let pool_state_key = pool_state.key();
        let authority_seed = &pool_state.token_auth_seeds(&pool_state_key);

        let cpi_accounts = TransferChecked {
            from: self.collateral_vault.to_account_info(),
            to: self.sp_coll_vault.to_account_info(),
            authority: self.token_authority.to_account_info(),
            mint: self.collateral.to_account_info(),
        };
        let cpi_program = self.token_program.to_account_info();

        transfer_checked(
            CpiContext::new(cpi_program, cpi_accounts).with_signer(&[&authority_seed[..]]),
            coll_amt,
            self.collateral.decimals,
        )?;
        Ok(())
    }

    pub fn move_tokens(&mut self, totals: LiquidationTotals) -> Result<()> {
        // Transfer compensation to liquidator
        self.send_gas_compensation(
            totals.total_usv_gas_compensation,
            totals.total_coll_gas_compensation,
        )?;

        // Move token to Stability Pool
        self.transfer_coll_from_active_pool_to_sp(totals.total_coll_to_send_to_sp)?;
        self.burn_usv_from_stability_pool(totals.total_debt_to_offset)?;

        Ok(())
    }
}

pub fn liquidate_trove_handler(ctx: Context<LiquidateTrove>) -> Result<()> {
    let price = ctx.accounts.price_feed_state.fetch_price(
        &ctx.accounts.chainlink_program,
        &ctx.accounts.chainlink_feed,
        &ctx.accounts.jitosol_stake_pool,
        &ctx.accounts.pyth_feed_account,
    )?;

    let trove = &mut ctx.accounts.trove;
    let pool_state = &mut ctx.accounts.pool_state;
    let sp_state = &mut ctx.accounts.stability_pool_state;
    let current_epoch_scale = &mut ctx.accounts.current_epoch_scale;
    let prev_trove_option = &mut ctx.accounts.prev_trove;
    let next_trove_option = &mut ctx.accounts.next_trove;

    // now we will not handle the case that Stability pool has non-zero USV
    let usv_in_stab_pool = sp_state.total_usv_deposits;

    // Check is recovery
    let is_recovery_mode_at_start = pool_state.check_recovery_mode(price);

    let totals = if is_recovery_mode_at_start {
        get_totals_from_liquidate_recovery_mode(
            price,
            usv_in_stab_pool,
            trove,
            prev_trove_option,
            next_trove_option,
            pool_state,
        )
        .unwrap()
    } else {
        get_totals_from_liquidate_normal_mode(
            price,
            usv_in_stab_pool,
            trove,
            prev_trove_option,
            next_trove_option,
            pool_state,
        )
        .unwrap()
    };

    require!(
        totals.total_debt_in_sequence > 0,
        BorrowerOpsError::LiquidateZeroDebt
    );

    // StabilityPool offset
    if totals.total_debt_to_offset > 0 && usv_in_stab_pool > 0 {
        // _triggerCVGTIssuance
        let config = &mut ctx.accounts.community_issuance_config;
        let cvgt_issuance = config.issue_token()?;

        sp_state.offset(current_epoch_scale, &totals, cvgt_issuance);
    }
    pool_state.redistribute_debt_and_coll(
        totals.total_debt_to_redistribute,
        totals.total_coll_to_redistribute,
    );
    if totals.total_coll_surplus > 0 {
        pool_state.decrease_active_coll(totals.total_coll_surplus);
        pool_state.increase_total_surplus(totals.total_coll_surplus);
    }

    pool_state.update_system_snapshots_exclude_coll_remainder(totals.total_coll_gas_compensation);

    let liquidated_debt = totals.total_debt_in_sequence;
    let liquidated_coll = totals.total_coll_in_sequence
        - totals.total_coll_surplus
        - totals.total_coll_gas_compensation;

    emit!(Liquidation {
        debt: liquidated_debt,
        coll: liquidated_coll,
        total_usv_compensation: totals.total_coll_gas_compensation,
        total_coll_compensation: totals.total_usv_gas_compensation,
    });

    pool_state.move_coll_debt_from_liquidate(sp_state, &totals);

    ctx.accounts.move_tokens(totals)
}

fn get_totals_from_liquidate_normal_mode(
    price: u64,
    usv_in_stab_pool: u64,
    trove: &mut Account<'_, Trove>,
    prev_trove: &mut Option<Box<Account<'_, Trove>>>,
    next_trove: &mut Option<Box<Account<'_, Trove>>>,
    pool_state: &mut PoolState,
) -> Result<LiquidationTotals> {
    let mut totals: LiquidationTotals = Default::default();
    let remaining_usv_in_stab_pool: u64 = usv_in_stab_pool;

    let icr: u64 = trove.get_current_icr(pool_state, price);
    if icr < pool_state.mcr {
        let single_liquidation = liquidate_normal_mode(
            pool_state,
            trove,
            prev_trove,
            next_trove,
            remaining_usv_in_stab_pool,
        )
        .unwrap();
        // Add liquidation values to their respective running totals
        totals.add_liquidation_values(&single_liquidation);
    }

    Ok(totals)
}

fn get_totals_from_liquidate_recovery_mode(
    price: u64,
    usv_in_stab_pool: u64,
    trove: &mut Account<'_, Trove>,
    prev_trove: &mut Option<Box<Account<'_, Trove>>>,
    next_trove: &mut Option<Box<Account<'_, Trove>>>,
    pool_state: &mut PoolState,
) -> Result<LiquidationTotals> {
    let mut totals: LiquidationTotals = Default::default();
    let remaining_usv_in_stab_pool: u64 = usv_in_stab_pool;
    // Skip non-active trove
    if trove.status != TroveStatus::Active {
        return Ok(totals);
    }
    let icr = trove.get_current_icr(pool_state, price);
    // Skip trove if ICR is greater than MCR and Stability Pool is empty
    if icr >= pool_state.mcr && remaining_usv_in_stab_pool == 0 {
        return Ok(totals);
    }
    let tcr = compute_cr(
        pool_state.get_entire_coll(),
        pool_state.get_entire_debt(),
        price,
    )
    .unwrap();
    let single_liquidation = liquidate_recovery_mode(
        pool_state,
        trove,
        prev_trove,
        next_trove,
        usv_in_stab_pool,
        tcr,
        icr,
        price,
    )?;
    totals.add_liquidation_values(&single_liquidation);
    Ok(totals)
}

fn liquidate_normal_mode(
    pool_state: &mut PoolState,
    trove: &mut Account<'_, Trove>,
    prev_trove: &mut Option<Box<Account<'_, Trove>>>,
    next_trove: &mut Option<Box<Account<'_, Trove>>>,
    usv_in_stab_pool: u64,
) -> Result<LiquidationValues> {
    let mut single_liquidation: LiquidationValues = Default::default();
    let trove_id = trove.key();

    let entire_debt_and_coll = trove.get_entire_debt_coll(pool_state);
    single_liquidation.entire_trove_debt = entire_debt_and_coll.0;
    single_liquidation.entire_trove_coll = entire_debt_and_coll.1;
    let pending_debt_reward = entire_debt_and_coll.2;
    let pending_coll_reward = entire_debt_and_coll.3;

    // moving pending debt/coll to active pool
    pool_state.move_pending_trove_rewards_to_active(pending_debt_reward, pending_coll_reward);
    // Remove stake
    trove.remove_stake(pool_state);

    single_liquidation.coll_gas_compensation =
        pool_state.get_coll_gas_compensation(single_liquidation.entire_trove_coll);
    single_liquidation.usv_gas_compensation = pool_state.gas_compensation;

    let coll_to_liquidate = single_liquidation
        .entire_trove_coll
        .checked_sub(single_liquidation.coll_gas_compensation)
        .unwrap();

    single_liquidation.offset_and_redistribute(coll_to_liquidate, usv_in_stab_pool);

    trove.close_trove(pool_state, TroveStatus::ClosedByLiquidation)?;
    trove.remove_sorted(trove_id, prev_trove, next_trove, pool_state)?;

    emit!(TroveLiquidated {
        borrower: trove.creator,
        debt: single_liquidation.entire_trove_debt,
        coll: single_liquidation.entire_trove_coll,
        operation: Operation::LiquidateInNormalMode,
    });

    emit!(TroveUpdated {
        borrower: trove.creator,
        debt: 0,
        coll: 0,
        stake: 0,
        operation: Operation::LiquidateInNormalMode,
    });

    Ok(single_liquidation)
}

fn liquidate_recovery_mode(
    pool_state: &mut PoolState,
    trove: &mut Account<'_, Trove>,
    prev_trove: &mut Option<Box<Account<'_, Trove>>>,
    next_trove: &mut Option<Box<Account<'_, Trove>>>,
    usv_in_stab_pool: u64,
    tcr: u64,
    icr: u64,
    price: u64,
) -> Result<LiquidationValues> {
    let mut single_liquidation: LiquidationValues = Default::default();
    let trove_id = trove.key();
    if pool_state.trove_size <= 1 {
        return Ok(single_liquidation);
    }
    let entire_debt_and_coll = trove.get_entire_debt_coll(pool_state);
    single_liquidation.entire_trove_debt = entire_debt_and_coll.0;
    single_liquidation.entire_trove_coll = entire_debt_and_coll.1;
    let pending_debt_reward = entire_debt_and_coll.2;
    let pending_coll_reward = entire_debt_and_coll.3;

    single_liquidation.coll_gas_compensation =
        pool_state.get_coll_gas_compensation(single_liquidation.entire_trove_coll);
    single_liquidation.usv_gas_compensation = pool_state.gas_compensation;
    let coll_to_liquidate = single_liquidation
        .entire_trove_coll
        .checked_sub(single_liquidation.coll_gas_compensation)
        .unwrap();

    // If ICR <= 100%, purely redistribute the Trove across all active Troves
    if icr <= ONE_HUNDERED_PERCENT {
        pool_state.move_pending_trove_rewards_to_active(pending_debt_reward, pending_coll_reward);
        trove.remove_stake(pool_state);
        single_liquidation.debt_to_offset = 0;
        single_liquidation.coll_to_send_to_sp = 0;
        single_liquidation.debt_to_redistribute = single_liquidation.entire_trove_debt;
        single_liquidation.coll_to_redistribute = coll_to_liquidate;
        trove.close_trove(pool_state, TroveStatus::ClosedByLiquidation)?;
        trove.remove_sorted(trove_id, prev_trove, next_trove, pool_state)?;
        emit!(TroveLiquidated {
            borrower: trove.key(),
            debt: single_liquidation.entire_trove_debt,
            coll: single_liquidation.entire_trove_coll,
            operation: Operation::LiquidateInRecoveryMode,
        });
        emit!(TroveUpdated {
            borrower: trove.key(),
            debt: 0,
            coll: 0,
            stake: 0,
            operation: Operation::LiquidateInRecoveryMode
        });

    // If 100% < ICR < MCR, offset as much as possible, and redistribute the remainder
    } else if (icr > ONE_HUNDERED_PERCENT) && (icr < pool_state.mcr) {
        pool_state.move_pending_trove_rewards_to_active(pending_debt_reward, pending_coll_reward);
        trove.remove_stake(pool_state);
        single_liquidation.offset_and_redistribute(coll_to_liquidate, usv_in_stab_pool);

        trove.close_trove(pool_state, TroveStatus::ClosedByLiquidation)?;
        trove.remove_sorted(trove_id, prev_trove, next_trove, pool_state)?;
        emit!(TroveLiquidated {
            borrower: trove.key(),
            debt: single_liquidation.entire_trove_debt,
            coll: single_liquidation.entire_trove_coll,
            operation: Operation::LiquidateInRecoveryMode,
        });
        emit!(TroveUpdated {
            borrower: trove.key(),
            debt: 0,
            coll: 0,
            stake: 0,
            operation: Operation::LiquidateInRecoveryMode
        });

    /*
     * If 110% <= ICR < current TCR (accounting for the preceding liquidations in the current sequence)
     * and there is USV in the Stability Pool, only offset, with no redistribution,
     * but at a capped rate of 1.1 and only if the whole debt can be liquidated.
     * The remainder due to the capped rate will be claimable as collateral surplus.
     */
    } else if (icr >= pool_state.mcr)
        && (icr < tcr)
        && (single_liquidation.entire_trove_debt <= usv_in_stab_pool)
    {
        pool_state.move_pending_trove_rewards_to_active(pending_debt_reward, pending_coll_reward);
        assert!(usv_in_stab_pool != 0);

        trove.remove_stake(pool_state);
        single_liquidation = pool_state.get_capped_offset_vals(
            single_liquidation.entire_trove_debt,
            single_liquidation.entire_trove_coll,
            price,
        )?;

        trove.close_trove(pool_state, TroveStatus::ClosedByLiquidation)?;
        trove.remove_sorted(trove_id, prev_trove, next_trove, pool_state)?;
        if single_liquidation.coll_surplus > 0 {
            trove.account_surplus(single_liquidation.coll_surplus);
        }
        emit!(TroveLiquidated {
            borrower: trove.key(),
            debt: single_liquidation.entire_trove_debt,
            coll: single_liquidation.coll_to_send_to_sp,
            operation: Operation::LiquidateInRecoveryMode,
        });
        emit!(TroveUpdated {
            borrower: trove.key(),
            debt: 0,
            coll: 0,
            stake: 0,
            operation: Operation::LiquidateInRecoveryMode
        });
    } else {
        // if (ICR >= MCR && ( ICR >= TCR || singleLiquidation.entireTroveDebt > USVInStabPool))
        return Ok(LiquidationValues::default());
    }
    Ok(single_liquidation)
}
