use crate::{
    cas::lmdb::LmdbStorage,
    common::LmdbInstance,
    eav::lmdb::EavLmdbStorage,
    error::{is_store_full_error, is_store_full_result, to_api_error},
};
use holochain_persistence_api::{
    cas::{content::*, storage::*},
    eav::*,
    error::*,
    reporting::{ReportStorage, StorageReport},
    txn::{Cursor, CursorProvider, DefaultPersistenceManager},
};
use rkv::EnvironmentFlags;
use serde::de::DeserializeOwned;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// A cursor over an lmdb environment
#[derive(Clone, Debug)]
pub struct EnvCursor<A: Attribute> {
    cas_db: LmdbStorage,
    eav_db: EavLmdbStorage<A>,
    staging_cas_db: LmdbStorage,
    staging_eav_db: EavLmdbStorage<A>,
}

impl<A: Attribute + Sync + Send + DeserializeOwned> EnvCursor<A> {
    /// Internal commit function which extracts `StoreError::MapFull` into the success value of
    /// a result where `true` indicates the commit is successful, and `false` means the map was
    /// full and retry is required with the newly allocated map size.
    fn commit_internal(&self) -> PersistenceResult<bool> {
        let env_lock = self.cas_db.lmdb.rkv().write().unwrap();
        let mut writer = env_lock.write().unwrap();

        let staging_env_lock = self.staging_cas_db.lmdb.rkv().read().unwrap();
        let staging_reader = staging_env_lock.read().map_err(to_api_error)?;

        let staged_cas_data = self
            .staging_cas_db
            .lmdb_iter(&staging_reader)
            .map_err(to_api_error)?;

        for (_address, maybe_content) in staged_cas_data {
            let result = maybe_content
                .as_ref()
                .map(|content| self.cas_db.lmdb_add(&mut writer, content))
                .unwrap_or_else(|| Ok(()));
            if is_store_full_result(&result) {
                let map_size = env_lock.info().map_err(to_api_error)?.map_size();
                env_lock.set_map_size(map_size * 2).map_err(to_api_error)?;
                return Ok(false);
            }
            result.map_err(to_api_error)?;
        }

        let staged_eav_data = self.staging_eav_db.fetch_eavi(&EaviQuery::default())?;

        for eavi in staged_eav_data {
            let result = self.eav_db.add_lmdb_eavi(&mut writer, &eavi);
            if is_store_full_result(&result) {
                let map_size = env_lock.info().map_err(to_api_error)?.map_size();
                env_lock.set_map_size(map_size * 2).map_err(to_api_error)?;
                return Ok(false);
            }
            result.map_err(to_api_error)?;
        }

        writer.commit().map(|()| Ok(true)).unwrap_or_else(|e| {
            if is_store_full_error(&e) {
                let map_size = env_lock.info().map_err(to_api_error)?.map_size();
                env_lock.set_map_size(map_size * 2).map_err(to_api_error)?;
                Ok(false)
            } else {
                Err(to_api_error(e))
            }
        })
    }
}

impl<A: Attribute + Sync + Send + DeserializeOwned> holochain_persistence_api::txn::Writer
    for EnvCursor<A>
{
    fn commit(self) -> PersistenceResult<()> {
        loop {
            let committed = self.commit_internal()?;
            if committed {
                return Ok(());
            }
        }
    }
}

impl<A: Attribute> ReportStorage for EnvCursor<A> {
    fn get_storage_report(&self) -> PersistenceResult<StorageReport> {
        self.cas_db.get_storage_report()
    }
}

impl<A: Attribute> EnvCursor<A> {
    pub fn new(
        cas_db: LmdbStorage,
        eav_db: EavLmdbStorage<A>,
        staging_cas_db: LmdbStorage,
        staging_eav_db: EavLmdbStorage<A>,
    ) -> Self {
        Self {
            cas_db,
            eav_db,
            staging_cas_db,
            staging_eav_db,
        }
    }
}

impl<A: Attribute> ContentAddressableStorage for EnvCursor<A> {

    /// Adds `content` only to the staging CAS database. Use `commit()` to write to the
    /// primary.
    fn add(&self, content: &dyn AddressableContent) -> PersistenceResult<()> {
        self.staging_cas_db.add(content)
    }

    fn contains(&self, address: &Address) -> PersistenceResult<bool> {
        self.fetch(address)
            .map(|maybe_content| maybe_content.is_some())
    }

    /// First try the staging CAS database, then the primary. Cache the results from the 
    /// primary into the staging database.
    fn fetch(&self, address: &Address) -> PersistenceResult<Option<Content>> {
        let maybe_content = self.staging_cas_db.fetch(address)?;

        if maybe_content.is_some() {
            return Ok(maybe_content);
        }

        let maybe_content = self.cas_db.fetch(address)?;

        if let Some(content) = maybe_content {
            self.staging_cas_db.add(&content)?;
            Ok(Some(content))
        } else {
            Ok(None)
        }
    }

    fn get_id(&self) -> uuid::Uuid {
        self.cas_db.get_id()
    }
}

impl<A: Attribute + serde::de::DeserializeOwned> EntityAttributeValueStorage<A> for EnvCursor<A> {
    /// Adds `content` only to the staging EAVI database. Use `commit()` to write to the
    /// primary.
    fn add_eavi(
        &self,
        eav: &EntityAttributeValueIndex<A>,
    ) -> PersistenceResult<Option<EntityAttributeValueIndex<A>>> {
        self.staging_eav_db
            .resizable_add_lmdb_eavi(eav)
            .map_err(to_api_error)
    }

    /// First query the staging EAVI database, then the primary. Cache the results from the 
    /// primary into the staging database.
     fn fetch_eavi(
        &self,
        query: &EaviQuery<A>,
    ) -> PersistenceResult<BTreeSet<EntityAttributeValueIndex<A>>> {
        let eavis = self.staging_eav_db.fetch_eavi(query)?;

        if !eavis.is_empty() {
            return Ok(eavis);
        }

        let eavis = self.eav_db.fetch_eavi(query)?;

        for eavi in &eavis {
            self.staging_cas_db.add(eavi)?;
        }
        Ok(eavis)
    }
}

impl<A: Attribute + DeserializeOwned> Cursor<A> for EnvCursor<A> {}

#[derive(Clone)]
pub struct LmdbCursorProvider<A: Attribute> {
    /// Primary CAS lmdb store
    cas_db: LmdbStorage,

    /// Primary EAV lmdb store, transactionally linked to the CAS
    eav_db: EavLmdbStorage<A>,

    /// Path prefix to generate staging databases
    staging_path_prefix: PathBuf,

    /// Initial map size of staging databases
    staging_initial_map_size: Option<usize>,

    /// Environment flags for staging databases.
    staging_env_flags: Option<EnvironmentFlags>,
}

/// Name of CAS staging database
const STAGING_CAS_BUCKET: &str = "staging_cas";

/// Name of EAV staging database
const STAGING_EAV_BUCKET: &str = "staging_eav";

impl<A: Attribute + DeserializeOwned> CursorProvider<A> for LmdbCursorProvider<A> {
    type Cursor = EnvCursor<A>;
    fn create_cursor(&self) -> PersistenceResult<Self::Cursor> {
        let db_names = vec![STAGING_CAS_BUCKET, STAGING_EAV_BUCKET];

        let mut staging_path = self.staging_path_prefix.clone();
        staging_path.push(format!("{}", Uuid::new_v4()));

        // TODO do we need this if the environment flags are set correctly? That is, it should just
        // be an in memory only database with no file system handles?
        fs::create_dir_all(staging_path.as_path())?;
        let staging_dbs = LmdbInstance::new_all(
            db_names.as_slice(),
            staging_path,
            self.staging_initial_map_size,
            self.staging_env_flags,
        );

        let staging_cas_db =
            LmdbStorage::wrap(staging_dbs.get(&STAGING_CAS_BUCKET.to_string()).unwrap());
        let staging_eav_db =
            EavLmdbStorage::wrap(staging_dbs.get(&STAGING_EAV_BUCKET.to_string()).unwrap());

        Ok(EnvCursor::new(
            self.cas_db.clone(),
            self.eav_db.clone(),
            staging_cas_db,
            staging_eav_db,
        ))
    }
}

pub type LmdbManager<A> = DefaultPersistenceManager<A, LmdbStorage, EavLmdbStorage<A>, LmdbCursorProvider<A>>;

pub fn new_manager<
    A: Attribute + DeserializeOwned,
    EP: AsRef<Path> + Clone,
    SP: AsRef<Path> + Clone,
>(
    env_path: EP,
    staging_path_prefix: SP,
    initial_map_size: Option<usize>,
    env_flags: Option<EnvironmentFlags>,
    staging_initial_map_size: Option<usize>,
    staging_env_flags: Option<EnvironmentFlags>,
) -> LmdbManager<A> {
    let cas_db_name = crate::cas::lmdb::CAS_BUCKET;
    let eav_db_name = crate::eav::lmdb::EAV_BUCKET;
    let db_names = vec![cas_db_name, eav_db_name];

    let dbs = LmdbInstance::new_all(db_names.as_slice(), env_path, initial_map_size, env_flags);

    let cas_db = LmdbStorage::wrap(dbs.get(&cas_db_name.to_string()).unwrap());
    let eav_db: EavLmdbStorage<A> =
        EavLmdbStorage::wrap(dbs.get(&eav_db_name.to_string()).unwrap());

    let cursor_provider = LmdbCursorProvider {
        cas_db: cas_db.clone(),
        eav_db: eav_db.clone(),
        staging_path_prefix: staging_path_prefix.as_ref().to_path_buf(),
        staging_initial_map_size,
        staging_env_flags,
    };

    DefaultPersistenceManager::new(cas_db, eav_db, cursor_provider)
}
