//! The pairing AirPlay receivers use when they ask for a PIN (HomeKit
//! Accessory Protocol style): SRP-6a with the PIN, then long-term Ed25519 keys
//! exchanged under ChaCha20-Poly1305 (`pair-setup`); later, a fresh X25519
//! exchange proves both sides still hold those keys (`pair-verify`) and gives
//! the keys of the encrypted session.
//!
//! Everything here is pure: messages in, messages out. The sockets are in
//! [`crate::airplay`]. Written from the published HAP description and the
//! open implementations of the same protocol; the tests check each side
//! against a model of the other, not against a real receiver.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use x25519_dalek::{PublicKey, StaticSecret};

/// Why a pairing step failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairError {
    /// The receiver answered something that is not the expected step.
    Unexpected(&'static str),
    /// The receiver refused: the PIN is wrong.
    WrongPin,
    /// The receiver refused: too many attempts, wait.
    Backoff,
    /// The receiver is busy with another pairing.
    Busy,
    /// A signature, a proof or a tag did not check out.
    Proof(&'static str),
    /// The receiver reported another error code.
    Device(u8),
}

pub mod tlv {
    pub const METHOD: u8 = 0x00;
    pub const IDENTIFIER: u8 = 0x01;
    pub const SALT: u8 = 0x02;
    pub const PUBLIC_KEY: u8 = 0x03;
    pub const PROOF: u8 = 0x04;
    pub const ENCRYPTED: u8 = 0x05;
    pub const STATE: u8 = 0x06;
    pub const ERROR: u8 = 0x07;
    pub const SIGNATURE: u8 = 0x0A;
    pub const FLAGS: u8 = 0x13;

    /// TLV8: values over 255 bytes are split in consecutive items of the same type.
    pub fn encode(items: &[(u8, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (kind, value) in items {
            if value.is_empty() {
                out.extend([*kind, 0]);
            }
            for chunk in value.chunks(255) {
                out.push(*kind);
                out.push(chunk.len() as u8);
                out.extend(chunk);
            }
        }
        out
    }

    /// The items of a TLV8 message; consecutive items of one type are joined.
    pub fn decode(data: &[u8]) -> Option<Vec<(u8, Vec<u8>)>> {
        let mut out: Vec<(u8, Vec<u8>)> = Vec::new();
        let mut rest = data;
        let mut previous_full = false;
        while !rest.is_empty() {
            let (&kind, after) = rest.split_first()?;
            let (&len, after) = after.split_first()?;
            let value = after.get(..len as usize)?;
            match out.last_mut() {
                Some((k, v)) if previous_full && *k == kind => v.extend(value),
                _ => out.push((kind, value.to_vec())),
            }
            previous_full = len == 255;
            rest = &after[len as usize..];
        }
        Some(out)
    }

    pub fn get(items: &[(u8, Vec<u8>)], kind: u8) -> Option<&[u8]> {
        items.iter().find(|(k, _)| *k == kind).map(|(_, v)| v.as_slice())
    }
}

use tlv::{get, ENCRYPTED, ERROR, FLAGS, IDENTIFIER, METHOD, PROOF, PUBLIC_KEY, SALT, SIGNATURE, STATE};

fn sha512(parts: &[&[u8]]) -> [u8; 64] {
    let mut h = Sha512::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

fn hkdf(ikm: &[u8], salt: &str, info: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    Hkdf::<Sha512>::new(Some(salt.as_bytes()), ikm).expand(info.as_bytes(), &mut out).expect("32 bytes is a valid HKDF length");
    out
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the system provides randomness");
    b
}

/// 4 zero bytes then 8 ASCII bytes: the nonce of the pairing messages.
fn tag_nonce(tag: &[u8; 8]) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(tag);
    n
}

fn seal(key: &[u8; 32], nonce: &[u8; 12], plain: &[u8]) -> Vec<u8> {
    ChaCha20Poly1305::new(Key::from_slice(key)).encrypt(Nonce::from_slice(nonce), Payload { msg: plain, aad: b"" }).expect("encryption does not fail")
}

fn open(key: &[u8; 32], nonce: &[u8; 12], sealed: &[u8]) -> Result<Vec<u8>, PairError> {
    ChaCha20Poly1305::new(Key::from_slice(key)).decrypt(Nonce::from_slice(nonce), Payload { msg: sealed, aad: b"" }).map_err(|_| PairError::Proof("a message did not decrypt"))
}

/// The state of a step's answer: its number, or the error the receiver gave.
fn expect_state(items: &[(u8, Vec<u8>)], want: u8) -> Result<(), PairError> {
    if let Some(e) = get(items, ERROR) {
        return Err(match e.first() {
            Some(2) => PairError::WrongPin,
            Some(3) => PairError::Backoff,
            Some(7) => PairError::Busy,
            Some(other) => PairError::Device(*other),
            None => PairError::Unexpected("empty error"),
        });
    }
    match get(items, STATE) {
        Some([s]) if *s == want => Ok(()),
        _ => Err(PairError::Unexpected("wrong step")),
    }
}

fn parse(message: &[u8]) -> Result<Vec<(u8, Vec<u8>)>, PairError> {
    tlv::decode(message).ok_or(PairError::Unexpected("malformed message"))
}

// ---- SRP-6a, HAP flavour: SHA-512, the 3072-bit group, user "Pair-Setup" ----

const N_LEN: usize = 384;

fn pad(n: &BigUint) -> Vec<u8> {
    let b = n.to_bytes_be();
    let mut out = vec![0u8; N_LEN.saturating_sub(b.len())];
    out.extend(b);
    out
}

/// The client side of one SRP exchange, once the receiver's `B` and salt are known.
pub struct SrpSession {
    pub a_pub: Vec<u8>,
    pub m1: [u8; 64],
    /// The session key `K`.
    pub key: [u8; 64],
    m2: [u8; 64],
}

impl std::fmt::Debug for SrpSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SrpSession").finish_non_exhaustive()
    }
}

impl SrpSession {
    pub fn new(seed: [u8; 32], user: &str, password: &str, salt: &[u8], b_pub: &[u8]) -> Result<Self, PairError> {
        let group = &*srp::groups::G_3072;
        let (n, g) = (&group.n, &group.g);
        let b = BigUint::from_bytes_be(b_pub);
        if (&b % n) == BigUint::from(0u8) {
            return Err(PairError::Proof("the receiver's public value is invalid"));
        }
        let a = BigUint::from_bytes_be(&seed);
        let a_pub = g.modpow(&a, n);
        let a_bytes = pad(&a_pub);
        let b_bytes = pad(&b);
        let k = BigUint::from_bytes_be(&sha512(&[&n.to_bytes_be(), &pad(g)]));
        let u = BigUint::from_bytes_be(&sha512(&[&a_bytes, &b_bytes]));
        if u == BigUint::from(0u8) {
            return Err(PairError::Proof("the shared value is invalid"));
        }
        let inner = sha512(&[user.as_bytes(), b":", password.as_bytes()]);
        let x = BigUint::from_bytes_be(&sha512(&[salt, &inner]));
        let gx = g.modpow(&x, n);
        let base = (&b % n + n - (k * &gx) % n) % n;
        let s = base.modpow(&(a + u * x), n);
        let key = sha512(&[&s.to_bytes_be()]);
        let hn = sha512(&[&n.to_bytes_be()]);
        let hg = sha512(&[&g.to_bytes_be()]);
        let xor: Vec<u8> = hn.iter().zip(hg.iter()).map(|(a, b)| a ^ b).collect();
        let m1 = sha512(&[&xor, &sha512(&[user.as_bytes()]), salt, &a_bytes, &b_bytes, &key]);
        let m2 = sha512(&[&a_bytes, &m1, &key]);
        Ok(Self { a_pub: a_bytes, m1, key, m2 })
    }

    /// Whether the receiver's proof shows it knew the PIN too.
    pub fn check_server(&self, m2: &[u8]) -> bool {
        m2.len() == 64 && m2.iter().zip(self.m2.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
    }
}

/// What pairing leaves to keep: enough to prove who we are next time, and who the receiver is.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials {
    pub client_id: String,
    /// Our Ed25519 seed, hex.
    pub client_seed: String,
    pub accessory_id: String,
    /// The receiver's Ed25519 public key, hex.
    pub accessory_key: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("accessory_id", &self.accessory_id).finish_non_exhaustive()
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

impl Credentials {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("credentials serialize")
    }

    pub fn decode(s: &str) -> Option<Self> {
        let c: Self = serde_json::from_str(s).ok()?;
        (unhex::<32>(&c.client_seed).is_some() && unhex::<32>(&c.accessory_key).is_some()).then_some(c)
    }
}

// ---- pair-setup ------------------------------------------------------------

/// The client side of `pair-setup`: three round trips, then credentials.
pub struct PairSetup {
    pin: String,
    seed: [u8; 32],
    client_id: String,
    srp: Option<SrpSession>,
}

impl std::fmt::Debug for PairSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairSetup").finish_non_exhaustive()
    }
}

impl PairSetup {
    pub fn new(pin: &str) -> Self {
        Self { pin: pin.to_owned(), seed: random(), client_id: uuid::Uuid::new_v4().to_string().to_uppercase(), srp: None }
    }

    /// Step 1. `transient` pairs for the session only (no keys kept).
    pub fn m1(&self, transient: bool) -> Vec<u8> {
        if transient {
            tlv::encode(&[(METHOD, &[0]), (STATE, &[1]), (FLAGS, &[0x10])])
        } else {
            tlv::encode(&[(METHOD, &[0]), (STATE, &[1])])
        }
    }

    /// Step 3, from the receiver's step 2.
    pub fn m3(&mut self, m2: &[u8]) -> Result<Vec<u8>, PairError> {
        let items = parse(m2)?;
        expect_state(&items, 2)?;
        let b = get(&items, PUBLIC_KEY).ok_or(PairError::Unexpected("no public key"))?;
        let salt = get(&items, SALT).ok_or(PairError::Unexpected("no salt"))?;
        let srp = SrpSession::new(random(), "Pair-Setup", &self.pin, salt, b)?;
        let out = tlv::encode(&[(STATE, &[3]), (PUBLIC_KEY, &srp.a_pub), (PROOF, &srp.m1)]);
        self.srp = Some(srp);
        Ok(out)
    }

    /// Step 5, from the receiver's step 4: checks its proof, then sends who we are.
    /// `None` when the pairing is transient: nothing more to exchange.
    pub fn m5(&self, m4: &[u8], transient: bool) -> Result<Option<Vec<u8>>, PairError> {
        let items = parse(m4)?;
        expect_state(&items, 4)?;
        let srp = self.srp.as_ref().ok_or(PairError::Unexpected("step 3 not done"))?;
        if !get(&items, PROOF).is_some_and(|p| srp.check_server(p)) {
            return Err(PairError::WrongPin);
        }
        if transient {
            return Ok(None);
        }
        let session = hkdf(&srp.key, "Pair-Setup-Encrypt-Salt", "Pair-Setup-Encrypt-Info");
        let sign_seed = hkdf(&srp.key, "Pair-Setup-Controller-Sign-Salt", "Pair-Setup-Controller-Sign-Info");
        let ltsk = SigningKey::from_bytes(&self.seed);
        let ltpk = ltsk.verifying_key().to_bytes();
        let mut info = sign_seed.to_vec();
        info.extend(self.client_id.as_bytes());
        info.extend(ltpk);
        let sig = ltsk.sign(&info).to_bytes();
        let inner = tlv::encode(&[(IDENTIFIER, self.client_id.as_bytes()), (PUBLIC_KEY, &ltpk), (SIGNATURE, &sig)]);
        let sealed = seal(&session, &tag_nonce(b"PS-Msg05"), &inner);
        Ok(Some(tlv::encode(&[(STATE, &[5]), (ENCRYPTED, &sealed)])))
    }

    /// Step 6: the receiver's identity, checked and kept.
    pub fn finish(&self, m6: &[u8]) -> Result<Credentials, PairError> {
        let items = parse(m6)?;
        expect_state(&items, 6)?;
        let srp = self.srp.as_ref().ok_or(PairError::Unexpected("step 3 not done"))?;
        let session = hkdf(&srp.key, "Pair-Setup-Encrypt-Salt", "Pair-Setup-Encrypt-Info");
        let inner = parse(&open(&session, &tag_nonce(b"PS-Msg06"), get(&items, ENCRYPTED).ok_or(PairError::Unexpected("no data"))?)?)?;
        let id = get(&inner, IDENTIFIER).ok_or(PairError::Unexpected("no identifier"))?;
        let key: [u8; 32] = get(&inner, PUBLIC_KEY).and_then(|k| k.try_into().ok()).ok_or(PairError::Unexpected("no key"))?;
        let sig: [u8; 64] = get(&inner, SIGNATURE).and_then(|s| s.try_into().ok()).ok_or(PairError::Unexpected("no signature"))?;
        let accessory_x = hkdf(&srp.key, "Pair-Setup-Accessory-Sign-Salt", "Pair-Setup-Accessory-Sign-Info");
        let mut info = accessory_x.to_vec();
        info.extend(id);
        info.extend(key);
        VerifyingKey::from_bytes(&key).and_then(|k| k.verify(&info, &Signature::from_bytes(&sig))).map_err(|_| PairError::Proof("the receiver's signature is wrong"))?;
        Ok(Credentials { client_id: self.client_id.clone(), client_seed: hex(&self.seed), accessory_id: String::from_utf8_lossy(id).into_owned(), accessory_key: hex(&key) })
    }

    /// The session key, for a transient pairing.
    pub fn session_key(&self) -> Option<[u8; 64]> {
        self.srp.as_ref().map(|s| s.key)
    }
}

// ---- pair-verify -----------------------------------------------------------

/// The keys of the encrypted session: ours to write with, theirs to read with.
#[derive(Clone)]
pub struct SessionKeys {
    pub write: [u8; 32],
    pub read: [u8; 32],
}

impl std::fmt::Debug for SessionKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionKeys").finish_non_exhaustive()
    }
}

/// The keys of a session from a shared secret.
pub fn session_keys(secret: &[u8]) -> SessionKeys {
    SessionKeys { write: hkdf(secret, "Control-Salt", "Control-Write-Encryption-Key"), read: hkdf(secret, "Control-Salt", "Control-Read-Encryption-Key") }
}

/// The client side of `pair-verify`: two round trips and the session keys.
pub struct PairVerify {
    creds: Credentials,
    secret: StaticSecret,
    public: [u8; 32],
    shared: Option<[u8; 32]>,
}

impl std::fmt::Debug for PairVerify {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairVerify").finish_non_exhaustive()
    }
}

impl PairVerify {
    pub fn new(creds: Credentials) -> Self {
        let secret = StaticSecret::from(random::<32>());
        let public = PublicKey::from(&secret).to_bytes();
        Self { creds, secret, public, shared: None }
    }

    pub fn m1(&self) -> Vec<u8> {
        tlv::encode(&[(STATE, &[1]), (PUBLIC_KEY, &self.public)])
    }

    /// Step 3, from the receiver's step 2: it proves itself, then we do.
    pub fn m3(&mut self, m2: &[u8]) -> Result<Vec<u8>, PairError> {
        let items = parse(m2)?;
        expect_state(&items, 2)?;
        let theirs: [u8; 32] = get(&items, PUBLIC_KEY).and_then(|k| k.try_into().ok()).ok_or(PairError::Unexpected("no public key"))?;
        let shared = self.secret.diffie_hellman(&PublicKey::from(theirs)).to_bytes();
        let key = hkdf(&shared, "Pair-Verify-Encrypt-Salt", "Pair-Verify-Encrypt-Info");
        let inner = parse(&open(&key, &tag_nonce(b"PV-Msg02"), get(&items, ENCRYPTED).ok_or(PairError::Unexpected("no data"))?)?)?;
        let id = get(&inner, IDENTIFIER).ok_or(PairError::Unexpected("no identifier"))?;
        let sig: [u8; 64] = get(&inner, SIGNATURE).and_then(|s| s.try_into().ok()).ok_or(PairError::Unexpected("no signature"))?;
        if id != self.creds.accessory_id.as_bytes() {
            return Err(PairError::Proof("this is not the receiver that was paired"));
        }
        let accessory_key = unhex::<32>(&self.creds.accessory_key).ok_or(PairError::Unexpected("stored key"))?;
        let mut info = theirs.to_vec();
        info.extend(id);
        info.extend(self.public);
        VerifyingKey::from_bytes(&accessory_key).and_then(|k| k.verify(&info, &Signature::from_bytes(&sig))).map_err(|_| PairError::Proof("the receiver's signature is wrong"))?;

        let seed = unhex::<32>(&self.creds.client_seed).ok_or(PairError::Unexpected("stored key"))?;
        let mut mine = self.public.to_vec();
        mine.extend(self.creds.client_id.as_bytes());
        mine.extend(theirs);
        let my_sig = SigningKey::from_bytes(&seed).sign(&mine).to_bytes();
        let inner = tlv::encode(&[(IDENTIFIER, self.creds.client_id.as_bytes()), (SIGNATURE, &my_sig)]);
        let sealed = seal(&key, &tag_nonce(b"PV-Msg03"), &inner);
        self.shared = Some(shared);
        Ok(tlv::encode(&[(STATE, &[3]), (ENCRYPTED, &sealed)]))
    }

    /// Step 4: the receiver accepted us; the session keys.
    pub fn finish(&self, m4: &[u8]) -> Result<SessionKeys, PairError> {
        let items = parse(m4)?;
        expect_state(&items, 4)?;
        Ok(session_keys(&self.shared.ok_or(PairError::Unexpected("step 3 not done"))?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tlv_splits_long_values_and_joins_them_back() {
        let long: Vec<u8> = (0..600u32).map(|i| i as u8).collect();
        let enc = tlv::encode(&[(STATE, &[1]), (PUBLIC_KEY, &long), (SALT, &[])]);
        assert_eq!(enc.len(), 3 + (2 + 255) + (2 + 255) + (2 + 90) + 2);
        let dec = tlv::decode(&enc).unwrap();
        assert_eq!(get(&dec, STATE), Some(&[1u8][..]));
        assert_eq!(get(&dec, PUBLIC_KEY), Some(&long[..]));
        assert_eq!(get(&dec, SALT), Some(&[][..]));
        assert!(tlv::decode(&[3, 5, 1]).is_none(), "a cut item is refused");
    }

    #[test]
    fn credentials_round_trip_and_reject_damage() {
        let c = Credentials { client_id: "A".into(), client_seed: hex(&[7; 32]), accessory_id: "B".into(), accessory_key: hex(&[9; 32]) };
        assert_eq!(Credentials::decode(&c.encode()), Some(c.clone()));
        assert!(Credentials::decode("{\"client_id\":\"A\"}").is_none());
        assert!(!format!("{c:?}").contains(&hex(&[7; 32])), "the seed is never printed");
    }

    // ---- a model of the receiver, to run the client against ----

    struct Accessory {
        pin: String,
        salt: [u8; 16],
        b: BigUint,
        b_pub: BigUint,
        v: BigUint,
        id: String,
        seed: [u8; 32],
        key: Option<[u8; 64]>,
        client_ltpk: Option<([u8; 32], Vec<u8>)>,
    }

    impl Accessory {
        fn new(pin: &str) -> Self {
            let group = &*srp::groups::G_3072;
            let (n, g) = (&group.n, &group.g);
            let salt: [u8; 16] = random();
            let inner = sha512(&[b"Pair-Setup", b":", pin.as_bytes()]);
            let x = BigUint::from_bytes_be(&sha512(&[&salt, &inner]));
            let v = g.modpow(&x, n);
            let b = BigUint::from_bytes_be(&random::<32>());
            let k = BigUint::from_bytes_be(&sha512(&[&n.to_bytes_be(), &pad(g)]));
            let b_pub = (k * &v + g.modpow(&b, n)) % n;
            Self { pin: pin.into(), salt, b, b_pub, v, id: "11:22:33:44:55:66".into(), seed: random(), key: None, client_ltpk: None }
        }

        fn m2(&self) -> Vec<u8> {
            tlv::encode(&[(STATE, &[2]), (PUBLIC_KEY, &pad(&self.b_pub)), (SALT, &self.salt)])
        }

        fn m4(&mut self, m3: &[u8]) -> Vec<u8> {
            let group = &*srp::groups::G_3072;
            let (n, g) = (&group.n, &group.g);
            let items = tlv::decode(m3).unwrap();
            let a_bytes = get(&items, PUBLIC_KEY).unwrap().to_vec();
            let a = BigUint::from_bytes_be(&a_bytes);
            let b_bytes = pad(&self.b_pub);
            let u = BigUint::from_bytes_be(&sha512(&[&a_bytes, &b_bytes]));
            let s = (&a * self.v.modpow(&u, n) % n).modpow(&self.b, n);
            let key = sha512(&[&s.to_bytes_be()]);
            let hn = sha512(&[&n.to_bytes_be()]);
            let hg = sha512(&[&g.to_bytes_be()]);
            let xor: Vec<u8> = hn.iter().zip(hg.iter()).map(|(a, b)| a ^ b).collect();
            let m1 = sha512(&[&xor, &sha512(&[b"Pair-Setup"]), &self.salt, &a_bytes, &b_bytes, &key]);
            if get(&items, PROOF) != Some(&m1[..]) {
                return tlv::encode(&[(STATE, &[4]), (ERROR, &[2])]);
            }
            self.key = Some(key);
            tlv::encode(&[(STATE, &[4]), (PROOF, &sha512(&[&a_bytes, &m1, &key]))])
        }

        fn m6(&mut self, m5: &[u8]) -> Vec<u8> {
            let key = self.key.unwrap();
            let session = hkdf(&key, "Pair-Setup-Encrypt-Salt", "Pair-Setup-Encrypt-Info");
            let items = tlv::decode(m5).unwrap();
            let inner = tlv::decode(&open(&session, &tag_nonce(b"PS-Msg05"), get(&items, ENCRYPTED).unwrap()).unwrap()).unwrap();
            let (id, ltpk, sig) = (get(&inner, IDENTIFIER).unwrap().to_vec(), get(&inner, PUBLIC_KEY).unwrap(), get(&inner, SIGNATURE).unwrap());
            let mut info = hkdf(&key, "Pair-Setup-Controller-Sign-Salt", "Pair-Setup-Controller-Sign-Info").to_vec();
            info.extend(&id);
            info.extend(ltpk);
            let ltpk: [u8; 32] = ltpk.try_into().unwrap();
            VerifyingKey::from_bytes(&ltpk).unwrap().verify(&info, &Signature::from_bytes(sig.try_into().unwrap())).expect("the client's signature checks out");
            self.client_ltpk = Some((ltpk, id));
            let ltsk = SigningKey::from_bytes(&self.seed);
            let mut mine = hkdf(&key, "Pair-Setup-Accessory-Sign-Salt", "Pair-Setup-Accessory-Sign-Info").to_vec();
            mine.extend(self.id.as_bytes());
            mine.extend(ltsk.verifying_key().to_bytes());
            let inner = tlv::encode(&[(IDENTIFIER, self.id.as_bytes()), (PUBLIC_KEY, &ltsk.verifying_key().to_bytes()), (SIGNATURE, &ltsk.sign(&mine).to_bytes())]);
            tlv::encode(&[(STATE, &[6]), (ENCRYPTED, &seal(&session, &tag_nonce(b"PS-Msg06"), &inner))])
        }
    }

    fn paired(pin_typed: &str, pin_shown: &str) -> (Result<Credentials, PairError>, Accessory) {
        let mut tv = Accessory::new(pin_shown);
        let mut client = PairSetup::new(pin_typed);
        let _ = client.m1(false);
        let run = (|| {
            let m3 = client.m3(&tv.m2())?;
            let m4 = tv.m4(&m3);
            let m5 = client.m5(&m4, false)?.expect("not transient");
            client.finish(&tv.m6(&m5))
        })();
        (run, tv)
    }

    #[test]
    fn pair_setup_with_the_right_pin_gives_matching_credentials() {
        let (creds, tv) = paired("1234", "1234");
        let creds = creds.expect("pairing succeeds");
        assert_eq!(creds.accessory_id, tv.id);
        assert_eq!(creds.accessory_key, hex(&SigningKey::from_bytes(&tv.seed).verifying_key().to_bytes()));
        let (ltpk, id) = tv.client_ltpk.clone().unwrap();
        assert_eq!(hex(&ltpk), hex(&SigningKey::from_bytes(&unhex(&creds.client_seed).unwrap()).verifying_key().to_bytes()));
        assert_eq!(id, creds.client_id.as_bytes());
        assert_eq!(tv.pin, "1234");
    }

    #[test]
    fn a_wrong_pin_is_reported_as_one() {
        let (creds, _) = paired("0000", "1234");
        assert_eq!(creds.unwrap_err(), PairError::WrongPin);
    }

    #[test]
    fn the_receiver_can_refuse_with_backoff_or_busy() {
        let mut client = PairSetup::new("1234");
        let busy = tlv::encode(&[(STATE, &[2]), (ERROR, &[7])]);
        assert_eq!(client.m3(&busy).unwrap_err(), PairError::Busy);
        let backoff = tlv::encode(&[(STATE, &[2]), (ERROR, &[3])]);
        assert_eq!(client.m3(&backoff).unwrap_err(), PairError::Backoff);
        assert_eq!(client.m3(&tlv::encode(&[(STATE, &[4])])).unwrap_err(), PairError::Unexpected("wrong step"));
    }

    /// The receiver's half of pair-verify.
    fn verify_with(creds: &Credentials, tv_seed: [u8; 32], tv_id: &str) -> Result<(SessionKeys, SessionKeys), PairError> {
        let mut client = PairVerify::new(creds.clone());
        let m1 = tlv::decode(&client.m1()).unwrap();
        let theirs_pub: [u8; 32] = get(&m1, PUBLIC_KEY).unwrap().try_into().unwrap();
        let secret = StaticSecret::from(random::<32>());
        let public = PublicKey::from(&secret).to_bytes();
        let shared = secret.diffie_hellman(&PublicKey::from(theirs_pub)).to_bytes();
        let key = hkdf(&shared, "Pair-Verify-Encrypt-Salt", "Pair-Verify-Encrypt-Info");
        let ltsk = SigningKey::from_bytes(&tv_seed);
        let mut info = public.to_vec();
        info.extend(tv_id.as_bytes());
        info.extend(theirs_pub);
        let inner = tlv::encode(&[(IDENTIFIER, tv_id.as_bytes()), (SIGNATURE, &ltsk.sign(&info).to_bytes())]);
        let m2 = tlv::encode(&[(STATE, &[2]), (PUBLIC_KEY, &public), (ENCRYPTED, &seal(&key, &tag_nonce(b"PV-Msg02"), &inner))]);
        let m3 = client.m3(&m2)?;
        // The receiver checks the client's proof.
        let items = tlv::decode(&m3).unwrap();
        let inner = tlv::decode(&open(&key, &tag_nonce(b"PV-Msg03"), get(&items, ENCRYPTED).unwrap()).unwrap()).unwrap();
        let mut info = theirs_pub.to_vec();
        info.extend(get(&inner, IDENTIFIER).unwrap());
        info.extend(public);
        let client_key = VerifyingKey::from_bytes(&unhex(&hex(&SigningKey::from_bytes(&unhex(&creds.client_seed).unwrap()).verifying_key().to_bytes())).unwrap()).unwrap();
        client_key.verify(&info, &Signature::from_bytes(get(&inner, SIGNATURE).unwrap().try_into().unwrap())).expect("the client proves itself");
        let keys = client.finish(&tlv::encode(&[(STATE, &[4])]))?;
        Ok((keys, session_keys(&shared)))
    }

    #[test]
    fn pair_verify_agrees_on_crossed_session_keys() {
        let (creds, tv) = paired("1234", "1234");
        let creds = creds.unwrap();
        let (ours, theirs) = verify_with(&creds, tv.seed, &tv.id).expect("verify succeeds");
        // What we write is what the receiver reads, and the other way round.
        assert_eq!(ours.write, theirs.write);
        assert_eq!(ours.read, theirs.read);
        assert_ne!(ours.write, ours.read);
    }

    #[test]
    fn pair_verify_refuses_another_receiver() {
        let (creds, tv) = paired("1234", "1234");
        let creds = creds.unwrap();
        assert_eq!(verify_with(&creds, random(), &tv.id).unwrap_err(), PairError::Proof("the receiver's signature is wrong"));
        assert_eq!(verify_with(&creds, tv.seed, "AA:BB:CC:DD:EE:FF").unwrap_err(), PairError::Proof("this is not the receiver that was paired"));
    }
}
