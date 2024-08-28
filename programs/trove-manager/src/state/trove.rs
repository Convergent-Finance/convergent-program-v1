use anchor_lang::prelude::*;

use crate::{
    constants::DECIMAL_PRECISION,
    errors::BorrowerOpsError,
    events::{
        NodeAdded, NodeRemoved, SurplusPoolCollBalanceUpdated, SurplusPoolCollSent,
        TotalStakesUpdated, TroveSnapshotsUpdated,
    },
    math::{compute_cr, compute_nominal_cr},
    ID,
};

use super::PoolState;

#[account]
#[derive(InitSpace)]
pub struct Trove {
    pub pool_state: Pubkey,
    pub creator: Pubkey,
    pub debt: u64,
    pub coll: u64,
    pub stake: u64,
    pub snapshot_coll_reward: u128,
    pub snapshot_debt_reward: u128,
    pub surplus_balance: u64,
    pub status: TroveStatus,
    pub prev: Pubkey,
    pub next: Pubkey,
}

#[derive(AnchorSerialize, AnchorDeserialize, Copy, Clone, PartialEq, Eq, InitSpace, Default)]
pub enum TroveStatus {
    #[default]
    NonExistent,
    Active,
    ClosedByOwner,
    ClosedByLiquidation,
    ClosedByRedemption,
}

impl Trove {
    pub fn init(&mut self, pool_state: Pubkey, creator: Pubkey, coll_amt: u64, debt_amt: u64) {
        self.pool_state = pool_state;
        self.creator = creator;
        self.coll = coll_amt;
        self.debt = debt_amt;
        self.status = TroveStatus::Active;
        self.stake = 0;
        self.snapshot_debt_reward = 0;
        self.snapshot_coll_reward = 0;
    }

    pub fn get_new_trove_amounts(
        &self,
        coll_change: u64,
        is_coll_increase: bool,
        debt_change: u64,
        is_debt_increase: bool,
    ) -> (u64, u64) {
        let new_coll = if is_coll_increase {
            self.coll.checked_add(coll_change).unwrap()
        } else {
            self.coll.checked_sub(coll_change).unwrap()
        };

        let new_debt = if is_debt_increase {
            self.debt.checked_add(debt_change).unwrap()
        } else {
            self.debt.checked_sub(debt_change).unwrap()
        };
        (new_coll, new_debt)
    }

    pub fn get_new_icr_from_trove_change(
        &self,
        coll_change: u64,
        is_coll_increase: bool,
        debt_change: u64,
        is_debt_increase: bool,
        price: u64,
    ) -> u64 {
        let (new_coll, new_debt) = self.get_new_trove_amounts(
            coll_change,
            is_coll_increase,
            debt_change,
            is_debt_increase,
        );
        compute_cr(new_coll, new_debt, price).unwrap()
    }

    pub fn get_new_norminal_icr_from_trove_change(
        &self,
        coll_change: u64,
        is_coll_increase: bool,
        debt_change: u64,
        is_debt_increase: bool,
    ) -> u64 {
        let (new_coll, new_debt) = self.get_new_trove_amounts(
            coll_change,
            is_coll_increase,
            debt_change,
            is_debt_increase,
        );
        compute_nominal_cr(new_coll, new_debt).unwrap()
    }

    pub fn get_icr(&self, price: u64) -> u64 {
        compute_cr(self.coll, self.debt, price).unwrap()
    }

    pub fn compute_new_stake(&self, pool_state: &PoolState, coll: u64) -> u64 {
        if pool_state.total_coll_snapshot == 0 {
            return coll;
        } else {
            assert!(pool_state.total_stakes_snapshot > 0);
            return u64::try_from(
                u128::from(coll)
                    .checked_mul(pool_state.total_stakes_snapshot.into())
                    .unwrap()
                    .checked_div(pool_state.total_coll_snapshot.into())
                    .unwrap(),
            )
            .unwrap();
        }
    }

    pub fn validate_head_tail(
        &self,
        id: &Pubkey,
        nicr: u64,
        prev: &Option<Box<Account<'_, Self>>>,
        next: &Option<Box<Account<'_, Self>>>,
        pool_state: &PoolState,
    ) -> Result<()> {
        // `(null, null)` is a valid insert position if number of trove is zero
        if prev.is_none() && next.is_none() {
            require!(
                pool_state.trove_size == 0,
                BorrowerOpsError::InvalidTroveNeighbor
            );
        } else if prev.is_none() {
            // `(null, next)` is a valid insert position if `next` is the head of the list
            let next_account = next.as_ref().unwrap();
            if pool_state.trove_head == *id {
                require!(
                    self.next == next_account.key()
                        && nicr >= next_account.get_nominal_icr(pool_state),
                    BorrowerOpsError::InvalidTroveNeighbor
                );
            } else {
                require!(
                    pool_state.trove_head == next_account.key()
                        && nicr >= next_account.get_nominal_icr(pool_state),
                    BorrowerOpsError::InvalidTroveNeighbor
                );
            }
        } else if next.is_none() {
            // `(prev, null)` is a valid insert position if `prev` is the tail of the list
            let prev_account = prev.as_ref().unwrap();
            if pool_state.trove_tail == *id {
                require!(
                    self.prev == prev_account.key()
                        && nicr <= prev_account.get_nominal_icr(pool_state),
                    BorrowerOpsError::InvalidTroveNeighbor
                );
            } else {
                require!(
                    pool_state.trove_tail == prev_account.key()
                        && nicr <= prev_account.get_nominal_icr(pool_state),
                    BorrowerOpsError::InvalidTroveNeighbor
                );
            }
        } else {
            // `(prev, next)` is a valid insert position if they are adjacent nodes and `NICR` falls between the two nodes' NICRs
            let next_account = next.as_ref().unwrap();
            let prev_account = prev.as_ref().unwrap();
            if prev_account.next != next_account.key() {
                require!(
                    prev_account.next == *id && next_account.prev == *id,
                    BorrowerOpsError::InvalidTroveNeighbor
                );
            } else {
                require!(
                    prev_account.next == next_account.key()
                        && next_account.prev == prev_account.key(),
                    BorrowerOpsError::InvalidTroveNeighbor
                );
            }
            require!(
                prev_account.get_nominal_icr(pool_state) >= nicr
                    && nicr >= next_account.get_nominal_icr(pool_state),
                BorrowerOpsError::InvalidTroveNeighbor
            );
        }
        Ok(())
    }

    pub fn require_trove_not_active(&self) -> Result<()> {
        require!(
            self.status != TroveStatus::Active,
            BorrowerOpsError::TroveIsActive
        );
        Ok(())
    }

    pub fn require_trove_active(&self) -> Result<()> {
        require!(
            self.status == TroveStatus::Active,
            BorrowerOpsError::TroveIsNotActive
        );
        Ok(())
    }

    pub fn has_pending_rewards(&self, pool_state: &PoolState) -> bool {
        if self.status != TroveStatus::Active {
            return false;
        }
        self.snapshot_coll_reward < pool_state.l_coll
    }

    pub fn get_pending_coll_reward(&self, pool_state: &PoolState) -> u64 {
        let snapshot_coll = self.snapshot_coll_reward;
        let reward_per_unit_staked = pool_state.l_coll.checked_sub(snapshot_coll).unwrap();
        if reward_per_unit_staked == 0 || self.status != TroveStatus::Active {
            return 0;
        }
        let stake = self.stake;
        u64::try_from(
            u128::from(stake)
                .checked_mul(reward_per_unit_staked.into())
                .unwrap()
                .checked_div(DECIMAL_PRECISION.into())
                .unwrap(),
        )
        .unwrap()
    }

    pub fn get_pending_debt_reward(&self, pool_state: &PoolState) -> u64 {
        let snapshot_debt = self.snapshot_debt_reward;
        let reward_per_unit_staked = pool_state.l_usv_debt.checked_sub(snapshot_debt).unwrap();
        if reward_per_unit_staked == 0 || self.status != TroveStatus::Active {
            return 0;
        }
        let stake = self.stake;
        u64::try_from(
            u128::from(stake)
                .checked_mul(reward_per_unit_staked.into())
                .unwrap()
                .checked_div(DECIMAL_PRECISION.into())
                .unwrap(),
        )
        .unwrap()
    }

    pub fn get_entire_debt_coll(&self, pool_state: &PoolState) -> (u64, u64, u64, u64) {
        let mut debt = self.debt;
        let mut coll = self.coll;

        let pending_usv_debt_reward = self.get_pending_debt_reward(pool_state);
        let pending_coll_reward = self.get_pending_coll_reward(pool_state);

        debt = debt.checked_add(pending_usv_debt_reward).unwrap();
        coll = coll.checked_add(pending_coll_reward).unwrap();
        (debt, coll, pending_usv_debt_reward, pending_coll_reward)
    }

    pub fn get_current_amounts(&self, pool_state: &PoolState) -> (u64, u64) {
        let pending_coll_reward = self.get_pending_coll_reward(pool_state);
        let pending_debt_reward = self.get_pending_debt_reward(pool_state);
        let current_coll = self.coll.checked_add(pending_coll_reward).unwrap();
        let current_debt = self.debt.checked_add(pending_debt_reward).unwrap();
        (current_coll, current_debt)
    }

    pub fn get_nominal_icr(&self, pool_state: &PoolState) -> u64 {
        let (current_coll, current_debt) = self.get_current_amounts(pool_state);
        compute_nominal_cr(current_coll, current_debt).unwrap()
    }

    pub fn get_current_icr(&self, pool_state: &PoolState, price: u64) -> u64 {
        let (current_coll, current_debt) = self.get_current_amounts(pool_state);
        compute_cr(current_coll, current_debt, price).unwrap()
    }

    pub fn update_from_adjustment(
        &mut self,
        coll_change: u64,
        is_coll_increase: bool,
        debt_change: u64,
        is_debt_increase: bool,
    ) -> (u64, u64) {
        let (new_coll, new_debt) = self.get_new_trove_amounts(
            coll_change,
            is_coll_increase,
            debt_change,
            is_debt_increase,
        );
        self.coll = new_coll;
        self.debt = new_debt;
        (self.coll, self.debt)
    }

    pub fn close_trove(
        &mut self,
        pool_state: &mut PoolState,
        closed_status: TroveStatus,
    ) -> Result<()> {
        assert!(closed_status != TroveStatus::NonExistent && closed_status != TroveStatus::Active);
        pool_state.require_more_than_one_trove_in_system()?;
        self.status = closed_status;
        self.coll = 0;
        self.debt = 0;
        self.snapshot_coll_reward = 0;
        self.snapshot_debt_reward = 0;

        Ok(())
    }

    pub fn update_reward_snapshot(&mut self, pool_state: &PoolState) {
        self.snapshot_coll_reward = pool_state.l_coll;
        self.snapshot_debt_reward = pool_state.l_usv_debt;
        emit!(TroveSnapshotsUpdated {
            l_coll: pool_state.l_coll,
            l_usv_debt: pool_state.l_usv_debt,
        });
    }

    pub fn update_stake_and_total_stakes(&mut self, pool_state: &mut PoolState) -> u64 {
        let new_stake = self.compute_new_stake(pool_state, self.coll);
        let old_stake = self.stake;
        self.stake = new_stake;
        pool_state.total_stakes = pool_state
            .total_stakes
            .checked_sub(old_stake)
            .unwrap()
            .checked_add(new_stake)
            .unwrap();

        emit!(TotalStakesUpdated {
            new_total_stakes: pool_state.total_stakes
        });
        new_stake
    }

    pub fn remove_stake(&mut self, pool_state: &mut PoolState) {
        let stake = self.stake;
        pool_state.total_stakes = pool_state.total_stakes.checked_sub(stake).unwrap();
        self.stake = 0;
    }

    pub fn account_surplus(&mut self, amount: u64) {
        let new_amount = self.surplus_balance.checked_add(amount).unwrap();
        self.surplus_balance = new_amount;

        emit!(SurplusPoolCollBalanceUpdated {
            account: self.creator,
            new_balance: self.surplus_balance
        });
    }

    pub fn clear_surplus(&mut self) -> u64 {
        let amount = self.surplus_balance;
        self.surplus_balance = 0;

        emit!(SurplusPoolCollSent {
            amount,
            to: self.creator
        });
        return amount;
    }

    pub fn insert_sorted(
        &mut self,
        id: Pubkey,
        nicr: u64,
        prev: &mut Option<Box<Account<'_, Self>>>,
        next: &mut Option<Box<Account<'_, Self>>>,
        pool_state: &mut PoolState,
    ) -> Result<()> {
        self.validate_head_tail(&id, nicr, prev, next, pool_state)?;
        if prev.is_none() && next.is_none() {
            // Insert trove as head and tail
            pool_state.trove_head = id;
            pool_state.trove_tail = id;
        } else if prev.is_none() {
            // Insert before `prev` as the head
            self.next = pool_state.trove_head;
            self.prev = Pubkey::default();
            next.as_mut().unwrap().reload()?;
            next.as_mut().unwrap().prev = id;
            pool_state.trove_head = id;
        } else if next.is_none() {
            // Insert after `next` as the tail
            self.prev = pool_state.trove_tail;
            self.next = Pubkey::default();
            prev.as_mut().unwrap().reload()?;
            prev.as_mut().unwrap().next = id;
            pool_state.trove_tail = id;
        } else {
            // Insert at insert position between `prev` and `next`
            self.next = next.as_mut().unwrap().key();
            self.prev = prev.as_mut().unwrap().key();
            next.as_mut().unwrap().reload()?;
            next.as_mut().unwrap().prev = id;
            prev.as_mut().unwrap().reload()?;
            prev.as_mut().unwrap().next = id;
        }
        pool_state.trove_size = pool_state.trove_size.checked_add(1).unwrap();
        emit!(NodeAdded {
            owner: self.creator,
            nicr
        });
        Ok(())
    }

    pub fn remove_sorted(
        &mut self,
        id: Pubkey,
        prev: &mut Option<Box<Account<'_, Self>>>,
        next: &mut Option<Box<Account<'_, Self>>>,
        pool_state: &mut PoolState,
    ) -> Result<()> {
        if pool_state.trove_size > 1 {
            // List contains more than a single node
            if id == pool_state.trove_head {
                require!(
                    prev.is_none() && next.as_ref().unwrap().prev == id,
                    BorrowerOpsError::InvalidTroveNeighbor
                );
                // The removed node is the head
                // Set head to next node
                pool_state.trove_head = next.as_ref().unwrap().key();
                // Set prev pointer of new head to default
                next.as_mut().unwrap().prev = Pubkey::default();
            } else if id == pool_state.trove_tail {
                require!(
                    next.is_none() && prev.as_ref().unwrap().next == id,
                    BorrowerOpsError::InvalidTroveNeighbor
                );
                // The removed node is the tail
                // Set tail to previous node
                pool_state.trove_tail = prev.as_ref().unwrap().key();
                // Set next pointer of new tail to default
                prev.as_mut().unwrap().next = Pubkey::default();
            } else {
                require!(
                    prev.as_ref().unwrap().next == id && next.as_ref().unwrap().prev == id,
                    BorrowerOpsError::InvalidTroveNeighbor
                );
                // The removed node is neither the head nor the tail
                // Set prev pointer of next node to the previous node
                next.as_mut().unwrap().prev = prev.as_ref().unwrap().key();
                // Set next pointer of previous node to the next node
                prev.as_mut().unwrap().next = next.as_ref().unwrap().key();
            }
        } else {
            pool_state.trove_head = Pubkey::default();
            pool_state.trove_tail = Pubkey::default();
        }
        self.next = Pubkey::default();
        self.prev = Pubkey::default();
        pool_state.trove_size = pool_state.trove_size.checked_sub(1).unwrap();
        emit!(NodeRemoved {
            owner: self.creator
        });
        Ok(())
    }

    pub fn re_insert(
        &mut self,
        id: Pubkey,
        new_nicr: u64,
        cur_prev: &mut Option<Box<Account<'_, Self>>>,
        cur_next: &mut Option<Box<Account<'_, Self>>>,
        new_prev: &mut Option<Box<Account<'_, Self>>>,
        new_next: &mut Option<Box<Account<'_, Self>>>,
        pool_state: &mut PoolState,
    ) -> Result<()> {
        require!(new_nicr > 0, BorrowerOpsError::NICRZero);

        self.remove_sorted(id, cur_prev, cur_next, pool_state)?;
        if cur_prev.is_some() {
            cur_prev.as_mut().unwrap().exit(&ID)?;
        }
        if cur_next.is_some() {
            cur_next.as_mut().unwrap().exit(&ID)?;
        }
        self.insert_sorted(id, new_nicr, new_prev, new_next, pool_state)?;
        Ok(())
    }

    pub fn remove_sorted_redemption(
        &mut self,
        accounts: &[AccountInfo<'_>],
        id: Pubkey,
        pool_state: &mut PoolState,
    ) -> Result<()> {
        if pool_state.trove_size > 1 {
            // List contains more than a single node
            if id == pool_state.trove_head {
                // The removed node is the head
                // Set head to next node
                pool_state.trove_head = self.next;
                let mut head_data = accounts
                    [find_trove_index(accounts, pool_state.trove_head).unwrap()]
                .try_borrow_mut_data()?;
                let mut head = Trove::try_deserialize(&mut head_data.as_ref())
                    .expect("Error Deserializing Data");
                // Set prev pointer of new head to default
                head.prev = Pubkey::default();
                head.try_serialize(&mut head_data.as_mut())?;
            } else if id == pool_state.trove_tail {
                // The removed node is the tail
                // Set tail to previous node
                pool_state.trove_tail = self.prev;
                let mut tail_data = accounts
                    [find_trove_index(accounts, pool_state.trove_tail).unwrap()]
                .try_borrow_mut_data()?;
                let mut tail = Trove::try_deserialize(&mut tail_data.as_ref())
                    .expect("Error Deserializing Data");
                // Set next pointer of new tail to null
                tail.next = Pubkey::default();
                tail.try_serialize(&mut tail_data.as_mut())?;
            } else {
                // The removed node is neither the head nor the tail
                // Set next pointer of previous node to the next node
                let mut prev_data = accounts[find_trove_index(accounts, self.prev).unwrap()]
                    .try_borrow_mut_data()?;
                let mut prev = Trove::try_deserialize(&mut prev_data.as_ref())
                    .expect("Error Deserializing Data");
                prev.next = self.next;
                prev.try_serialize(&mut prev_data.as_mut())?;
                // Set prev pointer of next node to the previous node
                let mut next_data = accounts[find_trove_index(accounts, self.next).unwrap()]
                    .try_borrow_mut_data()?;
                let mut next = Trove::try_deserialize(&mut next_data.as_ref())
                    .expect("Error Deserializing Data");
                next.prev = self.prev;
                next.try_serialize(&mut next_data.as_mut())?;
            }
        } else {
            pool_state.trove_head = Pubkey::default();
            pool_state.trove_tail = Pubkey::default();
        }
        self.next = Pubkey::default();
        self.prev = Pubkey::default();
        pool_state.trove_size = pool_state.trove_size.checked_sub(1).unwrap();
        emit!(NodeRemoved {
            owner: self.creator
        });
        Ok(())
    }

    pub fn re_insert_redemption(
        &mut self,
        accounts: &[AccountInfo<'_>],
        id: Pubkey,
        new_nicr: u64,
        new_prev: &mut Option<Box<Account<'_, Self>>>,
        new_next: &mut Option<Box<Account<'_, Self>>>,
        pool_state: &mut PoolState,
    ) -> Result<()> {
        require!(new_nicr > 0, BorrowerOpsError::NICRZero);
        self.remove_sorted_redemption(accounts, id, pool_state)?;
        self.insert_sorted(id, new_nicr, new_prev, new_next, pool_state)
    }
}

pub fn find_trove_index(remaining_accounts: &[AccountInfo<'_>], id: Pubkey) -> Option<usize> {
    for i in 0..remaining_accounts.len() {
        if remaining_accounts[i].key() == id {
            return Some(i);
        }
    }
    None
}
