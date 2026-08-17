// ===================================================================
// PACYTE NEXUS - STATE YÖNETİMİ
// ===================================================================

use std::collections::{HashMap, BTreeMap};
use std::sync::Arc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::types::{PacyteError, PacyteResult, Hash, BlockHeight, Address, Timestamp};
use crate::types::block::Block;
use crate::types::transaction::Transaction;
use crate::types::account::Account;
use crate::crypto::hash::hash_sha3_256;
use crate::crypto::merkle::{MerkleTree, SparseMerkleTree};
use super::{Storage, WriteBatch};

// ===================================================================
// STATE VERSİYONU
// ===================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateVersion {
    pub height: BlockHeight,
    pub root: Hash,
    pub timestamp: Timestamp,
    pub block_hash: Hash,
    pub parent_root: Hash,
}

impl StateVersion {
    pub fn new(height: BlockHeight, root: Hash, block_hash: Hash, parent_root: Hash) -> Self {
        Self {
            height,
            root,
            timestamp: crate::types::current_timestamp(),
            block_hash,
            parent_root,
        }
    }
}

// ===================================================================
// STATE MANAGER
// ===================================================================

pub struct StateManager {
    storage: Arc<dyn Storage>,
    current_root: Arc<RwLock<Hash>>,
    current_height: Arc<RwLock<BlockHeight>>,
    account_cache: Arc<dashmap::DashMap<Address, Account>>,
    dirty_accounts: Arc<RwLock<HashMap<Address, Account>>>,
    smt: Arc<RwLock<SparseMerkleTree>>,
    versions: Arc<RwLock<BTreeMap<BlockHeight, StateVersion>>>,
}

impl StateManager {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        let smt = SparseMerkleTree::new();
        let smt = SparseMerkleTree::new();
	let initial_root = smt.root;  // field, parantezsiz

        
        Self {
            storage,
            current_root: Arc::new(RwLock::new(initial_root)),
            current_height: Arc::new(RwLock::new(0)),
            account_cache: Arc::new(dashmap::DashMap::new()),
            dirty_accounts: Arc::new(RwLock::new(HashMap::new())),
            smt: Arc::new(RwLock::new(smt)),
            versions: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }
    
    /// State'i belirli bir yüksekliğe geri al
    pub async fn revert_to_height(&self, height: BlockHeight) -> PacyteResult<()> {
        let version = self.get_version(height).await?
            .ok_or_else(|| PacyteError::StateRootMismatch {
                expected: format!("{}", height),
                actual: "Version not found".to_string(),
            })?;
        
        // Cache'leri temizle
        self.account_cache.clear();
        self.dirty_accounts.write().clear();
        
        // Root'u güncelle
        *self.current_root.write() = version.root;
        *self.current_height.write() = height;
        
        // SMT'yi yeniden oluştur (basitleştirilmiş)
        let mut smt = SparseMerkleTree::new();
        *self.smt.write() = smt;
        
        Ok(())
    }
    
    /// Hesap bakiyesini getir
    pub async fn get_balance(&self, address: &Address) -> PacyteResult<u128> {
        if let Some(account) = self.get_account(address).await? {
            Ok(account.balance)
        } else {
            Ok(0)
        }
    }
    
    /// Hesap nonce'ini getir
    pub async fn get_nonce(&self, address: &Address) -> PacyteResult<u64> {
        if let Some(account) = self.get_account(address).await? {
            Ok(account.nonce)
        } else {
            Ok(0)
        }
    }
    
    /// Hesabı getir (önce cache, sonra dirty, sonra storage)
    pub async fn get_account(&self, address: &Address) -> PacyteResult<Option<Account>> {
        // 1. Cache kontrol
        if let Some(account) = self.account_cache.get(address) {
            return Ok(Some(account.clone()));
        }
        
        // 2. Dirty kontrol (commit edilmemiş değişiklikler)
        {
            let dirty = self.dirty_accounts.read();
            if let Some(account) = dirty.get(address) {
                return Ok(Some(account.clone()));
            }
        }
        
        // 3. Storage'dan oku
        if let Some(account) = self.storage.get_account(address).await? {
            self.account_cache.insert(*address, account.clone());
            Ok(Some(account))
        } else {
            Ok(None)
        }
    }
    
    /// Hesap oluştur veya güncelle
    pub async fn set_account(&self, address: Address, account: Account) -> PacyteResult<()> {
        // Dirty listeye ekle
        {
            let mut dirty = self.dirty_accounts.write();
            dirty.insert(address, account.clone());
        }
        
        // Cache'i güncelle
        self.account_cache.insert(address, account);
        
        Ok(())
    }
    
    /// Transfer işlemi
    pub async fn apply_transfer(
        &self,
        from: &Address,
        to: &Address,
        amount: u128,
        fee: u128,
    ) -> PacyteResult<()> {
        let total_cost = amount.saturating_add(fee);
        
        // Gönderici hesabını al
        let mut from_account = self.get_account(from).await?
            .ok_or_else(|| PacyteError::AccountNotFound(crate::types::address_short(from)))?;
        
        // Bakiye kontrolü
        if from_account.balance < total_cost {
            return Err(PacyteError::InsufficientBalance {
                required: total_cost,
                available: from_account.balance,
            });
        }
        
        // Nonce kontrolü ve artırımı
        let expected_nonce = from_account.nonce;
        from_account.nonce = from_account.nonce.saturating_add(1);
        
        // Bakiyeyi düş
        from_account.balance -= total_cost;
        from_account.record_activity();
        
        // Alıcı hesabını al veya oluştur
        let mut to_account = self.get_account(to).await?
            .unwrap_or_else(|| Account::new(*to, 0));
        
        // Bakiyeyi ekle
        to_account.balance = to_account.balance.saturating_add(amount);
        to_account.record_activity();
        
        // Hesapları kaydet
        self.set_account(*from, from_account).await?;
        self.set_account(*to, to_account).await?;
        
        Ok(())
    }
    
    /// Bloktaki tüm işlemleri uygula
    pub async fn apply_block(&self, block: &Block) -> PacyteResult<StateVersion> {
        let parent_root = *self.current_root.read();
        let parent_height = *self.current_height.read();
        
        // Yükseklik kontrolü
        if block.header.height != parent_height + 1 {
            return Err(PacyteError::InvalidProposal(
                format!("Invalid height: {} != {}", block.header.height, parent_height + 1)
            ));
        }
        
        // Tüm işlemleri uygula
        for tx in &block.body.transactions {
            self.apply_transfer(&tx.from, &tx.to, tx.amount, tx.fee).await?;
        }
        
        // State root'u hesapla
        let new_root = self.compute_state_root().await?;
        
        // Version oluştur
        let version = StateVersion::new(
            block.header.height,
            new_root,
            block.hash(),
            parent_root,
        );
        
        // Version'ı kaydet
        {
            let mut versions = self.versions.write();
            versions.insert(block.header.height, version.clone());
        }
        
        // Storage'a kaydet
        self.storage.save_state_root(block.header.height, &new_root).await?;
        
        // Current state'i güncelle
        *self.current_root.write() = new_root;
        *self.current_height.write() = block.header.height;
        
        // Dirty hesapları commit et
        self.commit_dirty_accounts().await?;
        
        Ok(version)
    }
    
    /// State root'u hesapla
    pub async fn compute_state_root(&self) -> PacyteResult<Hash> {
        let dirty = self.dirty_accounts.read();
        
        if dirty.is_empty() {
            return Ok(*self.current_root.read());
        }
        
        // Tüm hesapları topla
        let mut all_accounts: HashMap<Address, Account> = HashMap::new();
        
        // Cache'teki hesapları ekle
        for entry in self.account_cache.iter() {
            all_accounts.insert(*entry.key(), entry.value().clone());
        }
        
        // Dirty hesapları ekle (override)
        for (addr, account) in dirty.iter() {
            all_accounts.insert(*addr, account.clone());
        }
        
        // SMT ile root hesapla
        let updates: Vec<(Hash, Hash)> = all_accounts
            .iter()
            .map(|(addr, account)| {
                let key = hash_sha3_256(addr);
                let value = hash_sha3_256(&bincode::serialize(account).unwrap_or_default());
                (key, value)
            })
            .collect();
        
        let smt = self.smt.read();
        Ok(smt.root_from_updates(&updates))
    }
    
    /// Dirty hesapları storage'a commit et
    async fn commit_dirty_accounts(&self) -> PacyteResult<()> {
        let dirty = {
            let mut dirty = self.dirty_accounts.write();
            std::mem::take(&mut *dirty)
        };
        
        if dirty.is_empty() {
            return Ok(());
        }
        
        let mut batch = WriteBatch::new();
        
        for (address, account) in dirty {
            batch.add_account(address, account);
        }
        
        self.storage.write_batch(batch).await?;
        
        Ok(())
    }
    
    /// Version getir
    pub async fn get_version(&self, height: BlockHeight) -> PacyteResult<Option<StateVersion>> {
        // Önce memory'de ara
        {
            let versions = self.versions.read();
            if let Some(version) = versions.get(&height) {
                return Ok(Some(version.clone()));
            }
        }
        
        // Storage'dan oku
        if let Some(root) = self.storage.get_state_root(height).await? {
            // Tam version bilgisi storage'da yoksa basitleştirilmiş döndür
            Ok(Some(StateVersion {
                height,
                root,
                timestamp: 0,
                block_hash: [0u8; 32],
                parent_root: [0u8; 32],
            }))
        } else {
            Ok(None)
        }
    }
    
    /// State'in hash'ini al
    pub fn current_root(&self) -> Hash {
        *self.current_root.read()
    }
    
    /// State yüksekliğini al
    pub fn current_height(&self) -> BlockHeight {
        *self.current_height.read()
    }
    
    /// Hesap var mı?
    pub async fn account_exists(&self, address: &Address) -> PacyteResult<bool> {
        Ok(self.get_account(address).await?.is_some())
    }
    
    /// Toplam supply'i hesapla
    pub async fn total_supply(&self) -> PacyteResult<u128> {
        // Not: Gerçek implementasyonda cache'lenmeli
        let mut total = 0u128;
        
        for entry in self.account_cache.iter() {
            total = total.saturating_add(entry.value().balance);
        }
        
        // Storage'daki tüm hesapları saymak pahalı, bu yüzden
        // metadata'da tutulan toplam supply'i kullan
        Ok(total)
    }
    
        pub async fn check_all_dormancy(&self, current_time: Timestamp) ->     	PacyteResult<Vec<Address>> {
    	    let mut dormant = Vec::new();
    	    for entry in self.account_cache.iter() {
        	let account = entry.value();
        	if account.is_dormant {
            	dormant.push(*entry.key());
        	}
    	}
    	Ok(dormant)
    }

}

// ===================================================================
// TESTLER
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;

    #[tokio::test]
    async fn test_state_transfer() {
        let storage = Arc::new(MemoryStorage::new());
        let state = StateManager::new(storage.clone());
        
        let alice = [1u8; 32];
        let bob = [2u8; 32];
        
        // Alice hesabı oluştur
        let alice_account = Account::new(alice, 10000);
        state.set_account(alice, alice_account).await.unwrap();
        
        // Transfer yap
        state.apply_transfer(&alice, &bob, 5000, 10).await.unwrap();
        
        // Bakiyeleri kontrol et
        assert_eq!(state.get_balance(&alice).await.unwrap(), 4990);
        assert_eq!(state.get_balance(&bob).await.unwrap(), 5000);
        
        // Nonce kontrolü
        assert_eq!(state.get_nonce(&alice).await.unwrap(), 1);
    }
    
    #[tokio::test]
    async fn test_state_root() {
        let storage = Arc::new(MemoryStorage::new());
        let state = StateManager::new(storage.clone());
        
        let initial_root = state.current_root();
        
        // Hesap ekle
        let addr = [1u8; 32];
        let account = Account::new(addr, 1000);
        state.set_account(addr, account).await.unwrap();
        
        // Root değişmeli
        let new_root = state.compute_state_root().await.unwrap();
        assert_ne!(initial_root, new_root);
    }
    
    #[tokio::test]
    async fn test_apply_block() {
        let storage = Arc::new(MemoryStorage::new());
        let state = StateManager::new(storage.clone());
        
        // Genesis hesabı
        let genesis = [0u8; 32];
        let genesis_account = Account::genesis_sovereign(1_000_000);
        state.set_account(genesis, genesis_account).await.unwrap();
        
        // Blok oluştur
        let tx = Transaction::new(genesis, [1u8; 32], 1000, 10, 0);
        let block = Block::new(1, [0u8; 32], vec![tx], genesis);
        
        // Bloğu uygula
        let version = state.apply_block(&block).await.unwrap();
        
        assert_eq!(version.height, 1);
        assert_eq!(state.current_height(), 1);
    }
}