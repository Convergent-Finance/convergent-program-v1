use anchor_lang::prelude::{
    borsh::{BorshDeserialize, BorshSerialize},
    *,
};

#[event]
pub struct TroveCreated {
    pub borrower: Pubkey,
}

#[event]
pub struct TroveUpdated {
    pub borrower: Pubkey,
    pub debt: u64,
    pub coll: u64,
    pub stake: u64,
    pub operation: Operation,
}

#[event]
pub struct Liquidation {
    pub debt: u64,
    pub coll: u64,
    pub total_usv_compensation: u64,
    pub total_coll_compensation: u64,
}

#[event]
pub struct Redemption {
    pub attempted_usv_amount: u64,
    pub actual_usv_amount: u64,
    pub coll_sent: u64,
    pub coll_fee: u64,
}

#[event]
pub struct TroveLiquidated {
    pub borrower: Pubkey,
    pub debt: u64,
    pub coll: u64,
    pub operation: Operation,
}

#[event]
pub struct BaseRateUpdated {
    pub base_rate: u64,
}

#[event]
pub struct LastFeeOpTimeUpdated {
    pub last_fee_op_time: u64,
}

#[event]
pub struct TotalStakesUpdated {
    pub new_total_stakes: u64,
}

#[event]
pub struct SystemSnapshotsUpdated {
    pub total_stakes_snapshot: u64,
    pub total_coll_snapshot: u64,
}

#[event]
pub struct LTermsUpdated {
    pub l_coll: u64,
    pub l_usv_debt: u64,
}

#[event]
pub struct TroveSnapshotsUpdated {
    pub l_coll: u128,
    pub l_usv_debt: u128,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub enum Operation {
    OpenTrove,
    CloseTrove,
    AdjustTrove,
    ApplyPendingRewards,
    LiquidateInNormalMode,
    LiquidateInRecoveryMode,
    RedeemCollateral,
}

#[event]
pub struct USVBorrowingFeePaid {
    pub borrower: Pubkey,
    pub usv_fee: u64,
}

// SurplusPool
#[event]
pub struct SurplusPoolCollBalanceUpdated {
    pub account: Pubkey,
    pub new_balance: u64,
}

#[event]
pub struct SurplusPoolCollSent {
    pub to: Pubkey,
    pub amount: u64,
}

// SortedTroves
#[event]
pub struct NodeAdded {
    pub owner: Pubkey,
    pub nicr: u64,
}

#[event]
pub struct NodeRemoved {
    pub owner: Pubkey,
}

// StabilityPool
#[event]
pub struct CVGTPaidToDepositor {
    pub depositor: Pubkey,
    pub cvgt_gain: u64,
}

#[event]
pub struct StabilityPoolUSVBalanceUpdated {
    pub new_balance: u64,
}

#[event]
pub struct StabilityPoolCollBalanceUpdated {
    pub new_balance: u64,
}

#[event]
pub struct UserDepositChanged {
    pub depositor: Pubkey,
    pub new_deposit: u64,
}

#[event]
pub struct CollGainWithdrawn {
    pub depositor: Pubkey,
    pub coll: u64,
    pub usv_loss: u64,
}

#[event]
pub struct DepositSnapshotUpdated {
    pub depositor: Pubkey,
    pub p: u128,
    pub s: u128,
    pub g: u128,
}

#[event]
pub struct GUpdated {
    pub g: u128,
    pub epoch: u128,
    pub scale: u128,
}

#[event]
pub struct SUpdated {
    pub s: u128,
    pub epoch: u128,
    pub scale: u128,
}

#[event]
pub struct PUpdated {
    pub p: u128,
}

#[event]
pub struct EpochUpdated {
    pub current_epoch: u128,
}

#[event]
pub struct ScaleUpdated {
    pub current_scale: u128,
}

// Community Issuance
#[event]
pub struct TotalTokenIssuedUpdated {
    pub token: Pubkey,
    pub total_cvgt_issued: u64,
}

#[event]
pub struct EmissionEnabled {
    pub token: Pubkey,
}

#[event]
pub struct EmissionRateChanged {
    pub token: Pubkey,
    pub new_rate: u64,
}

#[event]
pub struct AuthorityChanged {
    pub token: Pubkey,
    pub new_authority: Pubkey,
}

// CVGT Staking
#[event]
pub struct StakerSnapshotsUpdated {
    pub user: Pubkey,
    pub f_coll: u64,
    pub f_usv: u64,
}

#[event]
pub struct TotalCVGTStakedUpdated {
    pub total_cvgt_staked: u64,
}

#[event]
pub struct StakeChanged {
    pub staker: Pubkey,
    pub new_stake: u64,
}

#[event]
pub struct StakingGainsWithdrawn {
    pub staker: Pubkey,
    pub usv_gain: u64,
    pub coll_gain: u64,
}

#[event]
pub struct FCollUpdated {
    pub f_coll: u64,
}

#[event]
pub struct FUSVUpdated {
    pub f_usv: u64,
}
