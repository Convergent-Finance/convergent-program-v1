use anchor_lang::prelude::*;

use crate::{
    errors::CommunityIssuanceError, events::AuthorityChanged, state::CommunityIssuanceConfig,
};

#[derive(Accounts)]
pub struct ChangeAuthority<'info> {
    #[account(mut)]
    pub config: Account<'info, CommunityIssuanceConfig>,

    #[account(
        mut,
        constraint = authority.key() == config.authority @ CommunityIssuanceError::InvalidSigner
    )]
    pub authority: Signer<'info>,

    /// CHECK: No need to check
    pub new_authority: AccountInfo<'info>,
}

pub fn change_authority_handler(ctx: Context<ChangeAuthority>) -> Result<()> {
    let config = &mut ctx.accounts.config;
    let new_authority = ctx.accounts.new_authority.key();

    config.authority = new_authority;

    emit!(AuthorityChanged {
        token: config.cvgt,
        new_authority,
    });
    Ok(())
}
