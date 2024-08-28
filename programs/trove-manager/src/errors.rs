use anchor_lang::prelude::*;

#[error_code]
pub enum BorrowerOpsError {
    #[msg("BorrowerOps: Trove's net debt must be greater than minimum")]
    DebtLessThanMin,
    #[msg("BorrowerOps: Recovery mode not allow")]
    RecoveryMode,
    #[msg("Calculation Error")]
    Calculation,
    #[msg("BorrowerOps: An operation that would result in ICR < MCR is not permitted")]
    ICRLowerThanMCR,
    #[msg("BorrowerOps: An operation that would result in TCR < CCR is not permitted")]
    TCRLowerThanCCR,
    #[msg("BorrowerOps: Operation must leave trove with ICR >= CCR")]
    ICRLowerThanCCR,
    #[msg("BorrowerOps: Cannot decrease your Trove's ICR in Recovery Mode")]
    NewICRLowerThanOldICR,
    #[msg("Max fee percentage must less than or equal to 100%")]
    InvalidMaxFeeRecoveryMode,
    #[msg("Max fee percentage must be between 0.5% and 100%")]
    InvalidRedeemMaxFee,
    #[msg("Max redeem fee percentage must be between 0.5% and 100%")]
    InvalidMaxFee,
    #[msg("Fee exceeded provided maximum")]
    FeeExceededMax,
    #[msg("Not support withdraw lamport")]
    IsLamportNotSupported,
    #[msg("BorrowerOps: Trove is active")]
    TroveIsActive,
    #[msg("BorrowerOps: Trove does not exist or is closed")]
    TroveIsNotActive,
    #[msg("BorrowerOps: Debt increase requires non-zero debtChange")]
    ZeroDebtChange,
    #[msg("BorrowerOps: There must be either a collateral change or a debt change")]
    ZeroAdjustment,
    #[msg("BorrowerOps: Collateral withdraw exceed trove coll")]
    CollateralWithdrawExceedBalance,
    #[msg("BorrowerOps: Collateral withdrawal not permitted Recovery Mode")]
    RecoveryNoCollWithdraw,
    #[msg("BorrowerOps: Amount repaid must not be larger than the Trove's debt")]
    InvalidUSVRepayment,
    #[msg("BorrowerOps: Caller doesnt have enough USV to make repayment")]
    InsufficientUSVBalance,
    #[msg("BorrowerOps: Operation not permitted during Recovery Mode")]
    InRecoveryMode,
    #[msg("TroveManager: Only one trove in the system")]
    OnlyOneTrove,
    #[msg("TroveManager: Cannot redeem when TCR < MCR")]
    TCRUnderMCR,
    #[msg("TroveManager: Amount must be greater than zero")]
    ZeroRedeemAmount,
    #[msg("TroveManager: Unable to redeem any amount")]
    ZeroCollDrawn,
    #[msg("TroveManager: Fee would eat up all returned collateral")]
    FeeEatUpAllColl,
    #[msg("Nothing to liquidate")]
    LiquidateZeroDebt,
    #[msg("Account not owned by program")]
    InvalidAccount,
    #[msg("Invalid trove's neighbor")]
    InvalidTroveNeighbor,
    #[msg("SortedTroves: NICR must be positive")]
    NICRZero,
}

#[error_code]
pub enum StabilityPoolError {
    #[msg("Invalid provided epoch scale")]
    InvalidEpochScale,
    #[msg("StabilityPool: User must have a non-zero deposit")]
    ZeroDeposit,
    #[msg("StabilityPool: Cannot withdraw while there are troves with ICR < MCR")]
    TroveUnderColl,
    #[msg("StabilityPool: Invalid lowest trove")]
    InvalidLowestTrove,
    #[msg("StabilityPool: Amount must be non-zero")]
    ZeroAmount,
}

#[error_code]
pub enum PriceFeedError {
    #[msg("PriceFeed: pyth must be working and current")]
    InitializePythNotWorking,
    #[msg("PriceFeed: chainlink wrong feed")]
    ChainlinkWrongFeed,
    #[msg("PriceFeed: pyth wrong feed")]
    PythWrongFeed,
    #[msg("PriceFeed: jitosol stake pool wrong")]
    JitoSolStakePoolWrong,
    #[msg("PriceFeed: staking pool wrong account")]
    StakingPoolWrong,
    #[msg("PriceFeed: Only support in dev mode")]
    OnlyDevMode,
    #[msg("PriceFeed: JitoSol stake list and pool out of date")]
    PoolNotUpdated,
}

#[error_code]
pub enum CommunityIssuanceError {
    #[msg("Invalid signer")]
    InvalidSigner,
    #[msg("Exceed maximum emission rate")]
    ExceedMax,
}

#[error_code]
pub enum CVGTStakingError {
    #[msg("CVGTStaking: Amount must be non-zero")]
    ZeroAmount,
    #[msg("CVGTStaking: User must have a non-zero stake")]
    UserNotHasStake,
    #[msg("CVGTStaking: Invalid signer")]
    InvalidSigner,
}
