//! Downloaded files are encrypted on disk, so a copy taken out of Flick's
//! folder is not a playable video.
//!
//! AES-256 in counter mode, keyed per download: the keystream position is the
//! byte offset in the file, so a resumed download and a seek during playback
//! cost nothing, and the file is exactly as long as the video.
//!
//! This keeps casual copies out; it is not copy protection. The key has to be
//! on the machine for Flick to play the file.

use aes::Aes256;
use ctr::Ctr128BE;
use ctr::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use hkdf::Hkdf;
use sha2::Sha256;

type Cipher = Ctr128BE<Aes256>;

/// The key and counter start of one download.
#[derive(Clone)]
pub struct Seal {
    key: [u8; 32],
    iv: [u8; 16],
}

impl std::fmt::Debug for Seal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Seal").finish_non_exhaustive()
    }
}

impl Seal {
    /// The download's own key and counter, derived from the app's master key and its id.
    pub fn derive(master: &[u8; 32], download_id: &str) -> Self {
        let mut okm = [0u8; 48];
        Hkdf::<Sha256>::new(None, master)
            .expand(format!("flick/download/v1/{download_id}").as_bytes(), &mut okm)
            .expect("48 bytes is a valid HKDF-SHA256 length");
        let mut key = [0u8; 32];
        let mut iv = [0u8; 16];
        key.copy_from_slice(&okm[..32]);
        iv.copy_from_slice(&okm[32..]);
        Self { key, iv }
    }

    /// Encrypts or decrypts `data`, which sits at `offset` in the file.
    pub fn apply(&self, offset: u64, data: &mut [u8]) {
        let mut cipher = Cipher::new(&self.key.into(), &self.iv.into());
        cipher.seek(offset);
        cipher.apply_keystream(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_hides_the_content() {
        let seal = Seal::derive(&[7; 32], "a");
        let plain: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let mut data = plain.clone();
        seal.apply(0, &mut data);
        assert_ne!(data, plain);
        seal.apply(0, &mut data);
        assert_eq!(data, plain);
    }

    #[test]
    fn any_slice_decrypts_on_its_own() {
        let seal = Seal::derive(&[7; 32], "a");
        let plain: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let mut whole = plain.clone();
        seal.apply(0, &mut whole);
        // 37 is not a multiple of the 16-byte block.
        let mut part = whole[37..300].to_vec();
        seal.apply(37, &mut part);
        assert_eq!(part, plain[37..300]);
    }

    #[test]
    fn each_download_and_each_master_has_its_own_stream() {
        let mut a = vec![0u8; 32];
        let mut b = vec![0u8; 32];
        let mut c = vec![0u8; 32];
        Seal::derive(&[7; 32], "a").apply(0, &mut a);
        Seal::derive(&[7; 32], "b").apply(0, &mut b);
        Seal::derive(&[8; 32], "a").apply(0, &mut c);
        assert!(a != b && a != c);
    }
}
