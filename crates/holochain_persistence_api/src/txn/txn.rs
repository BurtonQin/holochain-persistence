/// Transactional trait extensions to the CAS and EAV persistence
use crate::{
    cas::{
        content::{Address, AddressableContent, Content},
        storage::ContentAddressableStorage,
    },
    eav::*,
    error::*,
    reporting::{ReportStorage, StorageReport},
};
use std::{collections::BTreeSet, marker::PhantomData};
use uuid::Uuid;

/// Defines a transactional writer, typically implemented over a cursor.
pub trait Writer {
    /// Commits the transaction. Returns a `PersistenceError` if the
    /// transaction does not succeed.
    fn commit(self) -> PersistenceResult<()>;
}

/// Cursor interface over both CAS and EAV databases. Provides transactional support
/// by providing a `Writer` across both of them.
pub trait Cursor<A: Attribute>:
    Writer + ContentAddressableStorage + EntityAttributeValueStorage<A>
{
}

// TODO Should cursor's even be cloneable? SPIKE this
clone_trait_object!(<A:Attribute> Cursor<A>);

/// A write that does nothing, for testing or for
/// impementations that don't require the commit and abort functions
/// to do anything critical.
#[derive(Clone)]
pub struct NoopWriter;

impl NoopWriter {
    pub fn new() -> Self {
        NoopWriter
    }
}

impl Writer for NoopWriter {
    fn commit(self) -> PersistenceResult<()> {
        Ok(())
    }
}

/// A default cursor that does not execute anything explicitly within transactions.
#[derive(Clone, Debug)]
pub struct NonTransactionalCursor<
    A: Attribute,
    CAS: ContentAddressableStorage,
    EAV: EntityAttributeValueStorage<A>,
> {
    cas: CAS,
    eav: EAV,
    phantom: PhantomData<A>,
}

impl<
        A: Attribute,
        CAS: ContentAddressableStorage + Clone,
        EAV: EntityAttributeValueStorage<A> + Clone,
    > ContentAddressableStorage for NonTransactionalCursor<A, CAS, EAV>
{
    fn add(&self, content: &dyn AddressableContent) -> PersistenceResult<()> {
        self.cas.add(content)
    }

    fn fetch(&self, address: &Address) -> PersistenceResult<Option<Content>> {
        self.cas.fetch(address)
    }

    fn contains(&self, address: &Address) -> PersistenceResult<bool> {
        self.cas.contains(address)
    }

    fn get_id(&self) -> Uuid {
        self.cas.get_id()
    }
}

impl<
        A: Attribute,
        CAS: ContentAddressableStorage + Clone,
        EAV: EntityAttributeValueStorage<A> + Clone,
    > EntityAttributeValueStorage<A> for NonTransactionalCursor<A, CAS, EAV>
{
    fn add_eavi(
        &self,
        eavi: &EntityAttributeValueIndex<A>,
    ) -> PersistenceResult<Option<EntityAttributeValueIndex<A>>> {
        self.eav.add_eavi(eavi)
    }

    fn fetch_eavi(
        &self,
        query: &EaviQuery<A>,
    ) -> PersistenceResult<BTreeSet<EntityAttributeValueIndex<A>>> {
        self.eav.fetch_eavi(query)
    }
}

impl<
        A: Attribute,
        CAS: ContentAddressableStorage + Clone,
        EAV: EntityAttributeValueStorage<A> + Clone,
    > ReportStorage for NonTransactionalCursor<A, CAS, EAV>
{
    fn get_storage_report(&self) -> PersistenceResult<StorageReport> {
        self.cas.get_storage_report()
    }
}

impl<
        A: Attribute,
        CAS: ContentAddressableStorage + Clone,
        EAV: EntityAttributeValueStorage<A> + Clone,
    > Cursor<A> for NonTransactionalCursor<A, CAS, EAV>
{
}

impl<
        A: Attribute,
        CAS: ContentAddressableStorage + Clone,
        EAV: EntityAttributeValueStorage<A> + Clone,
    > Writer for NonTransactionalCursor<A, CAS, EAV>
{
    fn commit(self) -> PersistenceResult<()> {
        NoopWriter {}.commit()
    }
}

impl<
        A: Attribute,
        CAS: ContentAddressableStorage + Clone,
        EAV: EntityAttributeValueStorage<A> + Clone,
    > CursorProvider<A> for NonTransactionalCursor<A, CAS, EAV>
{
    type Cursor = Self;

    fn create_cursor(&self) -> PersistenceResult<Self::Cursor> {
        Ok(self.clone())
    }
}

/// Creates cursors over both EAV and CAS instances. May acquire read or write
/// resources to do so, depending on implementation.
///
/// Some cursors may cascade over temporary (aka "scratch") databases to improve
/// concurrency performance.
///
/// An advanced cursor might wrap other cursors and check external resources (such as peer data in a network)
/// in addition to the cursors it wraps.
pub trait CursorProvider<A: Attribute> {
    /// The type of a cursor for this cursor provider
    type Cursor: Cursor<A>;

    /// Creates a new cursor. Use carefully as one instance of a cursor may block another,
    /// especially when cursors are mutating the primary store.
    fn create_cursor(&self) -> PersistenceResult<Self::Cursor>;
}

/// A high level api which brings together a CAS, EAV, and
/// Cursor over them both. A cursor may start transactions over both
/// the stores or not, depending on implementation.
pub trait PersistenceManager<A: Attribute>: CursorProvider<A> {
    /// The type of Content Addressable Storage (CAS)
    type Cas: ContentAddressableStorage;
    /// The type of Entity Entity AttributeValue Storage (EAV)
    type Eav: EntityAttributeValueStorage<A>;

    /// Gets the CAS storage.
    fn cas(&self) -> Self::Cas;
    /// Gets the EAV storage
    fn eav(&self) -> Self::Eav;
}

/// Provides a simple, extensable version of a persistance manager. Intended
/// to be specialized for a particular database implementation easily.
pub struct DefaultPersistenceManager<
    A: Attribute,
    CAS: ContentAddressableStorage + Clone,
    EAV: EntityAttributeValueStorage<A> + Clone,
    CP: CursorProvider<A>,
> {
    cas: CAS,
    eav: EAV,
    cursor_provider: CP,
    phantom: PhantomData<A>,
}

impl<
        A: Attribute,
        CAS: ContentAddressableStorage + Clone,
        EAV: EntityAttributeValueStorage<A> + Clone,
        CP: CursorProvider<A>,
    > DefaultPersistenceManager<A, CAS, EAV, CP>
{
    pub fn new(cas: CAS, eav: EAV, cursor_provider: CP) -> Self {
        Self {
            cas: cas,
            eav: eav,
            cursor_provider,
            phantom: PhantomData,
        }
    }
}

impl<
        A: Attribute,
        CAS: ContentAddressableStorage + Clone,
        EAV: EntityAttributeValueStorage<A> + Clone,
        CP: CursorProvider<A>,
    > PersistenceManager<A> for DefaultPersistenceManager<A, CAS, EAV, CP>
{
    type Cas = CAS;
    type Eav = EAV;
    fn eav(&self) -> Self::Eav {
        self.eav.clone()
    }

    fn cas(&self) -> Self::Cas {
        self.cas.clone()
    }
}

impl<
        A: Attribute,
        CAS: ContentAddressableStorage + Clone,
        EAV: EntityAttributeValueStorage<A> + Clone,
        CP: CursorProvider<A>,
    > CursorProvider<A> for DefaultPersistenceManager<A, CAS, EAV, CP>
{
    type Cursor = CP::Cursor;

    fn create_cursor(&self) -> PersistenceResult<Self::Cursor> {
        self.cursor_provider.create_cursor()
    }
}
