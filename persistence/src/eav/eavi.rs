//! EAV stands for entity-attribute-value. It is a pattern implemented here
//! for adding metadata about entries in the DHT, additionally
//! being used to define relationships between AddressableContent values.
//! See [wikipedia](https://en.wikipedia.org/wiki/Entity%E2%80%93attribute%E2%80%93value_model) to learn more about this pattern.

use crate::{
    cas::content::{Address, AddressableContent, Content},
    error::{HcResult, HolochainError},
    json::JsonString,
};
use chrono::offset::Utc;
use eav::{
    query::{EaviQuery, IndexFilter},
    storage::{EntityAttributeValueStorage, ExampleEntityAttributeValueStorage},
};
use json::DefaultJson;
use std::{
    cmp::Ordering,
    collections::BTreeSet,
    convert::{TryFrom, TryInto},
    fmt,
    option::NoneError,
};

/// Address of AddressableContent representing the EAV entity
pub type Entity = Address;

/// All Attribute values are pre-defined here. If ever a user-defined Attribute is needed,
/// just add a new Custom variant for it with a String parameter
//#[derive(PartialEq, Eq, PartialOrd, Hash, Clone, Debug, Serialize, Deserialize, DefaultJson)]
//#[serde(rename_all = "snake_case")]
pub trait Attribute:
    PartialEq + Eq + PartialOrd + std::hash::Hash + Clone + fmt::Debug + TryFrom<String> + Into<String>
{
}

#[derive(PartialEq, Eq, PartialOrd, Hash, Clone, Debug, Serialize, Deserialize, DefaultJson)]
pub enum ExampleAttribute {
    WithoutPayload,
    WithPayload(String),
}

impl TryFrom<String> for ExampleAttribute {
    type Error = AttributeError;
    fn try_from(s: String) -> Result<Self, AttributeError> {
        match s.as_str() {
            "without-payload" => Ok(ExampleAttribute::WithoutPayload),
            with_payload => {
                let parts: Vec<&str> = with_payload.split("-").collect();
                if parts.len() < 2 {
                    return Err(AttributeError::ParseError);
                }
                Ok(ExampleAttribute::WithPayload(String::from(parts[1])))
            }
        }
    }
}

impl Attribute for ExampleAttribute {}

#[derive(PartialEq, Debug)]
pub enum AttributeError {
    Unrecognized(String),
    ParseError,
}

impl From<AttributeError> for HolochainError {
    fn from(err: AttributeError) -> HolochainError {
        let msg = match err {
            AttributeError::Unrecognized(a) => format!("Unknown attribute: {}", a),
            AttributeError::ParseError => {
                String::from("Could not parse attribute, bad regex match")
            }
        };
        HolochainError::ErrorGeneric(msg)
    }
}
impl From<NoneError> for AttributeError {
    fn from(_: NoneError) -> AttributeError {
        AttributeError::ParseError
    }
}

/// Address of AddressableContent representing the EAV value
pub type Value = Address;

// @TODO do we need this?
// unique (local to the source) monotonically increasing number that can be used for crdt/ordering
// @see https://papers.radixdlt.com/tempo/#logical-clocks
pub type Index = i64;

// @TODO do we need this?
// source agent asserting the meta
// type Source ...
/// The basic struct for EntityAttributeValue triple, implemented as AddressableContent
/// including the necessary serialization inherited.
#[derive(PartialEq, Eq, Hash, Clone, Debug, Serialize, Deserialize)]
pub struct EntityAttributeValueIndex<A: Attribute> {
    entity: Entity,
    attribute: A,
    value: Value,
    index: Index,
    // source: Source,
}

impl<A: Attribute> DefaultJson for EntityAttributeValueIndex<A> {

}

impl<A: Attribute> PartialOrd for EntityAttributeValueIndex<A> {
    fn partial_cmp(&self, other: &EntityAttributeValueIndex<A>) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<A:Attribute> Ord for EntityAttributeValueIndex<A> {
    fn cmp(&self, other: &EntityAttributeValueIndex<A>) -> Ordering {
        self.index.cmp(&other.index())
    }
}

impl<A:Attribute> AddressableContent for EntityAttributeValueIndex<A> {
    fn content(&self) -> Content {
        self.to_owned().into()
    }

    fn try_from_content(content: &Content) -> Result<Self, HolochainError> {
        content.to_owned().try_into()
    }
}

impl<A:Attribute> EntityAttributeValueIndex<A> {
    pub fn new(
        entity: &Entity,
        attribute: &A,
        value: &Value,
    ) -> HcResult<EntityAttributeValueIndex<A>> {
        Ok(EntityAttributeValueIndex {
            entity: entity.clone(),
            attribute: attribute.clone(),
            value: value.clone(),
            index: Utc::now().timestamp_nanos(),
        })
    }

    pub fn new_with_index(
        entity: &Entity,
        attribute: &A,
        value: &Value,
        timestamp: i64,
    ) -> HcResult<EntityAttributeValueIndex<A>> {
        Ok(EntityAttributeValueIndex {
            entity: entity.clone(),
            attribute: attribute.clone(),
            value: value.clone(),
            index: timestamp,
        })
    }

    pub fn entity(&self) -> Entity {
        self.entity.clone()
    }

    pub fn attribute(&self) -> A {
        self.attribute.clone()
    }

    pub fn value(&self) -> Value {
        self.value.clone()
    }

    pub fn index(&self) -> Index {
        self.index
    }

    pub fn set_index(&mut self, new_index: i64) {
        self.index = new_index
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, DefaultJson)]
pub struct ExampleEntry {
    pub data: String,
}

impl AddressableContent for ExampleEntry {
    fn address(&self) -> Address {
        Address::from(self.data.clone())
    }

    fn content(&self) -> Content {
        self.into()
    }

    fn try_from_content(content: &Content) -> HcResult<ExampleEntry> {
        ExampleEntry::try_from(content.to_owned())
    }
}

impl ExampleEntry {
    pub fn new(data: String) -> Self {
        Self { data }
    }
}

pub fn test_entry_a() -> ExampleEntry {
    ExampleEntry::new(String::from("a"))
}

pub fn test_entry_b() -> ExampleEntry {
    ExampleEntry::new(String::from("b"))
}

pub fn test_eav_entity() -> ExampleEntry {
    test_entry_a()
}

pub fn test_eav_attribute() -> ExampleAttribute {
    ExampleAttribute::WithPayload("foo-attribute".into())
}

pub fn test_eav_value() -> ExampleEntry {
    test_entry_b()
}

pub fn test_eav() -> EntityAttributeValueIndex<ExampleAttribute> {
    EntityAttributeValueIndex::new_with_index(
        &test_eav_entity().address(),
        &test_eav_attribute(),
        &test_eav_value().address(),
        0,
    )
    .expect("Could not create eav")
}

pub fn test_eav_content() -> Content {
    test_eav().content()
}

pub fn test_eav_address() -> Address {
    test_eav().address()
}

pub fn eav_round_trip_test_runner<A:Attribute>(
    entity_content: impl AddressableContent + Clone,
    attribute: A,
    value_content: impl AddressableContent + Clone,
) {
    let eav = EntityAttributeValueIndex::new(
        &entity_content.address(),
        &attribute,
        &value_content.address(),
    )
    .expect("Could not create EAV");
    let mut eav_storage = ExampleEntityAttributeValueStorage::new();

    assert_eq!(
        BTreeSet::new(),
        eav_storage
            .fetch_eavi(&EaviQuery::new(
                Some(entity_content.address()).into(),
                Some(attribute.clone()).into(),
                Some(value_content.address()).into(),
                IndexFilter::LatestByAttribute
            ))
            .expect("could not fetch eav"),
    );

    eav_storage.add_eavi(&eav).expect("could not add eav");

    let mut expected = BTreeSet::new();
    expected.insert(eav.clone());
    // some examples of constraints that should all return the eav
    for (e, a, v) in vec![
        // constrain all
        (
            Some(entity_content.address()),
            Some(attribute.clone()),
            Some(value_content.address()),
        ),
        // open entity
        (None, Some(attribute.clone()), Some(value_content.address())),
        // open attribute
        (
            Some(entity_content.address()),
            None,
            Some(value_content.address()),
        ),
        // open value
        (
            Some(entity_content.address()),
            Some(attribute.clone()),
            None,
        ),
        // open
        (None, None, None),
    ] {
        assert_eq!(
            expected,
            eav_storage
                .fetch_eavi(&EaviQuery::new(
                    e.into(),
                    a.into(),
                    v.into(),
                    IndexFilter::LatestByAttribute
                ))
                .expect("could not fetch eav")
        );
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        cas::{
            content::{AddressableContent, AddressableContentTestSuite, ExampleAddressableContent},
            storage::{
                test_content_addressable_storage, EavTestSuite, ExampleContentAddressableStorage,
            },
        },
        eav::EntityAttributeValueIndex,
        json::RawString,
    };

    pub fn test_eav_storage() -> ExampleEntityAttributeValueStorage {
        ExampleEntityAttributeValueStorage::new()
    }

    #[test]
    fn example_eav_round_trip() {
        let eav_storage = test_eav_storage();
        let entity =
            ExampleAddressableContent::try_from_content(&JsonString::from(RawString::from("foo")))
                .unwrap();
        let attribute = "favourite-color".to_string();
        let value =
            ExampleAddressableContent::try_from_content(&JsonString::from(RawString::from("blue")))
                .unwrap();

        EavTestSuite::test_round_trip(eav_storage, entity, attribute, value)
    }

    #[test]
    fn example_eav_one_to_many() {
        EavTestSuite::test_one_to_many::<
            ExampleAddressableContent,
            ExampleEntityAttributeValueStorage,
        >(test_eav_storage());
    }

    #[test]
    fn example_eav_many_to_one() {
        EavTestSuite::test_many_to_one::<
            ExampleAddressableContent,
            ExampleEntityAttributeValueStorage,
        >(test_eav_storage());
    }

    #[test]
    fn example_eav_range() {
        EavTestSuite::test_range::<ExampleAddressableContent, ExampleEntityAttributeValueStorage>(
            test_eav_storage(),
        );
    }

    #[test]
    fn example_eav_prefixes() {
        EavTestSuite::test_multiple_attributes::<
            ExampleAddressableContent,
            ExampleEntityAttributeValueStorage,
        >(test_eav_storage(), {
            let mut attrs: Vec<ExampleAttribute> = vec!["a_", "b_", "c_", "d_"]
                .into_iter()
                .map(|p| ExampleAttribute::WithPayload(p.to_string() + "one_to_many"))
                .collect();
            attrs.push(ExampleAttribute::WithoutPayload);
            attrs
        });
    }

    #[test]
    /// show AddressableContent implementation
    fn addressable_content_test() {
        // from_content()
        AddressableContentTestSuite::addressable_content_trait_test::<EntityAttributeValueIndex<ExampleAttribute>>(
            test_eav_content(),
            test_eav(),
            test_eav_address(),
        );
    }

    #[test]
    /// show CAS round trip
    fn cas_round_trip_test() {
        let addressable_contents = vec![test_eav()];
        AddressableContentTestSuite::addressable_content_round_trip::<
            EntityAttributeValueIndex<ExampleAttribute>,
            ExampleContentAddressableStorage,
        >(addressable_contents, test_content_addressable_storage());
    }

}
