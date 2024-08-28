use crate::{
    constants::{
        BORROWING_FEE_FLOOR, DECIMAL_PRECISION, FEED_DECIMAL_PRECISION, MAX_CONFIDENCE_RATE,
        MAX_PRICE_DIFFERENCE_BETWEEN_ORACLES, REDEMPTION_FEE_FLOOR, TIMEOUT,
    },
    errors::{BorrowerOpsError, PriceFeedError},
    state::CommunityIssuanceConfig,
};
use anchor_lang::prelude::*;
use spl_stake_pool::state::StakePool;

use chainlink_solana as chainlink;
use pyth_solana_receiver_sdk::price_update::Price;
use std::cmp;

pub fn get_current_timestamp() -> u64 {
    Clock::get().unwrap().unix_timestamp.try_into().unwrap()
}

pub fn get_current_timestamp_with_config(config: &CommunityIssuanceConfig) -> Result<u64> {
    if config._is_dev {
        return Ok(config._timestamp);
    }
    Ok(Clock::get()?.unix_timestamp.try_into().unwrap())
}

pub fn require_valid_borrow_max_fee_percentage(
    max_fee_percentage: u64,
    is_recovery_mode: bool,
) -> Result<()> {
    if is_recovery_mode {
        require!(
            max_fee_percentage <= DECIMAL_PRECISION,
            BorrowerOpsError::InvalidMaxFeeRecoveryMode
        );
    } else {
        require!(
            max_fee_percentage >= BORROWING_FEE_FLOOR && max_fee_percentage <= DECIMAL_PRECISION,
            BorrowerOpsError::InvalidMaxFee
        );
    }
    Ok(())
}

pub fn require_valid_redeem_max_fee_percentage(max_fee_percentage: u64) -> Result<()> {
    require!(
        max_fee_percentage >= REDEMPTION_FEE_FLOOR && max_fee_percentage <= DECIMAL_PRECISION,
        BorrowerOpsError::InvalidRedeemMaxFee
    );
    Ok(())
}

pub fn require_non_zero_redeem_amount(amount: u64) -> Result<()> {
    require!(amount > 0, BorrowerOpsError::ZeroRedeemAmount);
    Ok(())
}

pub fn require_non_zero_debt_change(usv_change: u64) -> Result<()> {
    require!(usv_change > 0, BorrowerOpsError::ZeroDebtChange);
    Ok(())
}

pub fn require_non_zero_adjustment(coll_change: u64, usv_change: u64) -> Result<()> {
    require!(
        coll_change != 0 || usv_change != 0,
        BorrowerOpsError::ZeroAdjustment
    );
    Ok(())
}

pub fn require_no_coll_withdrawal(coll_change: u64, is_coll_increase: bool) -> Result<()> {
    if !is_coll_increase {
        require!(coll_change == 0, BorrowerOpsError::RecoveryNoCollWithdraw);
    }
    Ok(())
}

pub fn require_new_icr_is_above_old_icr(new_icr: u64, old_icr: u64) -> Result<()> {
    require!(new_icr >= old_icr, BorrowerOpsError::RecoveryNoCollWithdraw);
    Ok(())
}

pub fn require_user_accepts_fee(fee: u64, amt: u64, max_fee_percentage: u64) -> Result<()> {
    let fee_percentage = u64::try_from(
        u128::from(fee)
            .checked_mul(DECIMAL_PRECISION.into())
            .unwrap()
            .checked_div(amt.into())
            .unwrap(),
    )
    .unwrap();
    require!(
        fee_percentage <= max_fee_percentage,
        BorrowerOpsError::FeeExceededMax
    );
    Ok(())
}

pub fn require_sufficient_usv_balance(balance: u64, payment: u64) -> Result<()> {
    require!(balance >= payment, BorrowerOpsError::InsufficientUSVBalance);
    Ok(())
}

// Price feed utilities
pub fn get_jitosol_rate(acc_data: &mut &[u8]) -> Result<u64> {
    let pool_state = StakePool::deserialize(acc_data).unwrap();
    require!(
        pool_state.last_update_epoch == Clock::get()?.epoch,
        PriceFeedError::PoolNotUpdated
    );
    let rate = pool_state
        .calc_pool_tokens_for_deposit(100_000_000)
        .unwrap();
    Ok(rate)
}

pub fn get_current_timestamp_i64() -> Result<i64> {
    if cfg!(test) {
        return Ok(1_000_000);
    }
    Ok(Clock::get()?.unix_timestamp.try_into().unwrap())
}

pub fn is_pyth_broken(msg: &Price) -> bool {
    let current_timestamp = get_current_timestamp_i64().unwrap();
    if msg.price <= 0
        || msg.publish_time == 0
        || msg.conf == 0
        || current_timestamp < msg.publish_time
    {
        return true;
    }
    false
}

pub fn is_chainlink_broken(round: &chainlink::Round) -> bool {
    if round.answer == 0 || round.round_id == 0 || round.slot == 0 || round.timestamp == 0 {
        return true;
    }
    false
}

pub fn is_pyth_frozen(msg: &Price) -> bool {
    let current_timestamp = get_current_timestamp_i64().unwrap();
    let elapsed = current_timestamp
        .checked_sub(msg.publish_time)
        .expect("underflow");
    elapsed > TIMEOUT
}

pub fn is_chainlink_frozen(round: &chainlink::Round) -> bool {
    let current_timestamp = get_current_timestamp_i64().unwrap();
    let elapsed = i64::from(current_timestamp)
        .checked_sub(round.timestamp.into())
        .expect("underflow");
    elapsed > TIMEOUT
}

pub fn pyth_price_conf_interval_above_max(msg: &Price) -> bool {
    let conf = msg.conf;
    let price = u64::try_from(msg.price).unwrap();

    let conf_rate = u64::try_from(
        (conf as u128)
            .checked_mul(FEED_DECIMAL_PRECISION.into())
            .unwrap()
            .checked_div(price.into())
            .unwrap(),
    )
    .unwrap();
    conf_rate > MAX_CONFIDENCE_RATE
}

pub fn both_oracles_live_unbroken_similar_price(
    pyth_res: &Price,
    chainlink_res: &chainlink::Round,
    chainlink_price: u64,
) -> bool {
    if is_chainlink_broken(chainlink_res)
        || is_chainlink_frozen(chainlink_res)
        || is_pyth_broken(pyth_res)
        || is_pyth_frozen(pyth_res)
    {
        return false;
    }
    both_oracles_similar_price(pyth_res.price.try_into().unwrap(), chainlink_price)
}

pub fn both_oracles_similar_price(pyth_price: u64, chainlink_price: u64) -> bool {
    let min_price = cmp::min(pyth_price, chainlink_price);
    let max_price = cmp::max(pyth_price, chainlink_price);
    let percent_price_diff = u64::try_from(
        (max_price.checked_sub(min_price).unwrap() as u128)
            .checked_mul(FEED_DECIMAL_PRECISION as u128)
            .unwrap()
            .checked_div(min_price as u128)
            .unwrap(),
    )
    .unwrap();
    percent_price_diff <= MAX_PRICE_DIFFERENCE_BETWEEN_ORACLES
}

#[cfg(test)]
pub mod utils_test {
    use chainlink::Round;

    use super::*;

    fn load_price_message(price: i64, conf: u64, publish_time: i64) -> Price {
        Price {
            price,
            conf,
            exponent: 0,
            publish_time,
        }
    }

    fn load_chainlink_response(round_id: u32, slot: u64, timestamp: u32, answer: i128) -> Round {
        Round {
            round_id,
            slot,
            timestamp,
            answer,
        }
    }

    #[test]
    fn is_pyth_broken_test() {
        let price_zero = load_price_message(0, 1, 1);
        let conf_zero = load_price_message(0, 1, 1);
        let time_zero = load_price_message(0, 1, 1);
        let from_future = load_price_message(1, 1, 1_000_001);
        let non_zero = load_price_message(1, 1, 1);
        assert!(is_pyth_broken(&price_zero));
        assert!(is_pyth_broken(&conf_zero));
        assert!(is_pyth_broken(&time_zero));
        assert!(is_pyth_broken(&from_future));
        assert!(is_pyth_broken(&non_zero) == false);
    }

    #[test]
    fn is_pyth_frozen_test() {
        let outdated_msg = load_price_message(1, 1, 1_000_000 - TIMEOUT - 1);
        let valid_msg = load_price_message(1, 1, 1_000_000);
        assert!(is_pyth_frozen(&outdated_msg));
        assert!(is_pyth_frozen(&valid_msg) == false);
    }

    #[test]
    fn is_chainlink_broken_test() {
        let valid_msg = load_chainlink_response(1, 1, 1_000_000, 1);
        let answer_zero = load_chainlink_response(1, 1, 1_000_000, 0);
        let round_id_zero = load_chainlink_response(0, 1, 1_000_000, 1);
        let timestamp_zero = load_chainlink_response(1, 1, 0, 1);
        let slot_zero = load_chainlink_response(1, 0, 1_000_000, 1);

        assert!(is_chainlink_broken(&answer_zero));
        assert!(is_chainlink_broken(&round_id_zero));
        assert!(is_chainlink_broken(&timestamp_zero));
        assert!(is_chainlink_broken(&slot_zero));
        assert!(is_chainlink_broken(&valid_msg) == false);
    }

    #[test]
    fn is_chainlink_frozen_test() {
        let outdated_msg =
            load_chainlink_response(1, 1, 1_000_000 - u32::try_from(TIMEOUT).unwrap() - 1, 1);
        let valid_msg = load_chainlink_response(1, 1, 1_000_000, 1);
        assert!(is_chainlink_frozen(&outdated_msg));
        assert!(is_chainlink_frozen(&valid_msg) == false);
    }
}
