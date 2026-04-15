use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::Signer;
use anchor_client::Program;
use anchor_lang::prelude::*;
use anchor_spl::associated_token::{self, get_associated_token_address_with_program_id};
use anchor_spl::token::ID;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::CommitmentConfig;
use solana_client::rpc_request::Address;
use std::net::TcpStream;
use std::ops::Deref;
use std::str::FromStr;

declare_program!(x402_contract);
use x402_contract::{client::accounts, client::args};

use crate::{read_message, write_message, X402Message, X402MessageId};

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

    pub fn submit_onchain<C, S>(
        &self,
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

        Ok(())
    }

    pub fn send_locked_payment_message(
        &self,
        stream: &mut TcpStream,
    ) -> ::std::result::Result<(), String> {
        let payload = self.merkle_root.to_vec();

        let message = X402Message::new(X402MessageId::LockedPayment, payload);

        if let Err(e) = write_message(stream, &message) {
            println!("Failed to send LockedPayment message: {}", e);
            return Err(e.to_string());
        }

        Ok(())
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
        self.submit_onchain(amount, program)?;
        self.send_locked_payment_message(stream)
    }

    pub fn receive_locked_payment(
        message: &X402Message,
        leecher_pubkey: &Pubkey,
        seeder_pubkey: &Pubkey,
        info_hash: &'a [u8; 20],
        expected_amount: u64,
    ) -> ::std::result::Result<[u8; 32], Box<dyn ::std::error::Error>> {
        if message.payload.len() != 32 {
            println!(
                "Invalid LockedPayment payload length: expected 32 bytes, got {}",
                message.payload.len()
            );
            return Err("Invalid LockedPayment payload length".into());
        }

        let mut merkle_root = [0u8; 32];
        merkle_root.copy_from_slice(&message.payload);

        // verify existence of locked payment on-chain using the merkle root
        //
        let client = RpcClient::new_with_commitment(
            String::from("https://api.devnet.solana.com"),
            CommitmentConfig::confirmed(),
        );

        let (scrow_account, _) = Pubkey::find_program_address(
            &[
                b"escrow",
                leecher_pubkey.as_ref(),
                seeder_pubkey.as_ref(),
                info_hash.as_ref(),
            ],
            &x402_contract::ID,
        );
        let scrow_address = scrow_account
            .to_string()
            .parse()
            .map_err(|e| format!("Invalid escrow address conversion: {}", e))?;

        client.get_account(&scrow_address).map_err(|e| {
            println!("Failed to fetch escrow account: {}", e);
            e
        })?;

        // verify vault account exists and has the correct amount of USDC
        let (vault_account, _) =
            Pubkey::find_program_address(&[b"vault", scrow_account.as_ref()], &x402_contract::ID);

        let vault_address: Address = vault_account
            .to_string()
            .parse()
            .map_err(|e| format!("Invalid vault address conversion: {}", e))?;

        let token_account_balance =
            client
                .get_token_account_balance(&vault_address)
                .map_err(|e| {
                    println!("Failed to fetch vault account balance: {}", e);
                    e
                })?;

        if token_account_balance.amount.parse::<u64>().map_err(|e| {
            println!("Failed to parse vault account balance: {}", e);
            e
        })? < expected_amount
        {
            println!(
                "Vault account has insufficient balance: expected at least {}, got {}",
                expected_amount, token_account_balance.amount
            );
            return Err("Vault account has insufficient balance".into());
        }

        Ok(merkle_root)
    }

    pub fn send_payment_ack(stream: &mut TcpStream) {
        // Send an empty PaymentAck message as payment acknowledgment
        let message = X402Message::new(X402MessageId::PaymentAck, vec![]);
        if let Err(e) = write_message(stream, &message) {
            println!("Failed to send PaymentAck message: {}", e);
        }
    }

    pub fn receive_payment_ack(
        stream: &mut TcpStream,
    ) -> ::std::result::Result<(), Box<dyn ::std::error::Error>> {
        let ack_message = read_message(stream)?;
        if ack_message.id != X402MessageId::PaymentAck {
            println!(
                "Expected PaymentAck message, but received message with ID: {:?}",
                ack_message.id
            );
            return Err("Expected PaymentAck message".into());
        }

        Ok(())
    }
}
