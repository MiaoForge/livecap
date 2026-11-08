use crate::transcript::Provider;
use std::sync::Mutex;
use zeroize::Zeroizing;

#[derive(Default)]
enum KeyState {
    #[default]
    Unread,
    Missing,
    Present(Zeroizing<String>),
}

impl KeyState {
    fn loaded(value: Option<String>) -> Self {
        value.map_or(Self::Missing, |value| Self::Present(Zeroizing::new(value)))
    }
}

#[derive(Default)]
struct Cache {
    openai: KeyState,
    soniox: KeyState,
}

impl Cache {
    fn slot(&mut self, provider: Provider) -> &mut KeyState {
        match provider {
            Provider::Openai => &mut self.openai,
            Provider::Soniox => &mut self.soniox,
        }
    }
}

/// Native, process-lifetime cache. Serializing vault access also prevents
/// concurrent requests from opening duplicate macOS Keychain permission dialogs.
#[derive(Default)]
pub struct Credentials(Mutex<Cache>);

impl Credentials {
    pub fn get(&self, provider: Provider) -> Result<Option<String>, String> {
        self.get_with(provider, || match entry(provider)?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(format!("无法读取 {} API Key。", provider.label())),
        })
    }

    pub fn save(&self, provider: Provider, key: &str) -> Result<(), String> {
        self.update_with(provider, Some(key.to_owned()), || {
            entry(provider)?
                .set_password(key)
                .map_err(|_| format!("无法保存 {} API Key。", provider.label()))
        })
    }

    pub fn delete(&self, provider: Provider) -> Result<(), String> {
        self.update_with(provider, None, || {
            match entry(provider)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err(format!("无法删除 {} API Key。", provider.label())),
            }
        })
    }

    fn get_with(
        &self,
        provider: Provider,
        load: impl FnOnce() -> Result<Option<String>, String>,
    ) -> Result<Option<String>, String> {
        let mut cache = self.0.lock().map_err(|_| "无法读取密钥缓存。")?;
        let slot = cache.slot(provider);
        if matches!(slot, KeyState::Unread) {
            // Authorization failures remain unread so an explicit retry can succeed.
            *slot = KeyState::loaded(load()?);
        }
        match slot {
            KeyState::Present(value) => Ok(Some(value.to_string())),
            KeyState::Missing => Ok(None),
            KeyState::Unread => unreachable!("successful load initializes the slot"),
        }
    }

    fn update_with(
        &self,
        provider: Provider,
        value: Option<String>,
        persist: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let mut cache = self.0.lock().map_err(|_| "无法读取密钥缓存。")?;
        persist()?;
        // Replacing a cached secret zeroes its old allocation. A failed vault
        // write leaves the existing cache intact, matching the persisted value.
        *cache.slot(provider) = KeyState::loaded(value);
        Ok(())
    }
}

fn entry(provider: Provider) -> Result<keyring::Entry, String> {
    let account = match provider {
        Provider::Openai => "openai",
        Provider::Soniox => "soniox",
    };
    keyring::Entry::new("com.chenmiaoi.livecap", account)
        .map_err(|_| "无法访问系统安全凭据存储。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn concurrent_reads_prompt_the_vault_only_once_per_provider() {
        let credentials = Credentials::default();
        let reads = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    let value = credentials
                        .get_with(Provider::Openai, || {
                            reads.fetch_add(1, Ordering::Relaxed);
                            std::thread::sleep(std::time::Duration::from_millis(5));
                            Ok(Some("test-value".into()))
                        })
                        .unwrap();
                    assert_eq!(value.as_deref(), Some("test-value"));
                });
            }
        });
        assert_eq!(reads.load(Ordering::Relaxed), 1);
        assert!(
            credentials
                .get_with(Provider::Soniox, || Ok(None))
                .unwrap()
                .is_none()
        );
        assert!(
            credentials
                .get_with(Provider::Soniox, || panic!("missing entry is cached too"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn access_denial_can_be_retried_without_restarting_the_app() {
        let credentials = Credentials::default();
        assert!(
            credentials
                .get_with(Provider::Openai, || Err("denied".into()))
                .is_err()
        );
        assert_eq!(
            credentials
                .get_with(Provider::Openai, || Ok(Some("test-value".into())))
                .unwrap()
                .as_deref(),
            Some("test-value")
        );
    }

    #[test]
    fn save_delete_and_failed_write_keep_cache_consistent_with_vault() {
        let credentials = Credentials::default();
        credentials
            .update_with(Provider::Openai, Some("old-test-value".into()), || Ok(()))
            .unwrap();
        assert!(
            credentials
                .update_with(Provider::Openai, Some("failed-test-value".into()), || Err(
                    "write failed".into()
                ))
                .is_err()
        );
        assert_eq!(
            credentials
                .get_with(Provider::Openai, || panic!("must use cache"))
                .unwrap()
                .as_deref(),
            Some("old-test-value")
        );
        credentials
            .update_with(Provider::Openai, Some("new-test-value".into()), || Ok(()))
            .unwrap();
        assert_eq!(
            credentials
                .get_with(Provider::Openai, || panic!("must use new cache"))
                .unwrap()
                .as_deref(),
            Some("new-test-value")
        );
        assert!(
            credentials
                .update_with(Provider::Openai, None, || Err("delete failed".into()))
                .is_err()
        );
        assert!(
            credentials
                .get_with(Provider::Openai, || panic!(
                    "failed delete must preserve cache"
                ))
                .unwrap()
                .is_some()
        );
        credentials
            .update_with(Provider::Openai, None, || Ok(()))
            .unwrap();
        assert!(
            credentials
                .get_with(Provider::Openai, || panic!(
                    "deleted entry must stay missing"
                ))
                .unwrap()
                .is_none()
        );
        let fresh_process = Credentials::default();
        assert!(
            fresh_process
                .get_with(Provider::Openai, || Ok(None))
                .unwrap()
                .is_none()
        );
    }
}
