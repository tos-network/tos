use crate::{
    account::{CiphertextCache, Nonce},
    crypto::{elgamal::Ciphertext, Hash},
    transaction::Reference,
};

use super::FeeHelper;

/// If the returned balance and ct do not match, the build function will panic and/or
/// the proof will be invalid.
pub trait AccountState: FeeHelper {
    /// Used to verify if the address is on the same chain
    fn is_mainnet(&self) -> bool;

    /// Get the balance from the source
    fn get_account_balance(&self, asset: &Hash) -> Result<u64, Self::Error>;

    /// Block topoheight at which the transaction is being built
    fn get_reference(&self) -> Reference;

    /// Update the balance
    fn update_account_balance(&mut self, asset: &Hash, new_balance: u64)
        -> Result<(), Self::Error>;

    /// Get the nonce of the account
    fn get_nonce(&self) -> Result<Nonce, Self::Error>;

    /// Update account nonce
    fn update_nonce(&mut self, new_nonce: Nonce) -> Result<(), Self::Error>;

    /// Check if an account is registered (exists) on the blockchain
    /// This is used to validate Energy fee restrictions for new addresses
    fn is_account_registered(&self, key: &crate::crypto::PublicKey) -> Result<bool, Self::Error>;
}

/// Extended trait for UNO (privacy-preserving) transactions
/// Provides access to encrypted balance ciphertexts required for ZK proof generation
pub trait UnoAccountState: AccountState {
    /// Get the UNO balance ciphertext for a given asset
    /// Required for generating CommitmentEqProof (proving new balance equals old - spent)
    fn get_uno_ciphertext(&self, asset: &Hash) -> Result<CiphertextCache, Self::Error>;

    /// Get the plaintext UNO balance amount (decrypted locally by wallet)
    fn get_uno_balance(&self, asset: &Hash) -> Result<u64, Self::Error>;

    /// Update the UNO balance after a transfer
    /// Both the plaintext amount and ciphertext must be updated
    fn update_uno_balance(
        &mut self,
        asset: &Hash,
        new_balance: u64,
        new_ciphertext: Ciphertext,
    ) -> Result<(), Self::Error>;
}
