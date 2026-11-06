//! Repli du coffre du système pour le token et le secret d'appareil.
//!
//! Sur certains PC, le Gestionnaire d'identification de Windows refuse
//! d'enregistrer (« Platform secure storage failure: Windows error code 8 ») :
//! le site délivrait le token, le bridge ne pouvait pas le garder et restait
//! déconnecté. Le token va alors dans un fichier du dossier du bridge, chiffré
//! par Windows pour ce seul compte utilisateur (DPAPI, `CryptProtectData`) ;
//! ailleurs, un fichier lisible par ce seul utilisateur (droits 0600).
//!
//! Le coffre reste le premier choix : le fichier n'est écrit que si le coffre
//! refuse, et il est effacé dès que le coffre accepte de nouveau.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::auth::{
    DeviceSecretStore, KeyringDeviceSecretStore, KeyringStore, StoreError, TokenStore,
};
use crate::fsutil::atomic_write;

/// Fichier du token de repli, dans le dossier de données du bridge.
pub const TOKEN_FILE: &str = "token.protected";
/// Fichier du secret d'appareil de repli.
pub const DEVICE_SECRET_FILE: &str = "device-secret.protected";

/// Un secret dans un fichier protégé (DPAPI sous Windows, 0600 ailleurs).
#[derive(Debug, Clone)]
pub struct ProtectedFile {
    path: PathBuf,
}

impl ProtectedFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        ProtectedFile { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Option<String>, StoreError> {
        let bytes = match fs::read(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(StoreError(format!("lecture du fichier de secours : {e}"))),
        };
        let plain = unprotect(&bytes)?;
        let text = String::from_utf8(plain)
            .map_err(|_| StoreError("fichier de secours illisible".into()))?;
        Ok(if text.is_empty() { None } else { Some(text) })
    }

    pub fn save(&self, secret: &str) -> Result<(), StoreError> {
        let protected = protect(secret.as_bytes())?;
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)
                .map_err(|e| StoreError(format!("dossier du fichier de secours : {e}")))?;
        }
        atomic_write(&self.path, &protected)
            .map_err(|e| StoreError(format!("écriture du fichier de secours : {e}")))?;
        restrict_permissions(&self.path);
        Ok(())
    }

    pub fn clear(&self) -> Result<(), StoreError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StoreError(format!(
                "effacement du fichier de secours : {e}"
            ))),
        }
    }
}

/// Token : coffre du système d'abord, fichier protégé si le coffre refuse.
pub struct FallbackTokenStore {
    primary: Box<dyn TokenStore>,
    file: ProtectedFile,
}

impl FallbackTokenStore {
    /// Coffre du système + `<data_dir>/token.protected`.
    pub fn new(data_dir: &Path) -> Self {
        Self::with(
            Box::new(KeyringStore::new()),
            ProtectedFile::new(data_dir.join(TOKEN_FILE)),
        )
    }

    pub fn with(primary: Box<dyn TokenStore>, file: ProtectedFile) -> Self {
        FallbackTokenStore { primary, file }
    }
}

impl TokenStore for FallbackTokenStore {
    fn load(&self) -> Result<Option<String>, StoreError> {
        match self.primary.load() {
            Ok(Some(token)) => Ok(Some(token)),
            Ok(None) => self.file.load(),
            Err(primary) => match self.file.load() {
                Ok(Some(token)) => Ok(Some(token)),
                Ok(None) => Err(primary),
                Err(file) => Err(both(primary, file)),
            },
        }
    }

    fn save(&self, token: &str) -> Result<(), StoreError> {
        match self.primary.save(token) {
            Ok(()) => {
                // The vault works again: an older fallback copy must not linger.
                let _ = self.file.clear();
                Ok(())
            }
            Err(primary) => self.file.save(token).map_err(|file| both(primary, file)),
        }
    }

    fn clear(&self) -> Result<(), StoreError> {
        let primary = self.primary.clear();
        let file = self.file.clear();
        match (primary, file) {
            (_, Err(file)) => Err(file),
            // A broken vault that never held the token must not block logout.
            _ => Ok(()),
        }
    }
}

/// Secret d'appareil (vigies) : même repli que le token.
pub struct FallbackDeviceSecretStore {
    primary: Box<dyn DeviceSecretStore>,
    file: ProtectedFile,
}

impl FallbackDeviceSecretStore {
    /// Coffre du système + `<data_dir>/device-secret.protected`.
    pub fn new(data_dir: &Path) -> Self {
        Self::with(
            Box::new(KeyringDeviceSecretStore::new()),
            ProtectedFile::new(data_dir.join(DEVICE_SECRET_FILE)),
        )
    }

    pub fn with(primary: Box<dyn DeviceSecretStore>, file: ProtectedFile) -> Self {
        FallbackDeviceSecretStore { primary, file }
    }
}

impl DeviceSecretStore for FallbackDeviceSecretStore {
    fn load(&self) -> Result<Option<String>, StoreError> {
        match self.primary.load() {
            Ok(Some(secret)) => Ok(Some(secret)),
            Ok(None) => self.file.load(),
            Err(primary) => match self.file.load() {
                Ok(Some(secret)) => Ok(Some(secret)),
                Ok(None) => Err(primary),
                Err(file) => Err(both(primary, file)),
            },
        }
    }

    fn save(&self, secret: &str) -> Result<(), StoreError> {
        match self.primary.save(secret) {
            Ok(()) => {
                let _ = self.file.clear();
                Ok(())
            }
            Err(primary) => self.file.save(secret).map_err(|file| both(primary, file)),
        }
    }
}

fn both(primary: StoreError, file: StoreError) -> StoreError {
    StoreError(format!("{} ; repli : {}", primary.0, file.0))
}

#[cfg(windows)]
fn protect(plain: &[u8]) -> Result<Vec<u8>, StoreError> {
    dpapi::protect(plain)
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> Result<Vec<u8>, StoreError> {
    dpapi::unprotect(data)
}

#[cfg(not(windows))]
fn protect(plain: &[u8]) -> Result<Vec<u8>, StoreError> {
    Ok(plain.to_vec())
}

#[cfg(not(windows))]
fn unprotect(data: &[u8]) -> Result<Vec<u8>, StoreError> {
    Ok(data.to_vec())
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

/// DPAPI, portée utilisateur : seul ce compte Windows, sur ce PC, peut relire.
#[cfg(windows)]
mod dpapi {
    use super::StoreError;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        }
    }

    /// Copie la sortie de DPAPI puis libère la mémoire allouée par Windows.
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        bytes
    }

    pub fn protect(plain: &[u8]) -> Result<Vec<u8>, StoreError> {
        let input = blob(plain);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: `input` points to `plain`, alive for the call; `out` is filled by
        // Windows and freed by `take` with LocalFree, as the API requires.
        let ok = unsafe {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 {
            return Err(StoreError(format!(
                "chiffrement Windows impossible ({})",
                std::io::Error::last_os_error()
            )));
        }
        Ok(unsafe { take(out) })
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, StoreError> {
        let input = blob(data);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: same contract as `protect`.
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 {
            return Err(StoreError(format!(
                "déchiffrement Windows impossible ({})",
                std::io::Error::last_os_error()
            )));
        }
        Ok(unsafe { take(out) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MemoryStore;

    /// Coffre qui refuse toujours, comme le Gestionnaire d'identification en panne.
    struct BrokenVault;
    impl TokenStore for BrokenVault {
        fn load(&self) -> Result<Option<String>, StoreError> {
            Err(StoreError("Windows error code 8".into()))
        }
        fn save(&self, _: &str) -> Result<(), StoreError> {
            Err(StoreError("Windows error code 8".into()))
        }
        fn clear(&self) -> Result<(), StoreError> {
            Err(StoreError("Windows error code 8".into()))
        }
    }
    impl DeviceSecretStore for BrokenVault {
        fn load(&self) -> Result<Option<String>, StoreError> {
            Err(StoreError("Windows error code 8".into()))
        }
        fn save(&self, _: &str) -> Result<(), StoreError> {
            Err(StoreError("Windows error code 8".into()))
        }
    }

    #[test]
    fn broken_vault_keeps_the_token_in_the_protected_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = ProtectedFile::new(dir.path().join(TOKEN_FILE));
        let store = FallbackTokenStore::with(Box::new(BrokenVault), file.clone());
        assert_eq!(store.load().unwrap_err().0, "Windows error code 8");
        store.save("12|abc").unwrap();
        assert!(file.path().exists());
        assert_eq!(store.load().unwrap().as_deref(), Some("12|abc"));
        // A fresh store (bridge restarted) reads it back.
        let again = FallbackTokenStore::with(Box::new(BrokenVault), file.clone());
        assert_eq!(again.load().unwrap().as_deref(), Some("12|abc"));
        store.clear().unwrap();
        assert!(!file.path().exists());
        assert!(store.load().is_err());
    }

    #[test]
    fn working_vault_wins_and_removes_an_old_fallback_copy() {
        let dir = tempfile::tempdir().unwrap();
        let file = ProtectedFile::new(dir.path().join(TOKEN_FILE));
        file.save("old").unwrap();
        let store = FallbackTokenStore::with(Box::new(MemoryStore::default()), file.clone());
        assert_eq!(
            store.load().unwrap().as_deref(),
            Some("old"),
            "fallback read while the vault is empty"
        );
        store.save("new").unwrap();
        assert!(
            !file.path().exists(),
            "the vault accepted: fallback copy removed"
        );
        assert_eq!(store.load().unwrap().as_deref(), Some("new"));
    }

    #[test]
    fn device_secret_falls_back_too() {
        let dir = tempfile::tempdir().unwrap();
        let file = ProtectedFile::new(dir.path().join(DEVICE_SECRET_FILE));
        let store = FallbackDeviceSecretStore::with(Box::new(BrokenVault), file);
        store.save("s3cret").unwrap();
        assert_eq!(store.load().unwrap().as_deref(), Some("s3cret"));
    }

    #[cfg(unix)]
    #[test]
    fn fallback_file_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = ProtectedFile::new(dir.path().join(TOKEN_FILE));
        file.save("t").unwrap();
        let mode = fs::metadata(file.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
