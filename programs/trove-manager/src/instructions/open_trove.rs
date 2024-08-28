use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
    token_2022::{mint_to, transfer_checked, MintTo, TransferChecked},
};
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;
use solana_program::program::invoke;

use crate::{
    errors::{BorrowerOpsError, PriceFeedError},
    events::{Operation, TroveUpdated, USVBorrowingFeePaid},
    math::{compute_cr, compute_nominal_cr},
    state::{CVGTStakingPoolState, CommunityIssuanceConfig, PoolState, PriceFeedState, Trove},
    utils::{require_user_accepts_fee, require_valid_borrow_max_fee_percentage},
};

#[derive(Accounts)]
pub struct OpenTrove<'info> {
    #[account(mut)]
    pub pool_state: Box<Account<'info, PoolState>>,

    #[account(
        mut,
        seeds = [
            b"trove", 
            pool_state.key().as_ref(),
            creator.key().as_ref(),
        ],
        bump
    )]
    pub trove: Box<Account<'info, Trove>>,

    #[account(mut)]
    pub next_trove: Option<Box<Account<'info, Trove>>>,

    #[account(mut)]
    pub prev_trove: Option<Box<Account<'info, Trove>>>,

    #[account(
        mut,
        associated_token::mint = collateral,
        associated_token::authority = creator,
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
        associated_token::authority = token_authority
    )]
    pub gas_compensation_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = stablecoin,
        associated_token::authority = creator
    )]
    pub stablecoin_receive_account: Box<Account<'info, TokenAccount>>,

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
    pub creator: Signer<'info>,

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

impl<'info> OpenTrove<'info> {
    pub fn transfer_coll_ctx(&self) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
        let cpi_accounts = TransferChecked {
            from: self.user_coll_ata.to_account_info(),
            to: self.collateral_vault.to_account_info(),
            authority: self.creator.to_account_info(),
            mint: self.collateral.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        CpiContext::new(cpi_program, cpi_accounts)
    }

    pub fn mint_stablecoin_to_user_ctx(&self) -> CpiContext<'_, '_, '_, 'info, MintTo<'info>> {
        let cpi_accounts = MintTo {
            mint: self.stablecoin.to_account_info(),
            to: self.stablecoin_receive_account.to_account_info(),
            authority: self.token_authority.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        CpiContext::new(cpi_program, cpi_accounts)
    }

    pub fn mint_stablecoin_to_gas_compensation_ctx(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, MintTo<'info>> {
        let cpi_accounts = MintTo {
            mint: self.stablecoin.to_account_info(),
            to: self.gas_compensation_vault.to_account_info(),
            authority: self.token_authority.to_account_info(),
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
        let creator = &self.creator;
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
            creator.key,
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
                creator.to_account_info(),
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

pub fn open_trove_handler(
    ctx: Context<OpenTrove>,
    max_fee_percentage: u64,
    is_lamport: bool,
    amt: u64,
    usv_amt: u64,
) -> Result<()> {
    let coll_amt = if is_lamport {
        ctx.accounts.stake_sol_for_jitosol(amt)?
    } else {
        amt
    };

    let creator = &ctx.accounts.creator.key();

    let next_trove = &mut ctx.accounts.next_trove;
    let prev_trove = &mut ctx.accounts.prev_trove;
    let community_issuance_config = &ctx.accounts.community_issuance_config;
    let trove_key = ctx.accounts.trove.key();

    // Fetch price
    let price = ctx.accounts.price_feed_state.fetch_price(
        &ctx.accounts.chainlink_program,
        &ctx.accounts.chainlink_feed,
        &ctx.accounts.jitosol_stake_pool,
        &ctx.accounts.pyth_feed_account,
    )?;

    let trove = &mut ctx.accounts.trove;
    let pool_state = &mut ctx.accounts.pool_state;
    let gas_compensation = pool_state.gas_compensation;

    // Check is recovery
    let is_recovery_mode = pool_state.check_recovery_mode(price);

    require_valid_borrow_max_fee_percentage(max_fee_percentage, is_recovery_mode)?;
    // Require trove is not active
    trove.require_trove_not_active()?;

    // Calculate debt
    let mut usv_fee = 0u64;
    let mut net_debt = usv_amt;
    if !is_recovery_mode {
        usv_fee = trigger_borrowing_fee(pool_state, usv_amt, max_fee_percentage)?;
        net_debt = net_debt.checked_add(usv_fee).unwrap();
    }

    // Require min debt
    pool_state.require_at_least_min_net_debt(net_debt)?;

    // ICR is based on the composite debt, i.e. the requested USV amount + USV borrowing fee + USV gas comp.
    let composite_debt = pool_state.get_composit_debt(net_debt);
    require!(composite_debt > 0, BorrowerOpsError::Calculation);

    let icr = compute_cr(coll_amt, composite_debt, price).unwrap();

    let nicr = compute_nominal_cr(coll_amt, composite_debt).unwrap();

    if is_recovery_mode {
        pool_state.require_icr_is_above_ccr(icr)?;
    } else {
        pool_state.require_icr_is_above_mcr(icr)?;
        let new_tcr =
            pool_state.get_new_tcr_from_trove_change(coll_amt, true, composite_debt, true, price); // bools: coll increase, debt increase
        pool_state.require_new_tcr_is_above_ccr(new_tcr)?;
    }

    // Set the trove struct's properties
    trove.init(pool_state.key(), *creator, coll_amt, composite_debt);
    trove.update_reward_snapshot(pool_state);
    let stake = trove.update_stake_and_total_stakes(pool_state);

    trove.insert_sorted(trove_key, nicr, prev_trove, next_trove, pool_state)?;

    // mint the USVAmount to the borrower
    let pool_config_key = pool_state.key();
    let token_auth_bump = pool_state.token_auth_bump;
    let authority_seed = &[
        &b"token-authority"[..],
        pool_config_key.as_ref(),
        token_auth_bump.as_ref(),
    ];

    // calculate pool state
    pool_state.increase_active_coll(coll_amt);
    pool_state.increase_active_debt(net_debt);
    pool_state.increase_active_debt(gas_compensation);

    mint_to(
        ctx.accounts
            .mint_stablecoin_to_staking_pool_ctx()
            .with_signer(&[&authority_seed[..]]),
        usv_fee,
    )?;
    if community_issuance_config.enable_emission {
        ctx.accounts.increase_f_usv(usv_fee)?;
    }

    mint_to(
        ctx.accounts
            .mint_stablecoin_to_user_ctx()
            .with_signer(&[&authority_seed[..]]),
        usv_amt,
    )?;

    // Move coll to vault
    if !is_lamport {
        transfer_checked(
            ctx.accounts.transfer_coll_ctx(),
            coll_amt,
            ctx.accounts.collateral.decimals,
        )?;
    }

    mint_to(
        ctx.accounts
            .mint_stablecoin_to_gas_compensation_ctx()
            .with_signer(&[&authority_seed[..]]),
        gas_compensation,
    )?;

    emit!(TroveUpdated {
        borrower: creator.key(),
        debt: composite_debt,
        coll: coll_amt,
        stake,
        operation: Operation::OpenTrove
    });
    emit!(USVBorrowingFeePaid {
        borrower: creator.key(),
        usv_fee
    });

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
