use anchor_lang::prelude::*;
use chainlink_solana as chainlink;
use chainlink_solana::Round;
use pyth_solana_receiver_sdk::price_update::{Price, PriceUpdateV2};

use crate::{
    constants::{FEED_DECIMAL_PRECISION, TARGET_DECIMAL_PRECISION},
    utils::{
        both_oracles_live_unbroken_similar_price, both_oracles_similar_price, get_jitosol_rate,
        is_chainlink_broken, is_chainlink_frozen, is_pyth_broken, is_pyth_frozen,
        pyth_price_conf_interval_above_max,
    },
};

#[account]
#[derive(InitSpace)]
pub struct PriceFeedState {
    pub creator: Pubkey,
    pub chainlink_feed: Pubkey,
    pub jitosol_stake_pool: Pubkey,
    pub pyth_feed_account: Pubkey,
    pub last_good_price: u64,
    pub status: Status,
    pub bump: u8,
    pub _is_dev: bool,
    pub _dev_price: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Copy, Clone, PartialEq, Eq, InitSpace, Debug)]
pub enum Status {
    PythWorking,
    UsingChainlinkPythUntrusted,
    BothOraclesUntrusted,
    UsingChainlinkPythFrozen,
    UsingPythChainlinkUntrusted,
}

impl PriceFeedState {
    pub fn fetch_price<'info>(
        &mut self,
        chainlink_program: &AccountInfo<'info>,
        chainlink_feed: &AccountInfo<'info>,
        jitosol_stake_pool: &AccountInfo<'info>,
        pyth_feed_account: &PriceUpdateV2,
    ) -> Result<u64> {
        if self._is_dev {
            return Ok(self._dev_price);
        }

        let chainlink_response = chainlink::latest_round_data(
            chainlink_program.to_account_info(),
            chainlink_feed.to_account_info(),
        )?;

        let jitosol_staking_data = &mut &jitosol_stake_pool.try_borrow_data()?[..][..];
        let rate = get_jitosol_rate(jitosol_staking_data)?;

        let jitosol_price_chainlink =
            u64::try_from(chainlink_response.answer).unwrap() * FEED_DECIMAL_PRECISION / rate;

        // Get price from pyth
        let pyth_price_message = pyth_feed_account
            .get_price_unchecked(&pyth_feed_account.price_message.feed_id)
            .unwrap();

        self.update(
            &pyth_price_message,
            &chainlink_response,
            jitosol_price_chainlink,
        )
    }

    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }

    pub fn update_price(&mut self, new_price: u64) -> u64 {
        self.last_good_price = u64::try_from(
            (new_price as u128)
                .checked_mul(TARGET_DECIMAL_PRECISION as u128)
                .unwrap()
                .checked_div(FEED_DECIMAL_PRECISION as u128)
                .unwrap(),
        )
        .unwrap();
        self.last_good_price
    }

    pub fn update(
        &mut self,
        pyth_price_message: &Price,
        chainlink_response: &Round,
        price_chainlink: u64,
    ) -> Result<u64> {
        let price_pyth = u64::try_from(if pyth_price_message.price > 0 {
            pyth_price_message.price
        } else {
            0
        })
        .unwrap();
        match self.status {
            // --- CASE 1: System fetched last price from Pyth  ---
            Status::PythWorking => {
                // If Pyth is broken, try Chainlink
                if is_pyth_broken(pyth_price_message) {
                    // If Chainlink is broken then both oracles are untrusted, so return the last good price
                    if is_chainlink_broken(&chainlink_response) {
                        self.set_status(Status::BothOraclesUntrusted);
                        return Ok(self.last_good_price);
                    }
                    // If Chainlink is only frozen but otherwise returning valid data, return the last good price.
                    if is_chainlink_frozen(&chainlink_response) {
                        self.set_status(Status::UsingChainlinkPythUntrusted);
                        return Ok(self.last_good_price);
                    }
                    // If Pyth is broken and Chainlink is working, switch to Chainlink and return current Chainlink price
                    self.set_status(Status::UsingChainlinkPythUntrusted);

                    return Ok(self.update_price(price_chainlink));
                }

                // If Pyth us frozen, try Chainlink
                if is_pyth_frozen(pyth_price_message) {
                    // If Chainlink is broken too, remember Chainlink broke, and return last good price
                    if is_chainlink_broken(&chainlink_response) {
                        self.set_status(Status::UsingPythChainlinkUntrusted);
                        return Ok(self.last_good_price);
                    }

                    // If Chainlink is frozen or working, remember Pyth froze, and switch to Chainlink
                    self.set_status(Status::UsingChainlinkPythFrozen);

                    if is_chainlink_frozen(&chainlink_response) {
                        return Ok(self.last_good_price);
                    }

                    // If Chainlink is working, use it
                    return Ok(self.update_price(price_chainlink));
                }

                // If Pyth price has changed by > 50% between two consecutive rounds, compare it to Chainlink's price
                if pyth_price_conf_interval_above_max(pyth_price_message) {
                    // If Chainlink is broken, both oracles are untrusted, and return last good price
                    if is_chainlink_broken(&chainlink_response) {
                        self.set_status(Status::BothOraclesUntrusted);
                        return Ok(self.last_good_price);
                    }

                    // If Chainlink is frozen, switch to Chainlink and return last good price

                    if is_chainlink_frozen(&chainlink_response) {
                        self.set_status(Status::UsingChainlinkPythUntrusted);
                        return Ok(self.last_good_price);
                    }

                    /*
                     * If Chainlink is live and both oracles have a similar price, conclude that Pyth's large price deviation between
                     * two consecutive rounds was likely a legitmate market price movement, and so continue using Pyth
                     */
                    if both_oracles_similar_price(price_pyth, price_chainlink) {
                        return Ok(self.update_price(price_pyth));
                    }

                    // If Chainlink is live but the oracles differ too much in price, conclude that Pyth's initial price deviation was
                    // an oracle failure. Switch to Chainlink, and use Chainlink price
                    self.set_status(Status::UsingChainlinkPythUntrusted);
                    return Ok(self.update_price(price_chainlink));
                }

                // If Pyth is working and Chainlink is broken, remember Chainlink is broken
                if is_chainlink_broken(&chainlink_response) {
                    self.set_status(Status::UsingPythChainlinkUntrusted);
                }

                // If Pyth is working, return Pyth current price (no status change)
                Ok(self.update_price(price_pyth))
            }
            // --- CASE 2: The system fetched last price from Chainlink ---
            Status::UsingChainlinkPythUntrusted => {
                // If both Chainlink and Pyth are live, unbroken, and reporting similar prices, switch back to Pyth
                if both_oracles_live_unbroken_similar_price(
                    pyth_price_message,
                    &chainlink_response,
                    price_chainlink,
                ) {
                    self.set_status(Status::PythWorking);
                    return Ok(self.update_price(price_pyth));
                }

                if is_chainlink_broken(&chainlink_response) {
                    self.set_status(Status::BothOraclesUntrusted);
                    return Ok(self.last_good_price);
                }

                // If Chainlink is only frozen but otherwise returning valid data, just return the last good price.
                if is_chainlink_frozen(&chainlink_response) {
                    return Ok(self.last_good_price);
                }

                // Otherwise, use Chainlink price
                return Ok(self.update_price(price_chainlink));
            }
            // --- CASE 3: Both oracles were untrusted at the last price fetch ---
            Status::BothOraclesUntrusted => {
                /*
                 * If both oracles are now live, unbroken and similar price, we assume that they are reporting
                 * accurately, and so we switch back to Pyth.
                 */
                if both_oracles_live_unbroken_similar_price(
                    pyth_price_message,
                    &chainlink_response,
                    price_chainlink,
                ) {
                    self.set_status(Status::PythWorking);
                    return Ok(self.update_price(price_pyth));
                }

                // Otherwise, return the last good price - both oracles are still untrusted (no status change)
                Ok(self.last_good_price)
            }
            // --- CASE 4: Using Chainlink, and Pyth is frozen ---
            Status::UsingChainlinkPythFrozen => {
                if is_pyth_broken(pyth_price_message) {
                    // If both Oracles are broken, return last good price
                    if is_chainlink_broken(&chainlink_response) {
                        self.set_status(Status::BothOraclesUntrusted);
                        return Ok(self.last_good_price);
                    }

                    // If Pyth is broken, remember it and switch to using Chainlink
                    self.set_status(Status::UsingChainlinkPythUntrusted);

                    if is_chainlink_frozen(&chainlink_response) {
                        return Ok(self.last_good_price);
                    }

                    // If Chainlink is working, return Chainlink current price
                    return Ok(self.update_price(price_chainlink));
                }

                if is_pyth_frozen(pyth_price_message) {
                    // if Pyth is frozen and Chainlink is broken, remember Chainlink broke, and return last good price
                    if is_chainlink_broken(&chainlink_response) {
                        self.set_status(Status::UsingPythChainlinkUntrusted);
                        return Ok(self.last_good_price);
                    }

                    // If both are frozen, just use lastGoodPrice
                    if is_chainlink_frozen(&chainlink_response) {
                        return Ok(self.last_good_price);
                    }

                    // if Pyth is frozen and Chainlink is working, keep using Chainlink (no status change)
                    return Ok(self.update_price(price_chainlink));
                }

                // if Pyth is live and Chainlink is broken, remember Chainlink broke, and return Pyth price
                if is_chainlink_broken(&chainlink_response) {
                    self.set_status(Status::UsingPythChainlinkUntrusted);
                    return Ok(self.update_price(price_pyth));
                }

                // If Pyth is live and Chainlink is frozen, just use last good price (no status change) since we have no basis for comparison
                if is_chainlink_frozen(&chainlink_response) {
                    return Ok(self.last_good_price);
                }

                // If Pyth is live and Chainlink is working, compare prices. Switch to Pyth
                // if prices are within 5%, and return Pyth price.
                if both_oracles_similar_price(price_pyth, price_chainlink) {
                    self.set_status(Status::PythWorking);
                    return Ok(self.update_price(price_pyth));
                }

                // Otherwise if Pyth is live but price not within 5% of Chainlink, distrust Pyth, and return Chainlink price
                self.set_status(Status::UsingChainlinkPythUntrusted);
                return Ok(self.update_price(price_chainlink));
            }
            // --- CASE 5: Using Pyth, Chainlink is untrusted ---
            Status::UsingPythChainlinkUntrusted => {
                // If Pyth breaks, now both oracles are untrusted
                if is_pyth_broken(pyth_price_message) {
                    self.set_status(Status::BothOraclesUntrusted);
                    return Ok(self.last_good_price);
                }

                // If Pyth is frozen, return last good price (no status change)
                if is_pyth_frozen(pyth_price_message) {
                    return Ok(self.last_good_price);
                }

                // If Pyth and Chainlink are both live, unbroken and similar price, switch back to PythWorking and return Pyth price
                if both_oracles_live_unbroken_similar_price(
                    pyth_price_message,
                    &chainlink_response,
                    price_chainlink,
                ) {
                    self.set_status(Status::PythWorking);
                    return Ok(self.update_price(price_pyth));
                }

                // If Pyth is live but deviated >50% from it's previous price and Chainlink is still untrusted, switch
                // to BothOraclesUntrusted and return last good price
                if pyth_price_conf_interval_above_max(pyth_price_message) {
                    self.set_status(Status::BothOraclesUntrusted);
                    return Ok(self.last_good_price);
                }

                // Otherwise if Pyth is live and deviated <50% from it's previous price and Chainlink is still untrusted,
                // return Pyth price (no status change)
                Ok(self.update_price(price_pyth))
            }
        }
    }
}

#[cfg(test)]
pub mod price_feed_info_test {
    use super::*;

    fn load_price_feed_info(last_good_price: u64, status: Status) -> PriceFeedState {
        PriceFeedState {
            creator: [1u8; 32].into(),
            chainlink_feed: [1u8; 32].into(),
            jitosol_stake_pool: [1u8; 32].into(),
            pyth_feed_account: [1u8; 32].into(),
            last_good_price,
            _is_dev: false,
            status,
            bump: 1,
            _dev_price: 0,
        }
    }

    fn load_price_message(price: i64, conf: u64, publish_time: i64) -> Price {
        Price {
            price,
            conf,
            exponent: 0,
            publish_time,
        }
    }

    fn load_chainlink_response(timestamp: u32, answer: i128) -> Round {
        Round {
            round_id: 1,
            slot: 1,
            timestamp,
            answer,
        }
    }

    fn dec(value: u64, decimals: u32) -> u64 {
        value * 10u64.pow(decimals)
    }

    #[test]
    /// C1 Pyth working: fetchPrice should return the correct price
    fn c1_pyth_working() {
        // Load default price feed
        let mut price_feed_info = load_price_feed_info(0, Status::PythWorking);
        let chainlink_response = &load_chainlink_response(1_000_000, dec(10, 8).into());
        let price_chainlink = dec(10, 8);

        // Pyth response price is 10
        let pyth_price_message = &load_price_message(dec(10, 8).try_into().unwrap(), 10, 1_000_000);
        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.last_good_price == 10_000_000_000);

        // Pyth response price is 1e9
        let pyth_price_message = &load_price_message(dec(1, 17).try_into().unwrap(), 10, 1_000_000);
        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.last_good_price == dec(1, 18));

        // Pyth response price is 0.0001
        let pyth_price_message = &load_price_message(dec(1, 4).try_into().unwrap(), 10, 1_000_000);
        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.last_good_price == dec(1, 5));

        // Pyth response price is 1234.56789
        let pyth_price_message = &load_price_message(123_456_789_000, 10, 1_000_000);
        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.last_good_price == 1_234_567_890_000);
    }

    #[test]
    /// C1 Pyth breaks, Chainlink working: fetchPrice should return the correct Chainlink price
    fn c1_pyth_breaks_chainlink_working() {
        let mut price_feed_info = load_price_feed_info(0, Status::PythWorking);

        // Load price pyth to break
        let pyth_price_message = &load_price_message(-5000, 1, 1_000_000);
        let price_chainlink = dec(123, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();

        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.last_good_price == 123_000_000_000);

        // Chainlink price is 1e9
        let price_chainlink = dec(1, 17);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());
        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.last_good_price == dec(1, 18));

        // Chainlink price is 0.0001
        let price_chainlink = dec(1, 4);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());
        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.last_good_price == dec(1, 5));

        // Chainlink price is 1234.56789
        let price_chainlink = 123_456_789_000;
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());
        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.last_good_price == 1_234_567_890_000);
    }

    #[test]
    /// C1 Pyth working: Pyth broken by zero conf, Chainlink working: switch to usingChainlinkPythUntrusted, use chainlink price
    fn c1_pyth_broken_zero_conf_switch_to_chainlink() {
        let mut price_feed_info = load_price_feed_info(dec(999, 9), Status::PythWorking);
        assert!(price_feed_info.status == Status::PythWorking);

        let price_chainlink = dec(123, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());
        let pyth_price_message = &load_price_message(dec(999, 8).try_into().unwrap(), 0, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();

        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.last_good_price == dec(123, 9));
    }

    #[test]
    /// C1 Pyth working: Pyth broken by zero timestamp, Chainlink working: switch to usingChainlinkPythUntrusted, use chainlink price
    fn c1_pyth_broken_zero_timestamp_switch_to_chainlink() {
        let mut price_feed_info = load_price_feed_info(dec(999, 9), Status::PythWorking);
        assert!(price_feed_info.status == Status::PythWorking);

        let price_chainlink = dec(123, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());
        let pyth_price_message = &load_price_message(dec(999, 8).try_into().unwrap(), 1, 0);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();

        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.last_good_price == dec(123, 9));
    }

    #[test]
    /// C1 Pyth working: Pyth broken by future timestamp, Chainlink working: switch to usingChainlinkPythUntrusted, use chainlink price
    fn c1_pyth_broken_future_timestamp_switch_to_chainlink() {
        let mut price_feed_info = load_price_feed_info(dec(999, 9), Status::PythWorking);
        assert!(price_feed_info.status == Status::PythWorking);

        let price_chainlink = dec(123, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());
        let pyth_price_message = &load_price_message(dec(999, 8).try_into().unwrap(), 1, 1_000_001);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();

        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.last_good_price == dec(123, 9));
    }

    #[test]
    /// C1 Pyth working: Pyth broken by negative price, Chainlink working: switch to usingChainlinkPythUntrusted, use chainlink price
    fn c1_pyth_broken_negative_price_switch_to_chainlink() {
        let mut price_feed_info = load_price_feed_info(dec(999, 9), Status::PythWorking);
        assert!(price_feed_info.status == Status::PythWorking);

        let price_chainlink = dec(123, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());
        let pyth_price_message = &load_price_message(-99_900_000_000, 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();

        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.last_good_price == dec(123, 9));
    }

    /// Pyth timeout

    #[test]
    /// C1 Pyth working: pyth frozen, chainlink working: switch to usingChainlinkPythFrozen, use chainlink price
    fn c1_pyth_frozen_switch_to_chainlink() {
        let mut price_feed_info = load_price_feed_info(dec(999, 9), Status::PythWorking);
        assert!(price_feed_info.status == Status::PythWorking);

        let price_chainlink = dec(123, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());
        let pyth_price_message =
            &load_price_message(dec(999, 8).try_into().unwrap(), 1, 1_000_000 - 14401);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();

        assert!(price_feed_info.status == Status::UsingChainlinkPythFrozen);
        assert!(price_feed_info.last_good_price == dec(123, 9));
    }

    #[test]
    /// C1 Pyth working: pyth frozen, chainlink broken: switch to usingPythChainlinkUntrusted, use pyth price
    fn c1_pyth_frozen_chainlink_broken_return_last_good_price() {
        let mut price_feed_info = load_price_feed_info(dec(999, 9), Status::PythWorking);
        assert!(price_feed_info.status == Status::PythWorking);

        let price_chainlink = dec(0, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());
        let pyth_price_message =
            &load_price_message(dec(123, 8).try_into().unwrap(), 1, 1_000_000 - 14401);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();

        assert!(price_feed_info.status == Status::UsingPythChainlinkUntrusted);
        // Expect lastGoodPrice has not updated
        assert!(price_feed_info.last_good_price == dec(999, 9));
    }

    #[test]
    /// C1 Pyth working: pyth is out of date by <4hrs: remain PythWorking
    fn c1_pyth_working_out_of_date_lesser_than_4_hrs() {
        let mut price_feed_info = load_price_feed_info(dec(999, 9), Status::PythWorking);
        assert!(price_feed_info.status == Status::PythWorking);

        let price_chainlink = dec(123, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message =
            &load_price_message(dec(321, 8).try_into().unwrap(), 1, 1_000_000 - 14399);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::PythWorking);
        // Expect lastGoodPrice has not updated
        assert!(price_feed_info.last_good_price == dec(321, 9));
    }

    #[test]
    /// C1 Pyth working: Pyth conf > 5%, switch to usingChainlinkPythUntrusted, use chainlink price
    fn c1_pyth_working_conf_bigger_than_5_percent() {
        let mut price_feed_info = load_price_feed_info(dec(999, 9), Status::PythWorking);
        assert!(price_feed_info.status == Status::PythWorking);

        let price_chainlink = dec(123, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message =
            &load_price_message(dec(321, 8).try_into().unwrap(), 1_606_000_000, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.last_good_price == dec(123, 9));
    }

    #[test]
    /// C1 Pyth working: Pyth conf > 5% and chainlink price similar
    fn c1_pyth_working_conf_bigger_than_5_percent_chainlink_price_similar() {
        let mut price_feed_info = load_price_feed_info(dec(999, 9), Status::PythWorking);
        assert!(price_feed_info.status == Status::PythWorking);

        let price_chainlink = dec(321, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message =
            &load_price_message(dec(321, 8).try_into().unwrap(), 1_606_000_000, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::PythWorking);
        assert!(price_feed_info.last_good_price == dec(321, 9));
    }

    #[test]
    /// C1 Pyth working: Pyth conf > 5% and chainlink frozen
    fn c1_pyth_working_conf_bigger_than_5_percent_chainlink_frozen() {
        let mut price_feed_info = load_price_feed_info(dec(999, 9), Status::PythWorking);
        assert!(price_feed_info.status == Status::PythWorking);

        let price_chainlink = dec(321, 8);
        let chainlink_response =
            &load_chainlink_response(1_000_000 - 14400 - 1, price_chainlink.into());

        let pyth_price_message =
            &load_price_message(dec(321, 8).try_into().unwrap(), 1_606_000_000, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.last_good_price == dec(999, 9));
    }

    #[test]
    /// C1 Pyth Working: Pyth conf > 5% and Chainlink is broken by 0 price: switch to bothOracleSuspect
    fn c1_pyth_conf_bigger_than_5_percent_chainlink_broken() {
        let mut price_feed_info = load_price_feed_info(dec(999, 9), Status::PythWorking);
        assert!(price_feed_info.status == Status::PythWorking);

        let price_chainlink = dec(0, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message =
            &load_price_message(dec(321, 8).try_into().unwrap(), 1_606_000_000, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::BothOraclesUntrusted);
        assert!(price_feed_info.last_good_price == dec(999, 9));
    }

    #[test]
    /// C1 Pyth Working: Chainlink working
    fn c1_pyth_working_chainlink_working() {
        let mut price_feed_info = load_price_feed_info(dec(101, 9), Status::PythWorking);
        assert!(price_feed_info.status == Status::PythWorking);

        let price_chainlink = dec(103, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message = &load_price_message(dec(102, 8).try_into().unwrap(), 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::PythWorking);
        assert!(price_feed_info.last_good_price == dec(102, 9));
    }

    #[test]
    /// C1 Pyth Working: Pyth working, chainlink break
    fn c1_pyth_working_chainlink_break() {
        let mut price_feed_info = load_price_feed_info(dec(101, 9), Status::PythWorking);
        assert!(price_feed_info.status == Status::PythWorking);

        let price_chainlink = dec(0, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message = &load_price_message(dec(102, 8).try_into().unwrap(), 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingPythChainlinkUntrusted);
        assert!(price_feed_info.last_good_price == dec(102, 9));
    }

    /// --- Case 2: Using Chainlink ---

    #[test]
    /// C2 UsingChainlinkPythUntrusted: Chainlink broke
    fn c2_using_chainlink_pyth_untrusted_chainlink_break() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);

        let price_chainlink = dec(0, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message = &load_price_message(dec(102, 8).try_into().unwrap(), 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::BothOraclesUntrusted);
        assert!(price_feed_info.last_good_price == dec(101, 9));
    }

    #[test]
    /// C2 UsingChainlinkPythUntrusted: chainlink frozen
    fn c2_using_chainlink_pyth_untrusted_chainlink_frozen() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);

        let price_chainlink = dec(103, 8);
        let chainlink_response =
            &load_chainlink_response(1_000_000 - 14400 - 1, price_chainlink.into());

        let pyth_price_message = &load_price_message(dec(102, 8).try_into().unwrap(), 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.last_good_price == dec(101, 9));
    }

    #[test]
    /// C2 UsingChainlinkPythUntrusted: both oracle live
    fn c2_using_chainlink_pyth_untrusted_both_go_live() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message = &load_price_message(dec(105, 8).try_into().unwrap(), 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::PythWorking);
        assert!(price_feed_info.last_good_price == dec(105, 9));
    }

    #[test]
    /// C2 UsingChainlinkPythUntrusted: both oracle live, > 5% price difference
    fn c2_using_chainlink_pyth_untrusted_both_go_live_price_difference() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message =
            &load_price_message((dec(105, 8) + 100).try_into().unwrap(), 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.last_good_price == dec(100, 9));
    }

    /// --- Case 3: Both Oracles suspect ---

    #[test]
    /// C3 BothOraclesUntrusted: both are live and > 5% price difference
    fn c3_both_oracles_suspect_live_and_price_difference() {
        let mut price_feed_info = load_price_feed_info(dec(101, 9), Status::BothOraclesUntrusted);
        assert!(price_feed_info.status == Status::BothOraclesUntrusted);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message =
            &load_price_message((dec(105, 8) + 100).try_into().unwrap(), 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::BothOraclesUntrusted);
        assert!(price_feed_info.last_good_price == dec(101, 9));
    }

    #[test]
    /// C3 BothOraclesUntrusted: both are live and <= 5% price difference
    fn c3_both_oracles_suspect_live_and_price_similar() {
        let mut price_feed_info = load_price_feed_info(dec(101, 9), Status::BothOraclesUntrusted);
        assert!(price_feed_info.status == Status::BothOraclesUntrusted);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message =
            &load_price_message((dec(105, 8)).try_into().unwrap(), 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::PythWorking);
        assert!(price_feed_info.last_good_price == dec(105, 9));
    }

    /// --- Case 4 ---

    #[test]
    fn c4_using_chainlink_pyth_frozen_both_broken() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingChainlinkPythFrozen);
        assert!(price_feed_info.status == Status::UsingChainlinkPythFrozen);

        let price_chainlink = dec(0, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message = &load_price_message((dec(105, 8)).try_into().unwrap(), 1, 0);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::BothOraclesUntrusted);
        assert!(price_feed_info.last_good_price == dec(101, 9));
    }

    #[test]
    fn c4_using_chainlink_pyth_frozen_pyth_break_chainlink_freeze() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingChainlinkPythFrozen);
        assert!(price_feed_info.status == Status::UsingChainlinkPythFrozen);

        let price_chainlink = dec(100, 8);
        let chainlink_response =
            &load_chainlink_response(1_000_000 - 14400 - 1, price_chainlink.into());

        let pyth_price_message = &load_price_message((dec(105, 8)).try_into().unwrap(), 1, 0);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.last_good_price == dec(101, 9));
    }

    #[test]
    fn c4_using_chainlink_pyth_frozen_pyth_break_chainlink_live() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingChainlinkPythFrozen);
        assert!(price_feed_info.status == Status::UsingChainlinkPythFrozen);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message = &load_price_message((dec(105, 8)).try_into().unwrap(), 1, 0);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.last_good_price == dec(100, 9));
    }

    #[test]
    fn c4_using_chainlink_pyth_frozen_both_live_price_similar() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingChainlinkPythFrozen);
        assert!(price_feed_info.status == Status::UsingChainlinkPythFrozen);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message =
            &load_price_message((dec(105, 8)).try_into().unwrap(), 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::PythWorking);
        assert!(price_feed_info.last_good_price == dec(105, 9));
    }

    #[test]
    fn c4_using_chainlink_pyth_frozen_both_live_price_different() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingChainlinkPythFrozen);
        assert!(price_feed_info.status == Status::UsingChainlinkPythFrozen);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message =
            &load_price_message((dec(105, 8) + 100).try_into().unwrap(), 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingChainlinkPythUntrusted);
        assert!(price_feed_info.last_good_price == dec(100, 9));
    }

    #[test]
    fn c4_using_chainlink_pyth_frozen_pyth_live_chainlink_break() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingChainlinkPythFrozen);
        assert!(price_feed_info.status == Status::UsingChainlinkPythFrozen);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(0, price_chainlink.into());

        let pyth_price_message = &load_price_message(dec(105, 8).try_into().unwrap(), 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingPythChainlinkUntrusted);
        assert!(price_feed_info.last_good_price == dec(105, 9));
    }

    #[test]
    fn c4_using_chainlink_pyth_frozen_pyth_frozen_chainlink_break() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingChainlinkPythFrozen);
        assert!(price_feed_info.status == Status::UsingChainlinkPythFrozen);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(0, price_chainlink.into());

        let pyth_price_message =
            &load_price_message(dec(105, 8).try_into().unwrap(), 1, 1_000_000 - 14401);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingPythChainlinkUntrusted);
        assert!(price_feed_info.last_good_price == dec(101, 9));
    }

    #[test]
    fn c4_using_chainlink_pyth_frozen_pyth_frozen_chainlink_live() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingChainlinkPythFrozen);
        assert!(price_feed_info.status == Status::UsingChainlinkPythFrozen);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message =
            &load_price_message(dec(105, 8).try_into().unwrap(), 1, 1_000_000 - 14401);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingChainlinkPythFrozen);
        assert!(price_feed_info.last_good_price == dec(100, 9));
    }

    #[test]
    fn c4_using_chainlink_pyth_frozen_pyth_frozen_chainlink_frozen() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingChainlinkPythFrozen);
        assert!(price_feed_info.status == Status::UsingChainlinkPythFrozen);

        let price_chainlink = dec(100, 8);
        let chainlink_response =
            &load_chainlink_response(1_000_000 - 14401, price_chainlink.into());

        let pyth_price_message =
            &load_price_message(dec(105, 8).try_into().unwrap(), 1, 1_000_000 - 14401);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingChainlinkPythFrozen);
        assert!(price_feed_info.last_good_price == dec(101, 9));
    }

    /// --- CASE 5 ---

    #[test]
    fn c5_using_pyth_chainlink_untrusted_pyth_live_chainlink_price_different() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingPythChainlinkUntrusted);
        assert!(price_feed_info.status == Status::UsingPythChainlinkUntrusted);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message =
            &load_price_message((dec(105, 8) + 100).try_into().unwrap(), 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingPythChainlinkUntrusted);
        assert!(price_feed_info.last_good_price == dec(105, 9) + 1000);
    }

    #[test]
    fn c5_using_pyth_chainlink_untrusted_pyth_live_chainlink_price_similar() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingPythChainlinkUntrusted);
        assert!(price_feed_info.status == Status::UsingPythChainlinkUntrusted);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message = &load_price_message(dec(105, 8).try_into().unwrap(), 1, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::PythWorking);
        assert!(price_feed_info.last_good_price == dec(105, 9));
    }

    #[test]
    fn c5_using_pyth_chainlink_untrusted_pyth_live_conf_bigger_than_5_percent() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingPythChainlinkUntrusted);
        assert!(price_feed_info.status == Status::UsingPythChainlinkUntrusted);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message =
            &load_price_message(dec(106, 8).try_into().unwrap(), 531_000_000, 1_000_000);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::BothOraclesUntrusted);
        assert!(price_feed_info.last_good_price == dec(101, 9));
    }

    #[test]
    fn c5_using_pyth_chainlink_untrusted_pyth_frozen() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingPythChainlinkUntrusted);
        assert!(price_feed_info.status == Status::UsingPythChainlinkUntrusted);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message =
            &load_price_message(dec(105, 8).try_into().unwrap(), 1, 1_000_000 - 14401);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::UsingPythChainlinkUntrusted);
        assert!(price_feed_info.last_good_price == dec(101, 9));
    }

    #[test]
    fn c5_using_pyth_chainlink_untrusted_pyth_break() {
        let mut price_feed_info =
            load_price_feed_info(dec(101, 9), Status::UsingPythChainlinkUntrusted);
        assert!(price_feed_info.status == Status::UsingPythChainlinkUntrusted);

        let price_chainlink = dec(100, 8);
        let chainlink_response = &load_chainlink_response(1_000_000, price_chainlink.into());

        let pyth_price_message = &load_price_message(dec(105, 8).try_into().unwrap(), 1, 0);

        price_feed_info
            .update(pyth_price_message, chainlink_response, price_chainlink)
            .unwrap();
        assert!(price_feed_info.status == Status::BothOraclesUntrusted);
        assert!(price_feed_info.last_good_price == dec(101, 9));
    }
}
