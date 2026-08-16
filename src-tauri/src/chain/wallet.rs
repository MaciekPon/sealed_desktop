//! Algorand wallet — BIP39 mnemonic + Ed25519 signing key, ported from
//! `algorand_wallet_client.dart`.
//!
//! New wallets use a 24-word BIP39 phrase (256 bits of entropy), matching
//! the mobile app's current default. The legacy 25-word Algorand-native
//! mnemonic restore path (for pre-BIP39 mobile installs) is *not* ported —
//! it uses a different word list and checksum scheme than BIP39 and is a
//! restore-only affordance for very old installs; flagged as a follow-up
//! rather than silently unsupported.

use bip39::Mnemonic;
use ed25519_dalek::{Signer, SigningKey};
use thiserror::Error;

use super::address;

#[derive(Debug, Error)]
pub enum WalletError {
    #[error("invalid mnemonic: {0}")]
    InvalidMnemonic(String),
    #[error(
        "25-word Algorand-native mnemonics are not supported by this port yet \
         (BIP39 12/24-word phrases only) — restore path for pre-BIP39 mobile \
         installs is a follow-up"
    )]
    LegacyAlgorandMnemonicUnsupported,
}

pub struct AlgorandWallet {
    signing_key: SigningKey,
    mnemonic: String,
    pub address: String,
}

// Content-free Debug impl (SigningKey deliberately doesn't implement Debug,
// to avoid secret material leaking into logs) — only needed so
// `Result<AlgorandWallet, _>::unwrap_err()` type-checks in tests.
impl std::fmt::Debug for AlgorandWallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlgorandWallet")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl AlgorandWallet {
    /// Generate a brand-new wallet: 24-word BIP39 mnemonic (256 bits of
    /// entropy), matching `AlgorandWallet.createWallet`.
    pub fn create() -> Self {
        let mnemonic = Mnemonic::generate(24).expect("24 is a valid BIP39 word count");
        Self::from_mnemonic_words(mnemonic)
    }

    /// Restore a wallet from an existing recovery phrase. Accepts 12 or
    /// 24-word BIP39 phrases (the legacy 12-word Sealed format and the
    /// current 24-word default) — both are fed through
    /// `Mnemonic::to_seed` and the first 32 bytes taken, preserving
    /// on-chain addresses for existing wallets. See [`WalletError::LegacyAlgorandMnemonicUnsupported`]
    /// for the not-yet-ported 25-word path.
    pub fn restore(phrase: &str) -> Result<Self, WalletError> {
        let word_count = phrase.split_whitespace().count();
        if word_count == 25 {
            return Err(WalletError::LegacyAlgorandMnemonicUnsupported);
        }
        let mnemonic = Mnemonic::parse(phrase).map_err(|e| WalletError::InvalidMnemonic(e.to_string()))?;
        Ok(Self::from_mnemonic_words(mnemonic))
    }

    fn from_mnemonic_words(mnemonic: Mnemonic) -> Self {
        let seed64 = mnemonic.to_seed("");
        let mut seed32 = [0u8; 32];
        seed32.copy_from_slice(&seed64[..32]);

        let signing_key = SigningKey::from_bytes(&seed32);
        let pubkey = *signing_key.verifying_key().as_bytes();
        let addr = address::encode_address(&pubkey);

        Self {
            signing_key,
            mnemonic: mnemonic.to_string(),
            address: addr,
        }
    }

    pub fn mnemonic(&self) -> &str {
        &self.mnemonic
    }

    pub fn public_key_bytes(&self) -> [u8; 32] {
        *self.signing_key.verifying_key().as_bytes()
    }

    /// The raw 32-byte seed backing this wallet's Ed25519 key. Also used
    /// (via HKDF, in a later phase) to derive the app's X25519 encryption
    /// and view keys and the ML-KEM seed, exactly as `KeyService` does.
    pub fn seed_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// Raw 64-byte Ed25519 signature (not sig+message), matching
    /// `signTransactionBytes` / `signMessage`.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing_key.sign(message).to_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_restore_round_trip() {
        let created = AlgorandWallet::create();
        assert_eq!(created.mnemonic().split_whitespace().count(), 24);

        let restored = AlgorandWallet::restore(created.mnemonic()).unwrap();
        assert_eq!(restored.address, created.address);
        assert_eq!(restored.public_key_bytes(), created.public_key_bytes());
    }

    #[test]
    fn address_is_58_chars_of_valid_base32() {
        let wallet = AlgorandWallet::create();
        assert_eq!(wallet.address.len(), 58);
        assert!(wallet.address.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
    }

    #[test]
    fn sign_produces_64_byte_signature() {
        let wallet = AlgorandWallet::create();
        let sig = wallet.sign(b"hello sealed");
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn different_mnemonics_give_different_addresses() {
        let a = AlgorandWallet::create();
        let b = AlgorandWallet::create();
        assert_ne!(a.address, b.address);
    }

    #[test]
    fn twenty_five_word_mnemonic_reports_unsupported() {
        let fake_25_words = "abandon ".repeat(25);
        let err = AlgorandWallet::restore(fake_25_words.trim()).unwrap_err();
        assert!(matches!(err, WalletError::LegacyAlgorandMnemonicUnsupported));
    }
}
