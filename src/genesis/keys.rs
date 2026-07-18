use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct GenesisKeys {
    pub signing: SigningKey,
    pub verifying: VerifyingKey,
}

impl GenesisKeys {
    pub fn public_hex(&self) -> String {
        hex::encode(self.verifying.as_bytes())
    }

    pub fn sign(&self, message: &[u8]) -> String {
        let sig: Signature = self.signing.sign(message);
        hex::encode(sig.to_bytes())
    }
}

pub fn genesis_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("genesis")
}

pub fn secret_path(config_dir: &Path) -> PathBuf {
    genesis_dir(config_dir).join("secret.key")
}

pub fn public_path(config_dir: &Path) -> PathBuf {
    genesis_dir(config_dir).join("public.key")
}

fn leader_secret_path(config_dir: &Path) -> PathBuf {
    genesis_dir(config_dir).join("leader.enc")
}

fn leader_public_path(config_dir: &Path) -> PathBuf {
    genesis_dir(config_dir).join("leader.pub")
}

fn recovery_dir(config_dir: &Path) -> PathBuf {
    genesis_dir(config_dir).join("recovery")
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisAuthority {
    pub leader_pubkey: String,
    pub recovery_pubkeys: Vec<String>,
}

pub fn load_authority(config_dir: &Path) -> Result<GenesisAuthority> {
    Ok(serde_json::from_str(&fs::read_to_string(
        genesis_dir(config_dir).join("authority.json"),
    )?)?)
}

/// Create a fresh leader and two recovery signing keys. All private material is
/// encrypted by the caller's unlocked combo/passkey vault; only public keys are
/// written in the genesis directory.
pub fn generate_protected(config_dir: &Path, dek: &[u8; 32]) -> Result<GenesisAuthority> {
    let dir = genesis_dir(config_dir);
    if leader_secret_path(config_dir).exists() || secret_path(config_dir).exists() {
        anyhow::bail!("genesis authority already exists — refuse to overwrite");
    }
    fs::create_dir_all(recovery_dir(config_dir))?;

    let leader = SigningKey::generate(&mut OsRng);
    let leader_pubkey = hex::encode(leader.verifying_key().as_bytes());
    write_secret(
        &leader_secret_path(config_dir),
        &crate::passkey::encrypt_with_vault(dek, &leader.to_bytes())?,
    )?;
    fs::write(leader_public_path(config_dir), &leader_pubkey)?;

    let mut recovery_pubkeys = Vec::with_capacity(2);
    for label in ["recovery-a", "recovery-b"] {
        let key = SigningKey::generate(&mut OsRng);
        let public = hex::encode(key.verifying_key().as_bytes());
        write_secret(
            &recovery_dir(config_dir).join(format!("{label}.enc")),
            &crate::passkey::encrypt_with_vault(dek, &key.to_bytes())?,
        )?;
        fs::write(
            recovery_dir(config_dir).join(format!("{label}.pub")),
            &public,
        )?;
        recovery_pubkeys.push(public);
    }
    let authority = GenesisAuthority {
        leader_pubkey,
        recovery_pubkeys,
    };
    fs::write(
        dir.join("authority.json"),
        serde_json::to_string_pretty(&authority)?,
    )?;
    Ok(authority)
}

/// Load the protected leader key after a fresh vault/passkey authorization.
pub fn load_protected(config_dir: &Path, dek: &[u8; 32]) -> Result<GenesisKeys> {
    let encrypted = fs::read(leader_secret_path(config_dir))
        .with_context(|| format!("read {}", leader_secret_path(config_dir).display()))?;
    let raw = crate::passkey::decrypt_with_vault(dek, &encrypted)?;
    let bytes: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("protected genesis key must be 32 bytes"))?;
    let signing = SigningKey::from_bytes(&bytes);
    Ok(GenesisKeys {
        verifying: signing.verifying_key(),
        signing,
    })
}

/// Generate genesis keypair. Secret file mode 0600 (Unix).
pub fn generate_keypair(config_dir: &Path) -> Result<GenesisKeys> {
    let dir = genesis_dir(config_dir);
    fs::create_dir_all(&dir)?;

    let sp = secret_path(config_dir);
    if sp.exists() {
        anyhow::bail!(
            "genesis secret already exists at {} — refuse to overwrite",
            sp.display()
        );
    }

    let signing = SigningKey::generate(&mut OsRng);
    let verifying = signing.verifying_key();

    write_secret(&sp, &signing.to_bytes())?;
    fs::write(public_path(config_dir), hex::encode(verifying.as_bytes()))?;

    Ok(GenesisKeys { signing, verifying })
}

pub fn load_keypair(config_dir: &Path) -> Result<GenesisKeys> {
    let raw = fs::read(secret_path(config_dir))
        .with_context(|| format!("read {}", secret_path(config_dir).display()))?;
    let bytes: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("secret.key must be 32 bytes"))?;
    let signing = SigningKey::from_bytes(&bytes);
    let verifying = signing.verifying_key();
    Ok(GenesisKeys { signing, verifying })
}

pub fn export_pubkey_hex(config_dir: &Path) -> Result<String> {
    let protected = leader_public_path(config_dir);
    if protected.exists() {
        return Ok(fs::read_to_string(protected)?.trim().to_string());
    }
    let p = public_path(config_dir);
    if p.exists() {
        return Ok(fs::read_to_string(p)?.trim().to_string());
    }
    Ok(load_keypair(config_dir)?.public_hex())
}

pub fn parse_pubkey(hex_str: &str) -> Result<VerifyingKey> {
    let bytes = hex::decode(hex_str.trim()).context("decode genesis pubkey hex")?;
    let arr: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("pubkey must be 32 bytes"))?;
    Ok(VerifyingKey::from_bytes(&arr)?)
}

pub fn verify_sig(pubkey: &VerifyingKey, message: &[u8], sig_hex: &str) -> Result<()> {
    let sig_bytes = hex::decode(sig_hex).context("decode signature")?;
    let arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("signature must be 64 bytes"))?;
    let sig = Signature::from_bytes(&arr);
    pubkey
        .verify(message, &sig)
        .map_err(|e| anyhow::anyhow!("invalid genesis signature: {e}"))
}

#[cfg(unix)]
fn write_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes)?;
    Ok(())
}
