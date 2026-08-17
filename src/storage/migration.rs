// ===================================================================
// PACYTE NEXUS - STORAGE MIGRATION
// Bölüm 15 - Dosya 15.4: src/storage/migration.rs
// ===================================================================

use crate::types::{PacyteError, PacyteResult};
use super::{Storage, RocksDBStorage};
use std::sync::Arc;
use tracing::info;

pub struct MigrationManager {
    current_version: u32,
}

impl MigrationManager {
    pub fn new() -> Self {
        Self { current_version: 1 }
    }

    pub fn needs_migration(&self, storage_version: u32) -> bool {
        storage_version < self.current_version
    }

    pub async fn migrate(&self, storage: Arc<dyn Storage>, from_version: u32) -> PacyteResult<()> {
        if from_version >= self.current_version {
            return Ok(());
        }

        info!("Migrating storage from v{} to v{}", from_version, self.current_version);

        let mut current = from_version;
        while current < self.current_version {
            match current {
                0 => {
                    info!("Running migration v0 -> v1: Adding founder vesting account");
                    self.migrate_v0_to_v1(storage.clone()).await?;
                }
                _ => {}
            }
            current += 1;
        }

        Ok(())
    }

    async fn migrate_v0_to_v1(&self, storage: Arc<dyn Storage>) -> PacyteResult<()> {
        use crate::types::{FOUNDER_VESTING_ADDRESS, FOUNDER_ALLOCATION};
        use crate::types::account::{Account, AccountType};

        let account_exists = storage.account_exists(&FOUNDER_VESTING_ADDRESS).await?;
        if !account_exists {
            let mut founder_vesting = Account::new(FOUNDER_VESTING_ADDRESS, FOUNDER_ALLOCATION);
            founder_vesting.account_type = AccountType::System;
            storage.save_account(&FOUNDER_VESTING_ADDRESS, &founder_vesting).await?;
            info!("Created founder vesting account during migration");
        }
        Ok(())
    }
}

impl Default for MigrationManager {
    fn default() -> Self {
        Self::new()
    }
}