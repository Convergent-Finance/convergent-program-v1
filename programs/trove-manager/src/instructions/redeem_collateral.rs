use std::cmp;

use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
    token_2022::{burn, transfer_checked, Burn, TransferChecked},
};
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;

use crate::{
    constants::DECIMAL_PRECISION,
    errors::{BorrowerOpsError, PriceFeedError},
    events::{Operation, Redemption, TroveUpdated},
    math::compute_nominal_cr,
    state::{
        CVGTStakingPoolState, CommunityIssuanceConfig, PoolState, PriceFeedState, Trove,
        TroveStatus,
    },
    utils::{
        require_non_zero_redeem_amount, require_sufficient_usv_balance, require_user_accepts_fee,
        require_valid_redeem_max_fee_percentage,
    },
    ID,
};

// Remainning accounts structure:
// - Trove accounts with order collateral ratio asc
// Note: the last trove account will not be redeemed, it need to be updated to become the first trove in trove sorted list
#[derive(Accounts)]
pub struct RedeemCollateral<'info> {
    #[account(mut)]
    pub pool_state: Box<Account<'info, PoolState>>,

    #[account(mut)]
    pub next_trove: Option<Box<Account<'info, Trove>>>,

    #[account(mut)]
    pub prev_trove: Option<Box<Account<'info, Trove>>>,

    #[account(
        mut,
        associated_token::mint = stablecoin,
        associated_token::authority = redeemer
    )]
    pub stablecoin_receive_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = collateral,
        associated_token::authority = cvgt_staking_state
    )]
    pub coll_cvgt_staking_vault: Box<Account<'info, TokenAccount>>,

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
        init_if_needed,
        payer = redeemer,
        associated_token::mint = collateral,
        associated_token::authority = redeemer,
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
    pub redeemer: Signer<'info>,

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

impl<'info> RedeemCollateral<'info> {
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

    pub fn transfer_coll_to_staking_pool_ctx(
        &self,
    ) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
        let cpi_accounts = TransferChecked {
            from: self.collateral_vault.to_account_info(),
            to: self.coll_cvgt_staking_vault.to_account_info(),
            authority: self.token_authority.to_account_info(),
            mint: self.collateral.to_account_info(),
        };
        let cpi_program = self.token_program.to_account_info();
        CpiContext::new(cpi_program, cpi_accounts)
    }

    pub fn burn_stablecoin_from_user_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Burn<'info>> {
        let cpi_accounts = Burn {
            mint: self.stablecoin.to_account_info(),
            from: self.stablecoin_receive_account.to_account_info(),
            authority: self.redeemer.to_account_info(),
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

    pub fn increase_f_coll(&self, coll_fee: u64) -> Result<()> {
        let cvgt_staking_account = &self.cvgt_staking_state;
        let mut data = cvgt_staking_account.try_borrow_mut_data()?;
        let mut cvgt_staking = CVGTStakingPoolState::try_deserialize(&mut data.as_ref())
            .expect("Error Deserializing Data");
        cvgt_staking.increase_f_coll(coll_fee);
        cvgt_staking.try_serialize(&mut data.as_mut())?;
        Ok(())
    }
}

pub fn redeem_collateral_handler(
    ctx: Context<RedeemCollateral>,
    max_fee_percentage: u64,
    usv_amt: u64,
) -> Result<()> {
    let mut totals: RedemptionTotals = Default::default();
    let pool_state = &mut ctx.accounts.pool_state;

    require_valid_redeem_max_fee_percentage(max_fee_percentage)?;
    // TODO: Do we need require boostrap period

    totals.price = ctx.accounts.price_feed_state.fetch_price(
        &ctx.accounts.chainlink_program,
        &ctx.accounts.chainlink_feed,
        &ctx.accounts.jitosol_stake_pool,
        &ctx.accounts.pyth_feed_account,
    )?;
    pool_state.require_tcr_over_mcr(totals.price)?;
    require_non_zero_redeem_amount(usv_amt)?;
    require_sufficient_usv_balance(ctx.accounts.stablecoin_receive_account.amount, usv_amt)?;

    totals.total_usv_supply_at_start = pool_state.get_entire_debt();
    assert!(ctx.accounts.stablecoin_receive_account.amount <= totals.total_usv_supply_at_start);

    totals.remaining_usv = usv_amt;

    let start_index = skim_provided_troves(ctx.remaining_accounts, pool_state, totals.price)?;
    let end_index = ctx.remaining_accounts.len();

    for i in start_index..(end_index - 1) {
        let account = &ctx.remaining_accounts[i];
        if totals.remaining_usv == 0 {
            break;
        }
        require!(
            account.to_account_info().owner == &ID,
            BorrowerOpsError::InvalidAccount
        );

        let mut data = account.try_borrow_mut_data()?;
        let mut trove =
            Trove::try_deserialize(&mut data.as_ref()).expect("Error Deserializing Data");

        pool_state.apply_pending_reward(&mut trove)?;

        let single_redemption = redeem_collateral_from_trove(
            &mut trove,
            totals.remaining_usv,
            totals.price,
            pool_state,
        )?;

        // Partial redemption was cancelled (out-of-date hint, or new net debt < minimum), therefore we could not redeem from the last Trove
        if single_redemption.canceled_partial {
            break;
        }

        if trove.status == TroveStatus::ClosedByRedemption {
            trove.remove_sorted_redemption(ctx.remaining_accounts, account.key(), pool_state)?;
            trove.try_serialize(&mut data.as_mut())?;
            drop(data);
            if ctx.accounts.next_trove.is_some() {
                ctx.accounts.next_trove.as_mut().unwrap().reload()?;
            }
            if ctx.accounts.prev_trove.is_some() {
                ctx.accounts.prev_trove.as_mut().unwrap().reload()?;
            }
        } else {
            let new_nicr = compute_nominal_cr(trove.coll, trove.debt).unwrap();
            trove.re_insert_redemption(
                ctx.remaining_accounts,
                account.key(),
                new_nicr,
                &mut ctx.accounts.prev_trove,
                &mut ctx.accounts.next_trove,
                pool_state,
            )?;
            // store trove data
            trove.try_serialize(&mut data.as_mut())?;
        }

        totals.total_usv_gas_to_burn = totals
            .total_usv_gas_to_burn
            .checked_add(single_redemption.usv_gas_to_burn)
            .unwrap();

        totals.total_usv_to_redeem = totals
            .total_usv_to_redeem
            .checked_add(single_redemption.usv_lot)
            .unwrap();

        totals.total_coll_drawn = totals
            .total_coll_drawn
            .checked_add(single_redemption.coll_lot)
            .unwrap();

        totals.remaining_usv = totals
            .remaining_usv
            .checked_sub(single_redemption.usv_lot)
            .unwrap();
    }
    require!(totals.total_coll_drawn > 0, BorrowerOpsError::ZeroCollDrawn);

    pool_state.update_base_fee_rate_from_redemption(
        totals.total_coll_drawn,
        totals.price,
        totals.total_usv_supply_at_start,
    )?;

    totals.coll_fee = pool_state.get_redemption_fee(totals.total_coll_drawn)?;

    require_user_accepts_fee(totals.coll_fee, totals.total_coll_drawn, max_fee_percentage)?;

    pool_state.decrease_active_coll(totals.coll_fee);

    totals.coll_to_send_to_redeemer = totals
        .total_coll_drawn
        .checked_sub(totals.coll_fee)
        .unwrap();

    emit!(Redemption {
        attempted_usv_amount: usv_amt,
        actual_usv_amount: totals.total_usv_to_redeem,
        coll_sent: totals.total_coll_drawn,
        coll_fee: totals.coll_fee,
    });

    pool_state.decrease_active_debt(totals.total_usv_to_redeem);
    pool_state.decrease_active_coll(totals.coll_to_send_to_redeemer);

    move_token_from_redeem(ctx, &totals)
}

fn redeem_collateral_from_trove(
    trove: &mut Trove,
    max_usv_amt: u64,
    price: u64,
    pool_state: &mut PoolState,
) -> Result<SingleRedemptionValues> {
    let mut single_redemption: SingleRedemptionValues = Default::default();

    // Determine the remaining amount (lot) to be redeemed, capped by the entire debt of the Trove minus the liquidation reserve
    single_redemption.usv_lot = cmp::min(
        max_usv_amt,
        trove.debt.checked_sub(pool_state.gas_compensation).unwrap(),
    );

    // Get the Coll Lot of equivalent value in USD
    single_redemption.coll_lot = u64::try_from(
        u128::from(single_redemption.usv_lot)
            .checked_mul(DECIMAL_PRECISION.into())
            .unwrap()
            .checked_div(price.into())
            .unwrap(),
    )
    .unwrap();

    // Decrease the debt and collateral of the current Trove according to the USV lot and corresponding Coll to send
    let new_debt = trove.debt.checked_sub(single_redemption.usv_lot).unwrap();
    let new_coll = trove.coll.checked_sub(single_redemption.coll_lot).unwrap();

    if new_debt == pool_state.gas_compensation {
        // No debt left in the Trove (except for the liquidation reserve), therefore the trove gets closed
        trove.remove_stake(pool_state);
        trove.close_trove(pool_state, TroveStatus::ClosedByRedemption)?;
        redeem_close_trove(trove, pool_state, pool_state.gas_compensation, new_coll);
        single_redemption.usv_gas_to_burn = pool_state.gas_compensation;
        emit!(TroveUpdated {
            borrower: trove.creator,
            debt: 0,
            coll: 0,
            stake: 0,
            operation: Operation::RedeemCollateral
        });
    } else {
        if pool_state.get_net_debt(new_debt) < pool_state.min_net_debt {
            single_redemption.canceled_partial = true;
            return Ok(single_redemption);
        }

        trove.debt = new_debt;
        trove.coll = new_coll;
        trove.update_stake_and_total_stakes(pool_state);

        emit!(TroveUpdated {
            borrower: trove.creator,
            debt: new_debt,
            coll: new_coll,
            stake: trove.stake,
            operation: Operation::RedeemCollateral
        });
    }

    Ok(single_redemption)
}

fn redeem_close_trove(trove: &mut Trove, pool_state: &mut PoolState, usv_amt: u64, coll_amt: u64) {
    pool_state.decrease_active_debt(usv_amt);
    trove.account_surplus(coll_amt);

    pool_state.decrease_active_coll(coll_amt);
    pool_state.increase_total_surplus(coll_amt);
}

fn move_token_from_redeem(
    ctx: Context<RedeemCollateral>,
    redemption_totals: &RedemptionTotals,
) -> Result<()> {
    let pool_state = &ctx.accounts.pool_state;
    let community_issuance_config = &ctx.accounts.community_issuance_config;
    let pool_state_key = pool_state.key();
    let authority_seed = &pool_state.token_auth_seeds(&pool_state_key);
    if redemption_totals.total_usv_gas_to_burn > 0 {
        burn(
            ctx.accounts
                .burn_stablecoin_from_gas_compensation_ctx()
                .with_signer(&[&authority_seed[..]]),
            redemption_totals.total_usv_gas_to_burn,
        )?;
    }
    burn(
        ctx.accounts.burn_stablecoin_from_user_ctx(),
        redemption_totals.total_usv_to_redeem,
    )?;
    transfer_checked(
        ctx.accounts
            .transfer_coll_out_ctx()
            .with_signer(&[&authority_seed[..]]),
        redemption_totals.coll_to_send_to_redeemer,
        ctx.accounts.collateral.decimals,
    )?;
    transfer_checked(
        ctx.accounts
            .transfer_coll_to_staking_pool_ctx()
            .with_signer(&[&authority_seed[..]]),
        redemption_totals.coll_fee,
        ctx.accounts.collateral.decimals,
    )?;
    if community_issuance_config.enable_emission {
        ctx.accounts.increase_f_coll(redemption_totals.coll_fee)?;
    }
    Ok(())
}

fn skim_provided_troves(
    accounts: &[AccountInfo<'_>],
    pool_state: &PoolState,
    price: u64,
) -> Result<usize> {
    // Trove account begin from the sixth remaining_accounts
    let start_index = 0;
    let end_index = accounts.len();
    let mut found = false;
    let mut found_index = start_index;

    for i in start_index..end_index {
        let account = &accounts[i];
        let key = account.key();
        // 1st check: The account is belong to our program
        // If the 1st trove is Pubkey::default then we continue the loop
        let trove = if key != Pubkey::default() {
            require!(
                account.to_account_info().owner == &ID,
                BorrowerOpsError::InvalidAccount
            );
            let data = account.try_borrow_mut_data()?;
            Trove::try_deserialize(&mut data.as_ref()).expect("Error Deserializing Data")
        } else {
            continue;
        };
        require!(
            trove.pool_state == pool_state.key(),
            BorrowerOpsError::InvalidAccount
        );

        // Trove need to be active
        trove.require_trove_active()?;

        // 2nd check: The start of provided accounts need to have ICR < MCR
        // If the program reach to this point, we are sure that first account isnt Pubkey::default
        if i == start_index {
            require!(
                trove.get_current_icr(pool_state, price) < pool_state.mcr,
                BorrowerOpsError::InvalidTroveNeighbor
            );
        }

        if i > start_index {
            // 3rd check: The current trove need to be linked with the previous trove
            let next_account = &accounts[i - 1];
            require!(
                trove.next == next_account.key(),
                BorrowerOpsError::InvalidTroveNeighbor
            );
            // Find the index of the first trove that have ICR >= MCR
            if trove.get_current_icr(pool_state, price) >= pool_state.mcr && !found {
                found = true;
                found_index = i;
            }
        }
    }
    require!(found, BorrowerOpsError::InvalidTroveNeighbor);
    Ok(found_index)
}

#[derive(Default)]
pub struct RedemptionTotals {
    pub remaining_usv: u64,
    pub total_usv_to_redeem: u64,
    pub total_coll_drawn: u64,
    pub coll_fee: u64,
    pub coll_to_send_to_redeemer: u64,
    pub decayed_base_rate: u64,
    pub price: u64,
    pub total_usv_supply_at_start: u64,
    pub total_usv_gas_to_burn: u64,
}

#[derive(Default)]
pub struct SingleRedemptionValues {
    pub usv_lot: u64,
    pub coll_lot: u64,
    pub canceled_partial: bool,
    pub usv_gas_to_burn: u64,
}
