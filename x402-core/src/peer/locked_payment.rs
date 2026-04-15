use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{Keypair, Signer};
use anchor_client::{Client, Cluster, Program};
use anchor_lang::{prelude::*, AnchorSerialize, Discriminator, InstructionData};
use anchor_spl::associated_token::{self, get_associated_token_address_with_program_id};
use anchor_spl::token::ID;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::CommitmentConfig;
use solana_client::rpc_request::Address;
use std::net::TcpStream;
use std::ops::Deref;
use std::str::FromStr;

declare_program!(x402_contract);
use x402_contract::client::accounts;

use crate::{read_message, write_message, X402Message, X402MessageId};

pub struct LockedPayment<'a> {
    pub leecher_pubkey: Pubkey,
    pub seeder_pubkey: Pubkey,
    pub info_hash: &'a [u8; 20],
    pub merkle_root: [u8; 32],
}

#[derive(Clone, AnchorSerialize)]
pub struct SecretClaimInput {
    pub index: u32,
    pub secret: [u8; 32],
    pub proof: Vec<[u8; 32]>,
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
        total_secrets: u32,
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
                b"escrow_v2",
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

        program
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
            .args(LockPaymentIxArgs {
                infohash: *self.info_hash,
                amount,
                merkle_root: self.merkle_root,
                total_secrets,
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
        total_secrets: u32,
        program: &Program<C>,
    ) -> ::std::result::Result<(), String>
    where
        C: Deref<Target = S> + Clone,
        S: Signer,
    {
        self.submit_onchain(amount, total_secrets, program)?;
        self.send_locked_payment_message(stream)
    }

    pub fn receive_locked_payment(
        message: &X402Message,
        leecher_pubkey: &Pubkey,
        seeder_pubkey: &Pubkey,
        info_hash: &'a [u8; 20],
        expected_amount: u64,
        expected_total_secrets: u32,
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
                b"escrow_v2",
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

        let escrow_account = client.get_account(&scrow_address).map_err(|e| {
            println!("Failed to fetch escrow account: {}", e);
            e
        })?;

        // Anchor account layout:
        // discriminator(8) + leecher(32) + seeder(32) + infohash(20)
        // + vault(32) + usdc_mint(32) + amount(8) + total_secrets(4)
        // + merkle_root(32) + bump(1)
        let data = escrow_account.data;
        if data.len() < 201 {
            return Err("Escrow account data is too short".into());
        }

        let total_secrets_offset = 164usize;
        let merkle_root_offset = 168usize;

        let mut onchain_total_secrets_bytes = [0u8; 4];
        onchain_total_secrets_bytes
            .copy_from_slice(&data[total_secrets_offset..total_secrets_offset + 4]);
        let onchain_total_secrets = u32::from_le_bytes(onchain_total_secrets_bytes);

        if onchain_total_secrets != expected_total_secrets {
            println!(
                "Escrow total_secrets mismatch: expected {}, got {}",
                expected_total_secrets, onchain_total_secrets
            );
            return Err("Escrow total_secrets mismatch".into());
        }

        let mut onchain_merkle_root = [0u8; 32];
        onchain_merkle_root
            .copy_from_slice(&data[merkle_root_offset..merkle_root_offset + 32]);
        if onchain_merkle_root != merkle_root {
            return Err("Escrow merkle_root mismatch".into());
        }

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

    pub fn claim_by_secrets_onchain<C, S>(
        &self,
        claims: Vec<SecretClaimInput>,
        program: &Program<C>,
    ) -> ::std::result::Result<String, String>
    where
        C: Deref<Target = S> + Clone,
        S: Signer,
    {
        let usdc_mint = Pubkey::from_str("Gh9ZwEmdLJ8DscKNTkTqPbNwLNNBjuSzaG9Vp2KGtKJr")
            .map_err(|e| e.to_string())?;

        let leecher_pubkey_bytes = self.leecher_pubkey.to_bytes();
        let leecher_pubkey_spl =
            anchor_spl::token_2022_extensions::spl_pod::solana_pubkey::Pubkey::new_from_array(
                leecher_pubkey_bytes,
            );

        let seeder_pubkey_bytes = self.seeder_pubkey.to_bytes();
        let seeder_pubkey_spl =
            anchor_spl::token_2022_extensions::spl_pod::solana_pubkey::Pubkey::new_from_array(
                seeder_pubkey_bytes,
            );

        let usdc_mint_bytes = usdc_mint.to_bytes();
        let usdc_mint_spl =
            anchor_spl::token_2022_extensions::spl_pod::solana_pubkey::Pubkey::new_from_array(
                usdc_mint_bytes,
            );

        let seeder_usdc_ata_spl =
            get_associated_token_address_with_program_id(&seeder_pubkey_spl, &usdc_mint_spl, &ID);
        let seeder_usdc_ata = Pubkey::new_from_array(seeder_usdc_ata_spl.to_bytes());

        let leecher_usdc_ata_spl = get_associated_token_address_with_program_id(
            &leecher_pubkey_spl,
            &usdc_mint_spl,
            &ID,
        );
        let leecher_usdc_ata = Pubkey::new_from_array(leecher_usdc_ata_spl.to_bytes());

        let (escrow, _) = Pubkey::find_program_address(
            &[
                b"escrow_v2",
                self.leecher_pubkey.as_ref(),
                self.seeder_pubkey.as_ref(),
                self.info_hash,
            ],
            &x402_contract::ID,
        );

        let (vault, _) =
            Pubkey::find_program_address(&[b"vault", escrow.as_ref()], &x402_contract::ID);
        let token_program = Pubkey::from_str(&ID.to_string()).map_err(|e| e.to_string())?;

        let signature = program
            .request()
            .accounts(accounts::ClaimBySecrets {
                seeder: self.seeder_pubkey,
                leecher: self.leecher_pubkey,
                usdc_mint,
                seeder_usdc_ata,
                leecher_usdc_ata,
                escrow,
                vault,
                token_program,
            })
            .args(ClaimBySecretsIxArgs {
                infohash: *self.info_hash,
                claims,
            })
            .send()
            .map_err(|e| e.to_string())?;

        Ok(signature.to_string())
    }

    pub fn claim_by_secrets_onchain_devnet(
        &self,
        claims: Vec<SecretClaimInput>,
        seeder_keypair: &Keypair,
    ) -> ::std::result::Result<String, String> {
        let anchor_client = Client::new(Cluster::Devnet, seeder_keypair);
        let program = anchor_client
            .program(x402_contract::ID)
            .map_err(|e| format!("Failed to create Anchor program client: {}", e))?;

        self.claim_by_secrets_onchain(claims, &program)
    }
}

#[derive(AnchorSerialize)]
struct LockPaymentIxArgs {
    infohash: [u8; 20],
    amount: u64,
    merkle_root: [u8; 32],
    total_secrets: u32,
}

impl InstructionData for LockPaymentIxArgs {
    fn data(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(8 + 20 + 8 + 32 + 4);
        // Anchor instruction discriminator for "global:lock_payment"
        data.extend_from_slice(&[170, 21, 188, 226, 187, 242, 186, 104]);
        data.extend_from_slice(&self.try_to_vec().expect("serialize lock_payment args"));
        data
    }
}

impl Discriminator for LockPaymentIxArgs {
    const DISCRIMINATOR: &'static [u8] = &[170, 21, 188, 226, 187, 242, 186, 104];
}

#[derive(AnchorSerialize)]
struct ClaimBySecretsIxArgs {
    infohash: [u8; 20],
    claims: Vec<SecretClaimInput>,
}

impl InstructionData for ClaimBySecretsIxArgs {
    fn data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        // Anchor instruction discriminator for "global:claim_by_secrets"
        data.extend_from_slice(&[33, 242, 232, 207, 180, 126, 183, 213]);
        data.extend_from_slice(&self.try_to_vec().expect("serialize claim_by_secrets args"));
        data
    }
}

impl Discriminator for ClaimBySecretsIxArgs {
    const DISCRIMINATOR: &'static [u8] = &[33, 242, 232, 207, 180, 126, 183, 213];
}
