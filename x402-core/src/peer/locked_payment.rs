use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::Signer;
use anchor_client::Program;
use anchor_lang::prelude::*;
use anchor_spl::associated_token::{self, get_associated_token_address_with_program_id};
use anchor_spl::token::ID;
use std::net::TcpStream;
use std::ops::Deref;
use std::str::FromStr;

declare_program!(x402_contract);
use x402_contract::{client::accounts, client::args};

use crate::{write_message, X402Message, X402MessageId};

pub struct LockedPayment<'a> {
    pub leecher_pubkey: Pubkey,
    pub seeder_pubkey: Pubkey,
    pub info_hash: &'a [u8; 20],
    pub merkle_root: [u8; 32],
}

impl<'a> LockedPayment<'a> {
    pub fn new(
        leecher_pubkey: Pubkey,
        seeder_pubkey: Pubkey,
        info_hash: &'a [u8; 20],
        merkle_root: [u8; 32],
    ) -> Self {
        Self {
            leecher_pubkey,
            seeder_pubkey,
            info_hash,
            merkle_root,
        }
    }

    pub fn lock_payment<C, S>(
        &self,
        stream: &mut TcpStream,
        amount: u64,
        program: &Program<C>,
    ) -> ::std::result::Result<(), String>
    where
        C: Deref<Target = S> + Clone,
        S: Signer,
    {
        let usdc_mint = Pubkey::from_str("Gh9ZwEmdLJ8DscKNTkTqPbNwLNNBjuSzaG9Vp2KGtKJr")
            .map_err(|e| e.to_string())?;

        // Convert anchor_lang::prelude::Pubkey to spl_pod::solana_pubkey::Pubkey
        let leecher_pubkey_bytes = self.leecher_pubkey.to_bytes();
        let leecher_pubkey_spl =
            anchor_spl::token_2022_extensions::spl_pod::solana_pubkey::Pubkey::new_from_array(
                leecher_pubkey_bytes,
            );

        let usdc_mint_bytes = usdc_mint.to_bytes();
        let usdc_mint_spl =
            anchor_spl::token_2022_extensions::spl_pod::solana_pubkey::Pubkey::new_from_array(
                usdc_mint_bytes,
            );

        // derive leecher payment account
        let leecher_usdc_ata_spl =
            get_associated_token_address_with_program_id(&leecher_pubkey_spl, &usdc_mint_spl, &ID);

        // Convert spl_pod::solana_pubkey::Pubkey to anchor_lang::prelude::Pubkey
        let leecher_usdc_ata = Pubkey::new_from_array(leecher_usdc_ata_spl.to_bytes());

        // Derive escrow PDA
        let (escrow, _) = Pubkey::find_program_address(
            &[
                b"escrow",
                self.leecher_pubkey.as_ref(),
                self.seeder_pubkey.as_ref(),
                self.info_hash,
            ],
            &x402_contract::ID,
        );

        // Derive vault PDA (seeded from the escrow PDA)
        let (vault, _) =
            Pubkey::find_program_address(&[b"vault", escrow.as_ref()], &x402_contract::ID);
        let associated_token_program =
            Pubkey::from_str(&associated_token::ID.to_string()).map_err(|e| e.to_string())?;
        let token_program = Pubkey::from_str(&ID.to_string()).map_err(|e| e.to_string())?;
        let system_program =
            Pubkey::from_str("11111111111111111111111111111111").map_err(|e| e.to_string())?;

        let signature = program
            .request()
            .accounts(accounts::LockPayment {
                leecher: self.leecher_pubkey,
                seeder: self.seeder_pubkey,
                usdc_mint,
                leecher_usdc_ata,
                escrow,
                vault,
                associated_token_program,
                system_program,
                token_program,
            })
            .args(args::LockPayment {
                infohash: *self.info_hash,
                amount,
                merkle_root: self.merkle_root,
            })
            .send()
            .map_err(|e| e.to_string())?;
        println!("LockPayment transaction sent with signature: {}", signature);

        let payload = self.merkle_root.to_vec();

        let message = X402Message::new(X402MessageId::LockedPayment, payload);

        if let Err(e) = write_message(stream, &message) {
            println!("Failed to send LockedPayment message: {}", e);
            return Err(e.to_string());
        }

        Ok(())
    }
}
