//! Multi-mode operator vault.
//!
//! | Mode | Unlock factors |
//! |------|----------------|
//! | **passkey** (default) | iCloud/device WebAuthn passkey |
//! | **password** | password only |
//! | **keyphrase** | 24-word phrase only |
//! | **combo** | password → passkey → keyphrase |
//! | **nocrypt** | plaintext operator.key (0600) |
//!
//! Legacy **master** vaults (password + passkey + phrase + off-node master key)
//! can still unlock via `grid auth login`. New `grid auth master` setup is
//! removed — it was never required for genesis authority or the mesh.
//!
//! Phase 2: policy may move to consensus; this vault stays local key hygiene.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use bip39::{Language, Mnemonic};
use zeroize::Zeroize;

use super::ceremony;
use super::store as pstore;

const SESSION_MAX_SECS: u64 = 8 * 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Passkey,
    Password,
    Keyphrase,
    Combo,
    Master,
    Nocrypt,
}

impl AuthMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passkey => "passkey",
            Self::Password => "password",
            Self::Keyphrase => "keyphrase",
            Self::Combo => "combo",
            Self::Master => "master",
            Self::Nocrypt => "nocrypt",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "" | "passkey" | "pk" | "default" => Ok(Self::Passkey),
            "password" | "pw" => Ok(Self::Password),
            "keyphrase" | "phrase" | "mnemonic" | "24" => Ok(Self::Keyphrase),
            "combo" => Ok(Self::Combo),
            "master" | "full" => Ok(Self::Master),
            "nocrypt" | "none" | "plain" => Ok(Self::Nocrypt),
            o => bail!("unknown auth mode '{o}'"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct VaultMeta {
    mode: String,
    encrypted: bool,
    algorithm: String,
    created_at: String,
    public_key_hex: String,
    #[serde(default)]
    kdf_salt_hex: Option<String>,
    /// True when master key was randomized and destroyed from this node.
    #[serde(default)]
    master_destroyed: bool,
    #[serde(default)]
    factors: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionFile {
    unlocked_at_unix: u64,
    dek_hex: String,
}

pub struct AuthStatus {
    pub mode: String,
    pub passkey_registered: bool,
    pub keys_encrypted: bool,
    pub session_unlocked: bool,
    pub master_destroyed: bool,
    pub public_key_hex: Option<String>,
    pub detail: String,
}

fn keys_dir(c: &Path) -> PathBuf {
    c.join("keys")
}
fn p_enc(c: &Path) -> PathBuf {
    keys_dir(c).join("operator.enc")
}
fn p_raw(c: &Path) -> PathBuf {
    keys_dir(c).join("operator.key")
}
fn p_seal(c: &Path) -> PathBuf {
    keys_dir(c).join("seal.key")
}
fn p_wrap(c: &Path) -> PathBuf {
    keys_dir(c).join("dek.wrap")
}
/// sealed DEK share for multi-factor XOR schemes
fn p_sealed(c: &Path) -> PathBuf {
    keys_dir(c).join("dek.sealed")
}
fn p_meta(c: &Path) -> PathBuf {
    keys_dir(c).join("vault.meta.json")
}
fn p_pub(c: &Path) -> PathBuf {
    keys_dir(c).join("operator.pub")
}
fn p_session(c: &Path) -> PathBuf {
    c.join("session").join("unlocked.json")
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(unix)]
fn write_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn aes_encrypt(key: &[u8; 32], pt: &[u8]) -> Result<Vec<u8>> {
    let c = Aes256Gcm::new_from_slice(key).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut n = [0u8; 12];
    OsRng.fill_bytes(&mut n);
    let ct = c
        .encrypt(Nonce::from_slice(&n), pt)
        .map_err(|e| anyhow::anyhow!("encrypt: {e}"))?;
    let mut o = n.to_vec();
    o.extend_from_slice(&ct);
    Ok(o)
}

fn aes_decrypt(key: &[u8; 32], blob: &[u8]) -> Result<Vec<u8>> {
    if blob.len() < 28 {
        bail!("ciphertext too short");
    }
    let (n, ct) = blob.split_at(12);
    let c = Aes256Gcm::new_from_slice(key).map_err(|e| anyhow::anyhow!("{e}"))?;
    c.decrypt(Nonce::from_slice(n), ct)
        .map_err(|_| anyhow::anyhow!("decrypt failed — wrong factor(s) or corrupt vault"))
}

fn kdf(secret: &str, salt: &[u8; 16], domain: &str) -> [u8; 32] {
    let mut h = blake3::Hasher::new_derive_key(domain);
    h.update(salt);
    h.update(secret.as_bytes());
    *h.finalize().as_bytes()
}

fn xor32(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut o = [0u8; 32];
    for i in 0..32 {
        o[i] = a[i] ^ b[i];
    }
    o
}

fn read_line_secret(prompt: &str) -> Result<String> {
    eprint!("{prompt}: ");
    let _ = io::stderr().flush();
    match rpassword::read_password() {
        Ok(s) => Ok(s),
        Err(_) => {
            let mut s = String::new();
            io::stdin().read_line(&mut s)?;
            Ok(s.trim_end_matches(['\n', '\r']).to_string())
        }
    }
}

fn read_confirm(prompt: &str) -> Result<String> {
    let a = read_line_secret(prompt)?;
    let b = read_line_secret("Confirm")?;
    if a != b {
        bail!("entries do not match");
    }
    if a.len() < 8 {
        bail!("too short (min 8)");
    }
    Ok(a)
}

fn vault_exists(c: &Path) -> bool {
    p_enc(c).exists() || p_raw(c).exists()
}

fn load_meta(c: &Path) -> Result<VaultMeta> {
    Ok(serde_json::from_str(&fs::read_to_string(p_meta(c))?)?)
}

fn gen_operator_secret() -> ([u8; 32], String) {
    let sk = SigningKey::generate(&mut OsRng);
    let pub_hex = hex::encode(sk.verifying_key().as_bytes());
    (sk.to_bytes(), pub_hex)
}

fn write_session(c: &Path, dek: &[u8; 32]) -> Result<()> {
    let s = SessionFile {
        unlocked_at_unix: now_unix(),
        dek_hex: hex::encode(dek),
    };
    write_secret(&p_session(c), serde_json::to_string_pretty(&s)?.as_bytes())
}

fn session_dek(c: &Path) -> Result<Option<[u8; 32]>> {
    let p = p_session(c);
    if !p.exists() {
        return Ok(None);
    }
    let s: SessionFile = serde_json::from_str(&fs::read_to_string(&p)?)?;
    if now_unix().saturating_sub(s.unlocked_at_unix) > SESSION_MAX_SECS {
        let _ = fs::remove_file(&p);
        return Ok(None);
    }
    let b = hex::decode(s.dek_hex)?;
    Ok(Some(
        b.as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("bad DEK"))?,
    ))
}

pub fn normalize_peer_target(raw: &str) -> String {
    let s = raw.trim();
    let hex_only: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex_only.len() == 12 && (s.contains(':') || s.contains('-')) {
        let l = hex_only.to_lowercase();
        return format!(
            "mac:{}:{}:{}:{}:{}:{}",
            &l[0..2],
            &l[2..4],
            &l[4..6],
            &l[6..8],
            &l[8..10],
            &l[10..12]
        );
    }
    if s.parse::<std::net::Ipv4Addr>().is_ok() || s.parse::<std::net::Ipv6Addr>().is_ok() {
        return format!("ip:{s}");
    }
    s.to_string()
}

// ─── Init ───────────────────────────────────────────────────────────────────

pub async fn auth_init(config_dir: &Path, mode: AuthMode) -> Result<()> {
    if vault_exists(config_dir) {
        bail!(
            "vault exists at {} — `grid auth delete --wipe-keys` first",
            keys_dir(config_dir).display()
        );
    }
    fs::create_dir_all(keys_dir(config_dir))?;
    match mode {
        AuthMode::Nocrypt => init_nocrypt(config_dir),
        AuthMode::Passkey => init_passkey(config_dir).await,
        AuthMode::Password => init_password(config_dir),
        AuthMode::Keyphrase => init_keyphrase_only(config_dir),
        AuthMode::Combo => init_combo(config_dir).await,
        AuthMode::Master => bail!(
            "master mode (DESTROY / master-key wipe) was removed.\n\
             It was never required for genesis or the blockchain.\n\
             Use:  grid auth            # passkey (default)\n\
                   grid auth combo      # password + passkey + keyphrase\n\
             Existing master vaults: grid auth login still works.\n\
             To switch modes: grid auth delete --wipe-keys  then  grid auth"
        ),
    }
}

fn init_nocrypt(c: &Path) -> Result<()> {
    let (secret, pub_hex) = gen_operator_secret();
    write_secret(&p_raw(c), &secret)?;
    write_secret(&p_pub(c), pub_hex.as_bytes())?;
    save_meta(
        c,
        VaultMeta {
            mode: "nocrypt".into(),
            encrypted: false,
            algorithm: "none".into(),
            created_at: Utc::now().to_rfc3339(),
            public_key_hex: pub_hex.clone(),
            kdf_salt_hex: None,
            master_destroyed: false,
            factors: None,
        },
    )?;
    println!("✓ nocrypt keys");
    println!("  public: {pub_hex}");
    println!("  secret: {} (0600, plaintext)", p_raw(c).display());
    println!("  ⚠ disk access = full key access");
    Ok(())
}

async fn init_passkey(c: &Path) -> Result<()> {
    println!("Mode: passkey (default)\n");
    ceremony::register_passkey(c).await?;
    let mut seal = [0u8; 32];
    let mut dek = [0u8; 32];
    OsRng.fill_bytes(&mut seal);
    OsRng.fill_bytes(&mut dek);
    let (secret, pub_hex) = gen_operator_secret();
    write_secret(&p_seal(c), &seal)?;
    write_secret(&p_wrap(c), &aes_encrypt(&seal, &dek)?)?;
    write_secret(&p_enc(c), &aes_encrypt(&dek, &secret)?)?;
    write_secret(&p_pub(c), pub_hex.as_bytes())?;
    save_meta(
        c,
        VaultMeta {
            mode: "passkey".into(),
            encrypted: true,
            algorithm: "AES-256-GCM+passkey-gate".into(),
            created_at: Utc::now().to_rfc3339(),
            public_key_hex: pub_hex.clone(),
            kdf_salt_hex: None,
            master_destroyed: false,
            factors: Some(vec!["passkey".into()]),
        },
    )?;
    write_session(c, &dek)?;
    println!("✓ encrypted vault (passkey)");
    println!("  public: {pub_hex}");
    println!("  session: UNLOCKED");
    Ok(())
}

fn init_password(c: &Path) -> Result<()> {
    println!("Mode: password\n");
    let pw = read_confirm("Master password")?;
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let k = kdf(&pw, &salt, "GRID vault password v1");
    finish_single_factor(c, "password", &k, &salt, "AES-256-GCM+password-kdf")
}

fn init_keyphrase_only(c: &Path) -> Result<()> {
    println!("Mode: keyphrase (24 words)\n");
    let phrase = gen_24_words()?;
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  WRITE THESE 24 WORDS DOWN. THEY WILL NOT BE SHOWN AGAIN ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");
    println!("{phrase}\n");
    println!("Press Enter after you have written them down…");
    let mut _line = String::new();
    let _ = io::stdin().read_line(&mut _line);

    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let k = kdf(&phrase, &salt, "GRID vault keyphrase v1");
    finish_single_factor(c, "keyphrase", &k, &salt, "AES-256-GCM+keyphrase-kdf")
}

fn gen_24_words() -> Result<String> {
    // 24 words = 256 bits entropy
    let mut entropy = [0u8; 32];
    OsRng.fill_bytes(&mut entropy);
    let mnemonic = Mnemonic::from_entropy_in(Language::English, &entropy)
        .map_err(|e| anyhow::anyhow!("mnemonic: {e}"))?;
    Ok(mnemonic.to_string())
}

async fn init_combo(c: &Path) -> Result<()> {
    println!("Mode: combo (password → passkey → keyphrase)\n");
    let pw = read_confirm("Password (1/3)")?;
    println!("\nPasskey (2/3)…");
    ceremony::register_passkey(c).await?;
    println!("\nKeyphrase (3/3) — generating 24 words…");
    let phrase = gen_24_words()?;
    println!("\n{phrase}\n");
    println!("Press Enter after saving the phrase offline…");
    let mut _line = String::new();
    let _ = io::stdin().read_line(&mut _line);

    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let k1 = kdf(&pw, &salt, "GRID vault password v1");
    let k2 = kdf(&phrase, &salt, "GRID vault keyphrase v1");
    let wrap = xor32(&k1, &k2);
    // passkey is a gate on login, not in the XOR (ceremony-only factor)

    let mut dek = [0u8; 32];
    OsRng.fill_bytes(&mut dek);
    let (secret, pub_hex) = gen_operator_secret();
    write_secret(&p_wrap(c), &aes_encrypt(&wrap, &dek)?)?;
    write_secret(&p_enc(c), &aes_encrypt(&dek, &secret)?)?;
    write_secret(&p_pub(c), pub_hex.as_bytes())?;
    save_meta(
        c,
        VaultMeta {
            mode: "combo".into(),
            encrypted: true,
            algorithm: "AES-256-GCM+combo".into(),
            created_at: Utc::now().to_rfc3339(),
            public_key_hex: pub_hex.clone(),
            kdf_salt_hex: Some(hex::encode(salt)),
            master_destroyed: false,
            factors: Some(vec![
                "password".into(),
                "passkey".into(),
                "keyphrase".into(),
            ]),
        },
    )?;
    write_session(c, &dek)?;
    println!("✓ encrypted vault (combo)");
    println!("  public: {pub_hex}");
    Ok(())
}

fn finish_single_factor(
    c: &Path,
    mode: &str,
    wrap_key: &[u8; 32],
    salt: &[u8; 16],
    algo: &str,
) -> Result<()> {
    let mut dek = [0u8; 32];
    OsRng.fill_bytes(&mut dek);
    let (secret, pub_hex) = gen_operator_secret();
    write_secret(&p_wrap(c), &aes_encrypt(wrap_key, &dek)?)?;
    write_secret(&p_enc(c), &aes_encrypt(&dek, &secret)?)?;
    write_secret(&p_pub(c), pub_hex.as_bytes())?;
    save_meta(
        c,
        VaultMeta {
            mode: mode.into(),
            encrypted: true,
            algorithm: algo.into(),
            created_at: Utc::now().to_rfc3339(),
            public_key_hex: pub_hex.clone(),
            kdf_salt_hex: Some(hex::encode(salt)),
            master_destroyed: false,
            factors: Some(vec![mode.into()]),
        },
    )?;
    write_session(c, &dek)?;
    println!("✓ encrypted vault ({mode})");
    println!("  public: {pub_hex}");
    Ok(())
}

fn save_meta(c: &Path, m: VaultMeta) -> Result<()> {
    fs::write(p_meta(c), serde_json::to_string_pretty(&m)?)?;
    Ok(())
}

// ─── Login ──────────────────────────────────────────────────────────────────

pub async fn auth_login(config_dir: &Path) -> Result<()> {
    if !vault_exists(config_dir) {
        bail!("no vault — run: grid auth");
    }
    let meta = load_meta(config_dir)?;
    let mode = AuthMode::parse(&meta.mode)?;

    if mode == AuthMode::Nocrypt {
        println!("✓ nocrypt — keys always available (not encrypted)");
        return Ok(());
    }

    let dek = match mode {
        AuthMode::Passkey => {
            ceremony::require_passkey(config_dir, "Unlock GRID vault").await?;
            unwrap_passkey_seal(config_dir)?
        }
        AuthMode::Password => {
            let pw = read_line_secret("Master password")?;
            unwrap_kdf(config_dir, &meta, &pw, "GRID vault password v1")?
        }
        AuthMode::Keyphrase => {
            let ph = read_line_secret("24-word keyphrase")?;
            unwrap_kdf(config_dir, &meta, &ph, "GRID vault keyphrase v1")?
        }
        AuthMode::Combo => {
            let pw = read_line_secret("Password (1/3)")?;
            ceremony::require_passkey(config_dir, "Unlock vault (2/3)").await?;
            let ph = read_line_secret("Keyphrase (3/3)")?;
            let salt = salt(&meta)?;
            let wrap = xor32(
                &kdf(&pw, &salt, "GRID vault password v1"),
                &kdf(&ph, &salt, "GRID vault keyphrase v1"),
            );
            let blob = fs::read(p_wrap(config_dir))?;
            let v = aes_decrypt(&wrap, &blob)?;
            v.as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("DEK"))?
        }
        AuthMode::Master => unlock_master(config_dir, &meta).await?,
        AuthMode::Nocrypt => unreachable!(),
    };

    write_session(config_dir, &dek)?;
    println!(
        "✓ Authenticated ({}) — session UNLOCKED for {}h",
        mode.as_str(),
        SESSION_MAX_SECS / 3600
    );
    Ok(())
}

async fn unlock_master(c: &Path, meta: &VaultMeta) -> Result<[u8; 32]> {
    let needs_passkey = meta
        .factors
        .as_ref()
        .map(|f| f.iter().any(|x| x == "passkey"))
        .unwrap_or(false)
        || pstore::has_passkey(c);

    let n = if needs_passkey { 4 } else { 3 };
    println!("Master vault unlock — all {n} factors required.\n");

    let step_pw = "1";
    let pw = read_line_secret(&format!("Master password ({step_pw}/{n})"))?;

    if needs_passkey {
        ceremony::require_passkey(c, &format!("Unlock vault (2/{n}) — passkey")).await?;
    }

    let step_ph = if needs_passkey { "3" } else { "2" };
    let phrase = read_line_secret(&format!("24-word keyphrase ({step_ph}/{n})"))?;

    let step_mk = if needs_passkey { "4" } else { "3" };
    println!("Master key: paste hex OR path to key file");
    let mk_in = read_line_secret(&format!("Master key ({step_mk}/{n}, hex or path)"))?;

    let mut master_key = load_master_key_input(&mk_in)?;
    let salt = salt(meta)?;
    let k_pw = kdf(&pw, &salt, "GRID vault password v1");
    let k_ph = kdf(&phrase, &salt, "GRID vault keyphrase v1");
    let sealed: [u8; 32] = fs::read(p_sealed(c))?
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("bad sealed DEK"))?;

    // DEK = sealed ⊕ K_pw ⊕ K_ph ⊕ master_key
    let t1 = xor32(&sealed, &k_pw);
    let t2 = xor32(&t1, &k_ph);
    let dek = xor32(&t2, &master_key);
    master_key.zeroize();

    // verify DEK by attempting decrypt of operator.enc
    let blob = fs::read(p_enc(c))?;
    let _ = aes_decrypt(&dek, &blob)
        .context("unlock failed — check password, phrase, and master key")?;
    Ok(dek)
}

fn load_master_key_input(input: &str) -> Result<[u8; 32]> {
    let s = input.trim();
    let hex_str = if Path::new(s).exists() {
        fs::read_to_string(s)?.trim().to_string()
    } else {
        s.to_string()
    };
    let bytes = hex::decode(hex_str.trim()).context("master key must be 64 hex chars or a file")?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("master key must be 32 bytes (64 hex chars)"))
}

fn salt(meta: &VaultMeta) -> Result<[u8; 16]> {
    let h = meta.kdf_salt_hex.as_ref().context("missing salt")?;
    Ok(hex::decode(h)?
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("bad salt"))?)
}

fn unwrap_kdf(c: &Path, meta: &VaultMeta, secret: &str, domain: &str) -> Result<[u8; 32]> {
    let s = salt(meta)?;
    let k = kdf(secret, &s, domain);
    let blob = fs::read(p_wrap(c))?;
    let v = aes_decrypt(&k, &blob)?;
    Ok(v.as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("DEK"))?)
}

fn unwrap_passkey_seal(c: &Path) -> Result<[u8; 32]> {
    let seal: [u8; 32] = fs::read(p_seal(c))?
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("seal"))?;
    let blob = fs::read(p_wrap(c))?;
    let v = aes_decrypt(&seal, &blob)?;
    Ok(v.as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("DEK"))?)
}

// ─── Status / delete / unlock gate ──────────────────────────────────────────

pub fn auth_status(config_dir: &Path) -> AuthStatus {
    let meta = load_meta(config_dir).ok();
    let mode = meta
        .as_ref()
        .map(|m| m.mode.clone())
        .unwrap_or_else(|| "none".into());
    let passkey_registered = pstore::has_passkey(config_dir);
    let keys_encrypted = meta.as_ref().map(|m| m.encrypted).unwrap_or(false);
    let master_destroyed = meta.as_ref().map(|m| m.master_destroyed).unwrap_or(false);
    let has = vault_exists(config_dir);
    let session_unlocked = if mode == "nocrypt" {
        has
    } else {
        session_dek(config_dir).ok().flatten().is_some()
    };
    let public_key_hex = meta.as_ref().map(|m| m.public_key_hex.clone());

    let detail = if !has {
        "uninitialized — run: grid auth".into()
    } else if mode == "nocrypt" {
        "keys: PLAINTEXT · not encrypted".into()
    } else if mode == "master" && session_unlocked {
        format!(
            "keys: ENCRYPTED (legacy master vault) · session: UNLOCKED · prefer migrate via `grid auth delete --wipe-keys` then `grid auth`"
        )
    } else if mode == "master" {
        format!("keys: ENCRYPTED (legacy master vault) · session: LOCKED — grid auth login")
    } else if keys_encrypted && session_unlocked {
        format!("keys: ENCRYPTED ({mode}) · session: UNLOCKED")
    } else if keys_encrypted {
        format!("keys: ENCRYPTED ({mode}) · session: LOCKED — grid auth login")
    } else {
        "partial".into()
    };
    let _ = master_destroyed; // legacy field; new vaults never set it

    AuthStatus {
        mode,
        passkey_registered,
        keys_encrypted,
        session_unlocked,
        master_destroyed,
        public_key_hex,
        detail,
    }
}

pub async fn require_unlocked(config_dir: &Path, purpose: &str) -> Result<[u8; 32]> {
    if let Ok(m) = load_meta(config_dir) {
        if m.mode == "nocrypt" {
            return Ok([0u8; 32]);
        }
    }
    if let Some(d) = session_dek(config_dir)? {
        return Ok(d);
    }
    println!("Vault locked ({purpose})");
    auth_login(config_dir).await?;
    session_dek(config_dir)?.context("no session after login")
}

/// Step-up: require an unlocked session **and** a fresh passkey ceremony when one is registered.
pub async fn require_identity(config_dir: &Path, purpose: &str) -> Result<[u8; 32]> {
    let dek = require_unlocked(config_dir, purpose).await?;
    if pstore::has_passkey(config_dir) {
        ceremony::require_passkey(config_dir, purpose).await?;
    }
    Ok(dek)
}

/// Public operator Ed25519 key (hex), from vault meta or `operator.pub`.
pub fn operator_pubkey_hex(config_dir: &Path) -> Result<String> {
    if let Ok(m) = load_meta(config_dir) {
        if !m.public_key_hex.is_empty() {
            return Ok(m.public_key_hex);
        }
    }
    let p = p_pub(config_dir);
    if p.exists() {
        return Ok(fs::read_to_string(p)?.trim().to_string());
    }
    bail!("no operator public key — run: grid auth");
}

/// Load the operator signing key. `dek` from [`require_unlocked`] / [`require_identity`].
pub fn load_operator_signing_key(config_dir: &Path, dek: &[u8; 32]) -> Result<SigningKey> {
    let secret = if p_raw(config_dir).exists() {
        let raw = fs::read(p_raw(config_dir)).context("read operator.key")?;
        let arr: [u8; 32] = raw
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("operator.key must be 32 bytes"))?;
        arr
    } else if p_enc(config_dir).exists() {
        let blob = fs::read(p_enc(config_dir)).context("read operator.enc")?;
        // nocrypt path passes zero DEK — encrypted vaults need a real session DEK
        if dek.iter().all(|&b| b == 0) {
            if let Ok(m) = load_meta(config_dir) {
                if m.mode != "nocrypt" {
                    bail!("vault locked — grid auth login first");
                }
            }
        }
        let pt = aes_decrypt(dek, &blob).context("decrypt operator secret")?;
        let arr: [u8; 32] = pt
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("operator secret must be 32 bytes"))?;
        arr
    } else {
        bail!("no operator key material — run: grid auth");
    };
    Ok(SigningKey::from_bytes(&secret))
}

/// Sign arbitrary message bytes with the operator key. Returns hex signature.
pub fn sign_operator(config_dir: &Path, dek: &[u8; 32], message: &[u8]) -> Result<String> {
    let sk = load_operator_signing_key(config_dir, dek)?;
    let sig: Signature = sk.sign(message);
    Ok(hex::encode(sig.to_bytes()))
}

/// Derive an in-memory X25519 static secret for the encrypted P2P transport.
///
/// It is domain-separated from the Ed25519 signing key and is never persisted.
/// The caller must first pass the normal vault/passkey unlock gate.
pub fn p2p_noise_static_key(config_dir: &Path, dek: &[u8; 32]) -> Result<[u8; 32]> {
    let signing = load_operator_signing_key(config_dir, dek)?;
    Ok(blake3::derive_key(
        "GRID P2P Noise static key v1",
        &signing.to_bytes(),
    ))
}

/// Verify an operator signature (hex pubkey + hex sig).
pub fn verify_operator_sig(pubkey_hex: &str, message: &[u8], sig_hex: &str) -> Result<()> {
    let pk_bytes = hex::decode(pubkey_hex.trim()).context("decode operator pubkey")?;
    let arr: [u8; 32] = pk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("operator pubkey must be 32 bytes"))?;
    let vk = VerifyingKey::from_bytes(&arr)?;
    let sig_bytes = hex::decode(sig_hex.trim()).context("decode signature")?;
    let sarr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("signature must be 64 bytes"))?;
    let sig = Signature::from_bytes(&sarr);
    vk.verify(message, &sig)
        .map_err(|e| anyhow::anyhow!("invalid operator signature: {e}"))
}

pub async fn auth_delete(config_dir: &Path, wipe_keys: bool) -> Result<()> {
    if !vault_exists(config_dir) && !pstore::has_passkey(config_dir) {
        bail!("nothing to delete");
    }
    let mode = load_meta(config_dir)
        .map(|m| m.mode)
        .unwrap_or_else(|_| "unknown".into());

    if mode != "nocrypt" {
        if pstore::has_passkey(config_dir) {
            ceremony::require_passkey(config_dir, "Delete auth protection").await?;
        } else if vault_exists(config_dir) {
            auth_login(config_dir).await?;
        }
    }

    if pstore::store_path(config_dir).exists() {
        fs::remove_file(pstore::store_path(config_dir))?;
        println!("✓ passkey credential removed");
    }
    if p_session(config_dir).exists() {
        fs::remove_file(p_session(config_dir))?;
        println!("✓ session cleared");
    }
    if wipe_keys {
        for p in [
            p_enc(config_dir),
            p_raw(config_dir),
            p_seal(config_dir),
            p_wrap(config_dir),
            p_sealed(config_dir),
            p_meta(config_dir),
            p_pub(config_dir),
        ] {
            if p.exists() {
                fs::remove_file(&p)?;
                println!("✓ wiped {}", p.file_name().unwrap().to_string_lossy());
            }
        }
    } else {
        println!("(keys kept — use --wipe-keys to destroy operator key material)");
    }
    println!("✓ auth deleted");
    Ok(())
}
