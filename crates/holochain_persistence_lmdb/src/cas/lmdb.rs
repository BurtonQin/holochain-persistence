use crate::common::LmdbInstance;
use holochain_json_api::json::JsonString;
use holochain_persistence_api::{
    cas::{
        content::{Address, AddressableContent, Content},
        storage::ContentAddressableStorage,
    },
    error::{PersistenceError, PersistenceResult},
    reporting::{ReportStorage, StorageReport},
};
use rkv::{
    error::{DataError, StoreError},
    Value,
};
use std::{
    fmt::{Debug, Error, Formatter},
    path::Path,
};
use uuid::Uuid;
use holochain_persistence_api::txn::{WriterProvider, Writer, CasEavManager};

const CAS_BUCKET: &str = "cas";

#[derive(Clone)]
pub struct LmdbStorage {
    id: Uuid,
    lmdb: LmdbInstance,
}


impl Debug for LmdbStorage {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        f.debug_struct("LmdbStorage").field("id", &self.id).finish()
    }
}


#[derive(Shrinkwrap)]
pub struct LmdbWriter<'txn>(rkv::Writer<'txn>);



impl<'txn, A:Attribute> WriterProvider for CasEavManager<LmbdbStorage, {
    type Writer = LmdbWriter<'txn>;

}

impl LmdbStorage {
    pub fn new<P: AsRef<Path> + Clone>(
        db_path: P,
        initial_map_bytes: Option<usize>,
    ) -> LmdbStorage {
        LmdbStorage {
            id: Uuid::new_v4(),
            lmdb: LmdbInstance::new(CAS_BUCKET, db_path, initial_map_bytes),
        }
    }
}

impl LmdbStorage {
    fn lmdb_add<'env>(&mut self, writer: rkv::Writer<'env>, content: &dyn AddressableContent) -> Result<(), StoreError> {
        self.lmdb.add(
            content.address(),
            &Value::Json(&content.content().to_string()),
        )
    }

    fn lmdb_fetch(&self, reader: rkv::Reader, address: &Address) -> Result<Option<Content>, StoreError> {

        match self.lmdb.store.get(&reader, address.clone()) {
            Ok(Some(value)) => match value {
                Value::Json(s) => Ok(Some(JsonString::from_json(s))),
                _ => Err(StoreError::DataError(DataError::Empty)),
            },
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

impl ContentAddressableStorage for LmdbStorage {
    fn add(&mut self, content: &dyn AddressableContent) -> PersistenceResult<()> {
        self.lmdb_add(content)
            .map_err(|e| PersistenceError::from(format!("CAS add error: {}", e)))
    }

    fn contains(&self, address: &Address) -> PersistenceResult<bool> {
        self.fetch(address).map(|result| match result {
            Some(_) => true,
            None => false,
        })
    }

    fn fetch(&self, address: &Address) -> PersistenceResult<Option<Content>> {
        self.lmdb_fetch(address)
            .map_err(|e| PersistenceError::from(format!("CAS fetch error: {}", e)))
    }

    fn get_id(&self) -> Uuid {
        self.id
    }
}

impl ReportStorage for LmdbStorage {
    fn get_storage_report(&self) -> PersistenceResult<StorageReport> {
        Ok(StorageReport::new(0)) // TODO: implement this
    }
}

#[cfg(test)]
mod tests {
    use crate::cas::lmdb::LmdbStorage;
    use holochain_json_api::json::RawString;
    use holochain_persistence_api::{
        cas::{
            content::{Content, ExampleAddressableContent, OtherExampleAddressableContent},
            storage::{CasBencher, ContentAddressableStorage, StorageTestSuite},
        },
        reporting::{ReportStorage, StorageReport},
    };
    use tempfile::{tempdir, TempDir};

    pub fn test_lmdb_cas() -> (LmdbStorage, TempDir) {
        let dir = tempdir().expect("Could not create a tempdir for CAS testing");
        (LmdbStorage::new(dir.path(), None), dir)
    }

    #[bench]
    fn bench_lmdb_cas_add(b: &mut test::Bencher) {
        let (store, _) = test_lmdb_cas();
        CasBencher::bench_add(b, store);
    }

    #[bench]
    fn bench_lmdb_cas_fetch(b: &mut test::Bencher) {
        let (store, _) = test_lmdb_cas();
        CasBencher::bench_fetch(b, store);
    }

    #[test]
    /// show that content of different types can round trip through the same storage
    /// this is copied straight from the example with a file CAS
    fn lmdb_content_round_trip_test() {
        let (cas, _dir) = test_lmdb_cas();
        let test_suite = StorageTestSuite::new(cas);
        test_suite.round_trip_test::<ExampleAddressableContent, OtherExampleAddressableContent>(
            RawString::from("foo").into(),
            RawString::from("bar").into(),
        );
    }

    #[test]
    fn lmdb_report_storage_test() {
        let (mut cas, _) = test_lmdb_cas();
        // add some content
        cas.add(&Content::from_json("some bytes"))
            .expect("could not add to CAS");
        assert_eq!(cas.get_storage_report().unwrap(), StorageReport::new(0),);

        // add some more
        cas.add(&Content::from_json("more bytes"))
            .expect("could not add to CAS");
        assert_eq!(cas.get_storage_report().unwrap(), StorageReport::new(0 + 0),);
    }
}
