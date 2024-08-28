use crate::{
    constants::{DEPLOYER, TREASURY_VAULT},
    state::{PoolState, StabilityPoolState},
};
use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{set_authority, spl_token::instruction::AuthorityType, Mint, SetAuthority, Token},
};

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = creator,
        space = 8 + PoolState::INIT_SPACE,
        seeds = [
            b"state", 
            cvgt.key().as_ref(),
        ],
        bump
    )]
    pub pool_state: Box<Account<'info, PoolState>>,

    #[account(
        init,
        payer = creator,
        space = 8 + StabilityPoolState::INIT_SPACE,
        seeds = [
            b"stability", 
            pool_state.key().as_ref(),
        ],
        bump
    )]
    pub stability_pool_state: Box<Account<'info, StabilityPoolState>>,

    #[account(
        mut,
        mint::authority = creator
    )]
    pub stablecoin: Box<Account<'info, Mint>>,

    #[account()]
    pub collateral: Box<Account<'info, Mint>>,

    #[account()]
    pub cvgt: Box<Account<'info, Mint>>,

    #[account(
        mut,
        constraint = creator.key() == DEPLOYER
    )]
    pub creator: Signer<'info>,

    /// CHECK: This account is not read or written
    #[account(
        seeds = [
            b"token-authority",
            pool_state.key().as_ref()
        ],
        bump
    )]
    pub token_authority: UncheckedAccount<'info>,
    /// CHECK: This account is not read or written
    pub cvgt_staking_state: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

impl<'info> Initialize<'info> {
    pub fn transfer_auth_ctx(&self) -> CpiContext<'_, '_, '_, 'info, SetAuthority<'info>> {
        let cpi_accounts = SetAuthority {
            account_or_mint: self.stablecoin.to_account_info(),
            current_authority: self.creator.to_account_info(),
        };

        let cpi_program = self.token_program.to_account_info();

        CpiContext::new(cpi_program, cpi_accounts)
    }
}

pub fn initialize_handler(
    ctx: Context<Initialize>,
    mcr: u64,
    ccr: u64,
    min_net_debt: u64,
    gas_compensation: u64,
    coll_gas_comp_percent_divisor: u64,
) -> Result<()> {
    let creator = ctx.accounts.creator.key();
    let stablecoin = ctx.accounts.stablecoin.key();
    let collateral = ctx.accounts.collateral.key();
    let cvgt = ctx.accounts.cvgt.key();
    let cvgt_staking_state = if cfg!(feature = "dev") {
        ctx.accounts.cvgt_staking_state.key()
    } else {
        TREASURY_VAULT
    };

    let token_authority = ctx.accounts.token_authority.key();

    let token_auth_bump = ctx.bumps.token_authority;
    let stability_pool_bump = ctx.bumps.stability_pool_state;
    let bump = ctx.bumps.pool_state;

    set_authority(
        ctx.accounts.transfer_auth_ctx(),
        AuthorityType::MintTokens,
        Some(token_authority),
    )?;

    let pool_state = &mut ctx.accounts.pool_state;

    pool_state.init(
        creator,
        stablecoin,
        collateral,
        cvgt,
        cvgt_staking_state,
        mcr,
        ccr,
        min_net_debt,
        gas_compensation,
        coll_gas_comp_percent_divisor,
        [token_auth_bump],
        [bump],
        [stability_pool_bump],
    );

    let stability_pool_state = &mut ctx.accounts.stability_pool_state;

    stability_pool_state.init(cvgt);

    Ok(())
}
