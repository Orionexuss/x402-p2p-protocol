use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{self, CloseAccount, Mint, TokenAccount, TokenInterface, TransferChecked},
};
use solana_program::hash::hashv;

declare_id!("CecHZhrZPyLZYFu1R3msJJEQeRDis83K3i99sRXydft3");

const ESCROW_SEED: &[u8] = b"escrow_v2";
const VAULT_SEED: &[u8] = b"vault";

#[program]
pub mod x402_contract {
    use super::*;

    pub fn lock_payment(
        ctx: Context<LockPayment>,
        infohash: [u8; 20],
        amount: u64,
        merkle_root: [u8; 32],
        total_secrets: u32,
    ) -> Result<()> {
        require!(amount > 0, PaymentError::ZeroAmount);
        require!(total_secrets > 0, PaymentError::InvalidTotalSecrets);

        msg!(
            " Locking {} USDC base units for torrent {:?} with seeder {} ",
            amount,
            infohash,
            ctx.accounts.seeder.key()
        );
        // Scope the mutable borrow so it is dropped before the CPI call below,
        // which needs an immutable borrow of the same account.
        {
            let escrow = &mut ctx.accounts.escrow;
            escrow.leecher = ctx.accounts.leecher.key();
            escrow.seeder = ctx.accounts.seeder.key();
            escrow.infohash = infohash;
            escrow.amount = amount;
            escrow.total_secrets = total_secrets;
            escrow.bump = ctx.bumps.escrow;
        }

        token_interface::transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.leecher_usdc_ata.to_account_info(),
                    mint: ctx.accounts.usdc_mint.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                    authority: ctx.accounts.leecher.to_account_info(),
                },
            ),
            amount,
            ctx.accounts.usdc_mint.decimals,
        )?;

        let escrow = &mut ctx.accounts.escrow;
        escrow.vault = ctx.accounts.vault.key();
        escrow.usdc_mint = ctx.accounts.usdc_mint.key();
        escrow.merkle_root = merkle_root;

        msg!(
            "Locked {} USDC base units for torrent {:?}, seeder {}",
            amount,
            infohash,
            ctx.accounts.seeder.key(),
        );
        Ok(())
    }

    pub fn claim_by_secrets(
        ctx: Context<ClaimBySecrets>,
        infohash: [u8; 20],
        claims: Vec<SecretClaim>,
    ) -> Result<()> {
        require!(!claims.is_empty(), PaymentError::NoSecretClaims);
        require!(
            claims.len() <= ClaimBySecrets::MAX_SECRET_CLAIMS,
            PaymentError::TooManySecretClaims
        );

        let escrow = &ctx.accounts.escrow;
        let leecher_key = escrow.leecher;
        let seeder_key = escrow.seeder;
        let escrow_bump = escrow.bump;
        let total_secrets = escrow.total_secrets;

        let mut proven_indices: Vec<u32> = Vec::with_capacity(claims.len());
        let mut valid_count: u32 = 0;

        for claim in &claims {
            if claim.index >= total_secrets {
                continue;
            }

            if proven_indices.iter().any(|&i| i == claim.index) {
                continue;
            }

            let leaf_hash = hashv(&[&claim.secret]).to_bytes();
            if verify_merkle_proof(leaf_hash, &claim.proof, claim.index, escrow.merkle_root) {
                proven_indices.push(claim.index);
                valid_count = valid_count.saturating_add(1);
            }
        }

        let vault_balance = ctx.accounts.vault.amount;
        let seeder_amount_u128 =
            (vault_balance as u128).saturating_mul(valid_count as u128) / (total_secrets as u128);
        let seeder_amount = seeder_amount_u128 as u64;
        let leecher_amount = vault_balance.saturating_sub(seeder_amount);

        let signer_seeds: &[&[u8]] = &[
            ESCROW_SEED,
            leecher_key.as_ref(),
            seeder_key.as_ref(),
            &infohash,
            &[escrow_bump],
        ];

        if seeder_amount > 0 {
            token_interface::transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.vault.to_account_info(),
                        mint: ctx.accounts.usdc_mint.to_account_info(),
                        to: ctx.accounts.seeder_usdc_ata.to_account_info(),
                        authority: ctx.accounts.escrow.to_account_info(),
                    },
                    &[signer_seeds],
                ),
                seeder_amount,
                ctx.accounts.usdc_mint.decimals,
            )?;
        }

        if leecher_amount > 0 {
            token_interface::transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.vault.to_account_info(),
                        mint: ctx.accounts.usdc_mint.to_account_info(),
                        to: ctx.accounts.leecher_usdc_ata.to_account_info(),
                        authority: ctx.accounts.escrow.to_account_info(),
                    },
                    &[signer_seeds],
                ),
                leecher_amount,
                ctx.accounts.usdc_mint.decimals,
            )?;
        }

        token_interface::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.vault.to_account_info(),
                destination: ctx.accounts.leecher.to_account_info(),
                authority: ctx.accounts.escrow.to_account_info(),
            },
            &[signer_seeds],
        ))?;

        msg!(
            "Claim settlement complete for torrent {:?}: valid {}/{}, seeder {}, leecher {}",
            infohash,
            valid_count,
            total_secrets,
            seeder_amount,
            leecher_amount,
        );

        Ok(())
    }

    pub fn release_payment(ctx: Context<ReleasePayment>, infohash: [u8; 20]) -> Result<()> {
        let amount = ctx.accounts.escrow.amount;
        let escrow_bump = ctx.accounts.escrow.bump;
        let leecher_key = ctx.accounts.leecher.key();
        let seeder_key = ctx.accounts.seeder.key();

        let signer_seeds: &[&[u8]] = &[
            ESCROW_SEED,
            leecher_key.as_ref(),
            seeder_key.as_ref(),
            &infohash,
            &[escrow_bump],
        ];

        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.vault.to_account_info(),
                    mint: ctx.accounts.usdc_mint.to_account_info(),
                    to: ctx.accounts.seeder_usdc_ata.to_account_info(),
                    authority: ctx.accounts.escrow.to_account_info(),
                },
                &[signer_seeds],
            ),
            amount,
            ctx.accounts.usdc_mint.decimals,
        )?;

        token_interface::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.vault.to_account_info(),
                destination: ctx.accounts.leecher.to_account_info(),
                authority: ctx.accounts.escrow.to_account_info(),
            },
            &[signer_seeds],
        ))?;

        msg!(
            "Released {} USDC base units to seeder {} for torrent {:?}",
            amount,
            ctx.accounts.seeder.key(),
            ctx.accounts.escrow.infohash,
        );
        Ok(())
    }

    pub fn refund(ctx: Context<Refund>) -> Result<()> {
        let amount = ctx.accounts.escrow.amount;
        let escrow_bump = ctx.accounts.escrow.bump;
        let infohash = ctx.accounts.escrow.infohash;
        let leecher_key = ctx.accounts.leecher.key();
        let seeder_key = ctx.accounts.escrow.seeder;

        let signer_seeds: &[&[u8]] = &[
            ESCROW_SEED,
            leecher_key.as_ref(),
            seeder_key.as_ref(),
            &infohash,
            &[escrow_bump],
        ];

        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.vault.to_account_info(),
                    mint: ctx.accounts.usdc_mint.to_account_info(),
                    to: ctx.accounts.leecher_usdc_ata.to_account_info(),
                    authority: ctx.accounts.escrow.to_account_info(),
                },
                &[signer_seeds],
            ),
            amount,
            ctx.accounts.usdc_mint.decimals,
        )?;

        token_interface::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.vault.to_account_info(),
                destination: ctx.accounts.leecher.to_account_info(),
                authority: ctx.accounts.escrow.to_account_info(),
            },
            &[signer_seeds],
        ))?;

        msg!(
            "Refunded {} USDC base units to leecher {} for torrent {:?}",
            amount,
            ctx.accounts.leecher.key(),
            ctx.accounts.escrow.infohash,
        );
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(infohash: [u8; 20])]
pub struct LockPayment<'info> {
    #[account(mut)]
    pub leecher: Signer<'info>,

    /// CHECK: The seeder's public key — used as a PDA seed and as the
    /// expected payment recipient when the download completes.
    pub seeder: UncheckedAccount<'info>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = usdc_mint,
        associated_token::authority = leecher,
        token::token_program = token_program,
    )]
    pub leecher_usdc_ata: InterfaceAccount<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = leecher,
        space = 8 + PaymentEscrow::INIT_SPACE,
        seeds = [ESCROW_SEED, leecher.key().as_ref(), seeder.key().as_ref(), &infohash],
        bump,
    )]
    pub escrow: Account<'info, PaymentEscrow>,

    #[account(
        init_if_needed,
        payer = leecher,
        seeds = [VAULT_SEED, escrow.key().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = escrow,
        token::token_program = token_program,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
#[instruction(infohash: [u8; 20])]
pub struct ReleasePayment<'info> {
    #[account(mut)]
    pub leecher: Signer<'info>,

    /// CHECK: The seeder receiving the payment upon delivery confirmation.
    #[account(mut)]
    pub seeder: UncheckedAccount<'info>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    #[account(
        init_if_needed,
        payer = leecher,
        associated_token::mint = usdc_mint,
        associated_token::authority = seeder,
        token::token_program = token_program,
    )]
    pub seeder_usdc_ata: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [ESCROW_SEED, leecher.key().as_ref(), seeder.key().as_ref(), &infohash],
        bump = escrow.bump,
        has_one = leecher,
        has_one = seeder,
        constraint = escrow.usdc_mint == usdc_mint.key() @ PaymentError::InvalidMint,
        close = leecher,
    )]
    pub escrow: Account<'info, PaymentEscrow>,

    #[account(
        mut,
        constraint = vault.key() == escrow.vault @ PaymentError::InvalidVault,
        constraint = vault.mint == usdc_mint.key() @ PaymentError::InvalidMint,
        constraint = vault.owner == escrow.key() @ PaymentError::InvalidVaultOwner,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
}

#[derive(Accounts)]
#[instruction(infohash: [u8; 20])]
pub struct ClaimBySecrets<'info> {
    #[account(mut)]
    pub seeder: Signer<'info>,

    /// CHECK: Leecher recipient for any unclaimed funds.
    #[account(mut, address = escrow.leecher)]
    pub leecher: UncheckedAccount<'info>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        constraint = seeder_usdc_ata.owner == seeder.key(),
        constraint = seeder_usdc_ata.mint == usdc_mint.key(),
    )]
    pub seeder_usdc_ata: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = leecher_usdc_ata.owner == leecher.key(),
        constraint = leecher_usdc_ata.mint == usdc_mint.key(),
    )]
    pub leecher_usdc_ata: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [ESCROW_SEED, escrow.leecher.as_ref(), seeder.key().as_ref(), &infohash],
        bump = escrow.bump,
        has_one = seeder,
        constraint = escrow.usdc_mint == usdc_mint.key() @ PaymentError::InvalidMint,
        close = leecher,
    )]
    pub escrow: Account<'info, PaymentEscrow>,

    #[account(
        mut,
        constraint = vault.key() == escrow.vault @ PaymentError::InvalidVault,
        constraint = vault.mint == usdc_mint.key() @ PaymentError::InvalidMint,
        constraint = vault.owner == escrow.key() @ PaymentError::InvalidVaultOwner,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

impl<'info> ClaimBySecrets<'info> {
    pub const MAX_SECRET_CLAIMS: usize = 2048;
}

#[derive(Accounts)]
pub struct Refund<'info> {
    #[account(mut)]
    pub leecher: Signer<'info>,

    #[account(
        mut,
        seeds = [ESCROW_SEED, leecher.key().as_ref(), escrow.seeder.as_ref(), &escrow.infohash],
        bump = escrow.bump,
        has_one = leecher,
        close = leecher,
    )]
    pub escrow: Account<'info, PaymentEscrow>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        constraint = leecher_usdc_ata.owner == leecher.key(),
        constraint = leecher_usdc_ata.mint == usdc_mint.key(),
    )]
    pub leecher_usdc_ata: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = vault.key() == escrow.vault @ PaymentError::InvalidVault,
        constraint = vault.mint == usdc_mint.key() @ PaymentError::InvalidMint,
        constraint = vault.owner == escrow.key() @ PaymentError::InvalidVaultOwner,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

#[account]
#[derive(InitSpace)]
pub struct PaymentEscrow {
    pub leecher: Pubkey,
    pub seeder: Pubkey,
    pub infohash: [u8; 20],
    pub vault: Pubkey,
    pub usdc_mint: Pubkey,
    pub amount: u64,
    pub total_secrets: u32,
    pub merkle_root: [u8; 32],
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct SecretClaim {
    pub index: u32,
    pub secret: [u8; 32],
    pub proof: Vec<[u8; 32]>,
}

fn verify_merkle_proof(
    leaf: [u8; 32],
    proof: &[[u8; 32]],
    mut index: u32,
    root: [u8; 32],
) -> bool {
    let mut computed = leaf;
    for sibling in proof {
        computed = if index % 2 == 0 {
            hashv(&[&computed, sibling]).to_bytes()
        } else {
            hashv(&[sibling, &computed]).to_bytes()
        };
        index /= 2;
    }
    computed == root
}

#[error_code]
pub enum PaymentError {
    #[msg("Payment amount must be greater than zero")]
    ZeroAmount,
    #[msg("Provided mint does not match escrow mint")]
    InvalidMint,
    #[msg("Provided vault does not match escrow vault")]
    InvalidVault,
    #[msg("Vault owner does not match escrow PDA")]
    InvalidVaultOwner,
    #[msg("Total secrets must be greater than zero")]
    InvalidTotalSecrets,
    #[msg("At least one secret claim is required")]
    NoSecretClaims,
    #[msg("Too many secret claims in one transaction")]
    TooManySecretClaims,
}
