use anchor_lang::prelude::*;

declare_id!("CecHZhrZPyLZYFu1R3msJJEQeRDis83K3i99sRXydft3");

#[program]
pub mod x402_contract {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
