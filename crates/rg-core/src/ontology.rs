use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use serde::Deserialize;

use crate::{
    Assertion, AssertionId, AssertionStatus, Entity, EntityId, EntityType, GraphValue, PredicateId,
    PropertyKey,
};

#[derive(Clone, Debug, PartialEq)]
pub struct GraphOntology {
    entity_types: BTreeMap<String, EntityTypeSchema>,
    predicates: BTreeMap<PredicateId, PredicateSchema>,
}

impl GraphOntology {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, OntologyError> {
        let raw: RawOntology =
            serde_yaml::from_str(yaml).map_err(|error| OntologyError::Yaml(error.to_string()))?;

        Ok(Self {
            entity_types: raw.entity_types,
            predicates: raw
                .predicates
                .into_iter()
                .map(|(predicate, schema)| (PredicateId::new(predicate), schema.into()))
                .collect(),
        })
    }

    pub fn entity_type(&self, name: &str) -> Option<&EntityTypeSchema> {
        self.entity_types.get(name)
    }

    pub fn predicate(&self, predicate: &PredicateId) -> Option<&PredicateSchema> {
        self.predicates.get(predicate)
    }

    pub fn validate_entity(&self, entity: &Entity) -> Result<(), OntologyValidationError> {
        let (entity_type_name, schema) = self.entity_schema_for(entity).ok_or_else(|| {
            OntologyValidationError::UnknownEntityType {
                entity_id: entity.id.clone(),
                entity_type: entity.entity_type.clone(),
            }
        })?;

        for (property, property_type) in &schema.properties {
            let key = PropertyKey::new(property.clone());
            let Some(value) = entity.properties.0.get(&key) else {
                continue;
            };
            let actual = GraphValueKind::from_value(value, None);
            if !property_type.matches(value) {
                return Err(OntologyValidationError::PropertyTypeMismatch {
                    entity_id: entity.id.clone(),
                    property: key,
                    expected: *property_type,
                    actual,
                });
            }
        }

        for property in schema.properties.keys() {
            let key = PropertyKey::new(property.clone());
            if !entity.properties.0.contains_key(&key) {
                return Err(OntologyValidationError::MissingProperty {
                    entity_id: entity.id.clone(),
                    property: key,
                });
            }
        }

        if !entity_type_matches_name(entity_type_name, &entity.entity_type) {
            return Err(OntologyValidationError::UnknownEntityType {
                entity_id: entity.id.clone(),
                entity_type: entity.entity_type.clone(),
            });
        }

        Ok(())
    }

    pub fn validate_assertion(
        &self,
        assertion: &Assertion,
        entities: &BTreeMap<EntityId, Entity>,
    ) -> Result<(), OntologyValidationError> {
        let predicate_schema = self.predicate(&assertion.predicate).ok_or_else(|| {
            OntologyValidationError::UnknownPredicate(assertion.predicate.clone())
        })?;

        let subject = entities
            .get(&assertion.subject)
            .ok_or_else(|| OntologyValidationError::UnknownSubject(assertion.subject.clone()))?;
        if !predicate_schema
            .subject
            .matches_entity_type(&subject.entity_type)
        {
            return Err(OntologyValidationError::SubjectTypeMismatch {
                predicate: assertion.predicate.clone(),
                subject: assertion.subject.clone(),
                expected: predicate_schema.subject.clone(),
                actual: subject.entity_type.clone(),
            });
        }

        self.validate_assertion_object(assertion, predicate_schema, entities)
    }

    pub fn validate_assertions<'a>(
        &self,
        assertions: impl IntoIterator<Item = &'a Assertion>,
        entities: &BTreeMap<EntityId, Entity>,
    ) -> Result<(), OntologyValidationError> {
        let assertions = assertions.into_iter().collect::<Vec<_>>();
        for assertion in &assertions {
            self.validate_assertion(assertion, entities)?;
        }
        self.validate_max_active_object_constraints(&assertions)
    }

    fn entity_schema_for<'a>(&'a self, entity: &Entity) -> Option<(&'a str, &'a EntityTypeSchema)> {
        self.entity_types
            .iter()
            .find(|(name, _)| entity_type_matches_name(name, &entity.entity_type))
            .map(|(name, schema)| (name.as_str(), schema))
    }

    fn validate_assertion_object(
        &self,
        assertion: &Assertion,
        predicate_schema: &PredicateSchema,
        entities: &BTreeMap<EntityId, Entity>,
    ) -> Result<(), OntologyValidationError> {
        match &assertion.object {
            GraphValue::Entity(entity_id) => {
                let entity = entities
                    .get(entity_id)
                    .ok_or_else(|| OntologyValidationError::UnknownObject(entity_id.clone()))?;
                if predicate_schema
                    .object
                    .matches_entity_type(&entity.entity_type)
                {
                    Ok(())
                } else {
                    Err(OntologyValidationError::ObjectTypeMismatch {
                        predicate: assertion.predicate.clone(),
                        assertion_id: assertion.id.clone(),
                        expected: predicate_schema.object.clone(),
                        actual: GraphValueKind::Entity(entity.entity_type.clone()),
                    })
                }
            }
            value => Err(OntologyValidationError::ObjectTypeMismatch {
                predicate: assertion.predicate.clone(),
                assertion_id: assertion.id.clone(),
                expected: predicate_schema.object.clone(),
                actual: GraphValueKind::from_value(value, None),
            }),
        }
    }

    fn validate_max_active_object_constraints(
        &self,
        assertions: &[&Assertion],
    ) -> Result<(), OntologyValidationError> {
        for (predicate, schema) in &self.predicates {
            let Some(max) = schema.constraints.max_active_objects_per_subject else {
                continue;
            };
            if max != 1 {
                continue;
            }

            let constrained = assertions
                .iter()
                .copied()
                .filter(|assertion| {
                    assertion.status == AssertionStatus::Active && &assertion.predicate == predicate
                })
                .collect::<Vec<_>>();

            for left_index in 0..constrained.len() {
                let left = constrained[left_index];
                for right in constrained.iter().skip(left_index + 1) {
                    if left.subject == right.subject
                        && left.context == right.context
                        && left.object != right.object
                        && left.valid_time.overlaps(&right.valid_time)
                    {
                        let mut assertion_ids = vec![left.id.clone(), right.id.clone()];
                        assertion_ids.sort();
                        return Err(OntologyValidationError::MaxActiveObjectsPerSubject {
                            predicate: predicate.clone(),
                            subject: left.subject.clone(),
                            max,
                            assertion_ids,
                        });
                    }
                }
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityTypeSchema {
    pub properties: BTreeMap<String, PropertyType>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PredicateSchema {
    pub subject: OntologyTypeRef,
    pub object: OntologyTypeRef,
    pub temporal: bool,
    pub mutually_exclusive_with: Vec<PredicateId>,
    pub constraints: PredicateConstraints,
    pub properties: BTreeMap<String, PropertyType>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PredicateConstraints {
    pub max_active_objects_per_subject: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OntologyTypeRef {
    Entity,
    Named(String),
}

impl OntologyTypeRef {
    fn matches_entity_type(&self, entity_type: &EntityType) -> bool {
        match self {
            Self::Entity => true,
            Self::Named(name) => entity_type_matches_name(name, entity_type),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyType {
    String,
    Date,
    Float,
    Integer,
    Boolean,
}

impl PropertyType {
    fn matches(self, value: &GraphValue) -> bool {
        matches!(
            (self, value),
            (Self::String, GraphValue::Text(_))
                | (Self::Date, GraphValue::Time(_))
                | (Self::Float, GraphValue::Decimal(_))
                | (Self::Integer, GraphValue::Integer(_))
                | (Self::Boolean, GraphValue::Boolean(_))
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphValueKind {
    Entity(EntityType),
    Text,
    Integer,
    Decimal,
    Boolean,
    Time,
    Null,
}

impl GraphValueKind {
    fn from_value(value: &GraphValue, entity_type: Option<EntityType>) -> Self {
        match value {
            GraphValue::Entity(_) => Self::Entity(
                entity_type.unwrap_or_else(|| EntityType::Custom("UnknownEntity".to_owned())),
            ),
            GraphValue::Text(_) => Self::Text,
            GraphValue::Integer(_) => Self::Integer,
            GraphValue::Decimal(_) => Self::Decimal,
            GraphValue::Boolean(_) => Self::Boolean,
            GraphValue::Time(_) => Self::Time,
            GraphValue::Null => Self::Null,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OntologyValidationError {
    UnknownEntityType {
        entity_id: EntityId,
        entity_type: EntityType,
    },
    UnknownPredicate(PredicateId),
    UnknownSubject(EntityId),
    UnknownObject(EntityId),
    MissingProperty {
        entity_id: EntityId,
        property: PropertyKey,
    },
    PropertyTypeMismatch {
        entity_id: EntityId,
        property: PropertyKey,
        expected: PropertyType,
        actual: GraphValueKind,
    },
    SubjectTypeMismatch {
        predicate: PredicateId,
        subject: EntityId,
        expected: OntologyTypeRef,
        actual: EntityType,
    },
    ObjectTypeMismatch {
        predicate: PredicateId,
        assertion_id: AssertionId,
        expected: OntologyTypeRef,
        actual: GraphValueKind,
    },
    MaxActiveObjectsPerSubject {
        predicate: PredicateId,
        subject: EntityId,
        max: usize,
        assertion_ids: Vec<AssertionId>,
    },
}

impl fmt::Display for OntologyValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for OntologyValidationError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OntologyError {
    Yaml(String),
}

impl fmt::Display for OntologyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Yaml(message) => write!(formatter, "invalid ontology yaml: {message}"),
        }
    }
}

impl Error for OntologyError {}

#[derive(Deserialize)]
struct RawOntology {
    entity_types: BTreeMap<String, EntityTypeSchema>,
    predicates: BTreeMap<String, RawPredicateSchema>,
}

#[derive(Deserialize)]
struct RawPredicateSchema {
    subject: OntologyTypeRef,
    object: OntologyTypeRef,
    #[serde(default)]
    temporal: bool,
    #[serde(default)]
    mutually_exclusive_with: Vec<String>,
    #[serde(default)]
    constraints: PredicateConstraints,
    #[serde(default)]
    properties: BTreeMap<String, PropertyType>,
}

impl From<RawPredicateSchema> for PredicateSchema {
    fn from(raw: RawPredicateSchema) -> Self {
        Self {
            subject: raw.subject,
            object: raw.object,
            temporal: raw.temporal,
            mutually_exclusive_with: raw
                .mutually_exclusive_with
                .into_iter()
                .map(PredicateId::new)
                .collect(),
            constraints: raw.constraints,
            properties: raw.properties,
        }
    }
}

impl<'de> Deserialize<'de> for EntityTypeSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawEntityTypeSchema {
            #[serde(default)]
            properties: BTreeMap<String, PropertyType>,
        }

        let raw = RawEntityTypeSchema::deserialize(deserializer)?;
        Ok(Self {
            properties: raw.properties,
        })
    }
}

impl<'de> Deserialize<'de> for PredicateSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(RawPredicateSchema::deserialize(deserializer)?.into())
    }
}

impl<'de> Deserialize<'de> for PredicateConstraints {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawPredicateConstraints {
            max_active_objects_per_subject: Option<usize>,
        }

        let raw = RawPredicateConstraints::deserialize(deserializer)?;
        Ok(Self {
            max_active_objects_per_subject: raw.max_active_objects_per_subject,
        })
    }
}

impl<'de> Deserialize<'de> for OntologyTypeRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value == "Entity" {
            Ok(Self::Entity)
        } else {
            Ok(Self::Named(value))
        }
    }
}

impl<'de> Deserialize<'de> for PropertyType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "string" => Ok(Self::String),
            "date" => Ok(Self::Date),
            "float" => Ok(Self::Float),
            "integer" => Ok(Self::Integer),
            "boolean" => Ok(Self::Boolean),
            other => Err(serde::de::Error::custom(format!(
                "unsupported property type {other}"
            ))),
        }
    }
}

fn entity_type_matches_name(name: &str, entity_type: &EntityType) -> bool {
    match entity_type {
        EntityType::Person => name == "Person",
        EntityType::Organization => name == "Company" || name == "Organization",
        EntityType::Place => name == "Place",
        EntityType::Event => name == "Event",
        EntityType::Document => name == "Document",
        EntityType::Concept => name == "Concept",
        EntityType::Custom(value) => value == name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Assertion, AssertionId, AssertionStatus, Confidence, ContextScope, Entity, EntityId,
        EntityType, GraphValue, PredicateId, PropertyKey, PropertyMap, SourceId, TimeInterval,
        TxTime, ValidTime,
    };
    use std::collections::BTreeMap;

    fn ontology() -> GraphOntology {
        GraphOntology::from_yaml_str(include_str!("../../../schemas/ontology/reality-graph.yaml"))
            .expect("ontology yaml parses")
    }

    fn entity(id: &str, entity_type: EntityType, properties: &[(&str, GraphValue)]) -> Entity {
        Entity {
            id: EntityId::new(id),
            entity_type,
            canonical_name: None,
            properties: PropertyMap(
                properties
                    .iter()
                    .map(|(key, value)| (PropertyKey::new(*key), value.clone()))
                    .collect(),
            ),
            created_tx: TxTime::new(1),
        }
    }

    fn assertion(
        id: &str,
        subject: &str,
        predicate: &str,
        object: GraphValue,
        start: i64,
        end: Option<i64>,
    ) -> Assertion {
        Assertion {
            id: AssertionId::new(id),
            subject: EntityId::new(subject),
            predicate: PredicateId::new(predicate),
            object,
            valid_time: TimeInterval::new(ValidTime::new(start), end.map(ValidTime::new))
                .expect("valid time interval"),
            transaction_time: TimeInterval::new(TxTime::new(1), None)
                .expect("valid transaction interval"),
            confidence: Confidence::new(0.9).expect("valid confidence"),
            source_ids: vec![SourceId::new("source-1")],
            context: ContextScope::Global,
            status: AssertionStatus::Active,
        }
    }

    fn entities() -> BTreeMap<EntityId, Entity> {
        [
            entity(
                "person-a",
                EntityType::Person,
                &[
                    ("name", GraphValue::Text("Person A".to_owned())),
                    ("birth_date", GraphValue::Time(ValidTime::new(10))),
                ],
            ),
            entity(
                "company-a",
                EntityType::Organization,
                &[
                    ("name", GraphValue::Text("Company A".to_owned())),
                    ("jurisdiction", GraphValue::Text("US-DE".to_owned())),
                ],
            ),
            entity(
                "company-b",
                EntityType::Organization,
                &[
                    ("name", GraphValue::Text("Company B".to_owned())),
                    ("jurisdiction", GraphValue::Text("US-NY".to_owned())),
                ],
            ),
        ]
        .into_iter()
        .map(|entity| (entity.id.clone(), entity))
        .collect()
    }

    #[test]
    fn loads_entity_and_predicate_schemas_from_yaml() {
        let ontology = ontology();

        assert_eq!(
            ontology
                .entity_type("Person")
                .expect("person schema")
                .properties
                .get("birth_date"),
            Some(&PropertyType::Date)
        );
        let ceo_of = ontology
            .predicate(&PredicateId::new("CEO_OF"))
            .expect("CEO_OF predicate");
        assert_eq!(ceo_of.subject, OntologyTypeRef::Named("Person".to_owned()));
        assert_eq!(ceo_of.object, OntologyTypeRef::Named("Company".to_owned()));
        assert!(ceo_of.temporal);
        assert_eq!(ceo_of.constraints.max_active_objects_per_subject, Some(1));
    }

    #[test]
    fn validates_entity_properties_against_declared_types() {
        let ontology = ontology();
        let invalid_person = entity(
            "person-b",
            EntityType::Person,
            &[("name", GraphValue::Integer(42))],
        );

        assert_eq!(
            ontology.validate_entity(&invalid_person),
            Err(OntologyValidationError::PropertyTypeMismatch {
                entity_id: EntityId::new("person-b"),
                property: PropertyKey::new("name"),
                expected: PropertyType::String,
                actual: GraphValueKind::Integer,
            })
        );
    }

    #[test]
    fn validates_assertion_subject_and_object_entity_types() {
        let ontology = ontology();
        let entities = entities();
        let valid = assertion(
            "worked-at",
            "person-a",
            "WORKED_AT",
            GraphValue::Entity(EntityId::new("company-a")),
            10,
            Some(20),
        );
        let invalid_subject = assertion(
            "ceo-subject",
            "company-a",
            "CEO_OF",
            GraphValue::Entity(EntityId::new("company-b")),
            10,
            Some(20),
        );
        let invalid_object = assertion(
            "ceo-object",
            "person-a",
            "CEO_OF",
            GraphValue::Entity(EntityId::new("person-a")),
            10,
            Some(20),
        );

        assert_eq!(ontology.validate_assertion(&valid, &entities), Ok(()));
        assert_eq!(
            ontology.validate_assertion(&invalid_subject, &entities),
            Err(OntologyValidationError::SubjectTypeMismatch {
                predicate: PredicateId::new("CEO_OF"),
                subject: EntityId::new("company-a"),
                expected: OntologyTypeRef::Named("Person".to_owned()),
                actual: EntityType::Organization,
            })
        );
        assert_eq!(
            ontology.validate_assertion(&invalid_object, &entities),
            Err(OntologyValidationError::ObjectTypeMismatch {
                predicate: PredicateId::new("CEO_OF"),
                assertion_id: AssertionId::new("ceo-object"),
                expected: OntologyTypeRef::Named("Company".to_owned()),
                actual: GraphValueKind::Entity(EntityType::Person),
            })
        );
    }

    #[test]
    fn enforces_max_active_objects_per_subject_over_overlapping_valid_time() {
        let ontology = ontology();
        let entities = entities();
        let first = assertion(
            "ceo-a",
            "person-a",
            "CEO_OF",
            GraphValue::Entity(EntityId::new("company-a")),
            10,
            Some(20),
        );
        let overlapping = assertion(
            "ceo-b",
            "person-a",
            "CEO_OF",
            GraphValue::Entity(EntityId::new("company-b")),
            15,
            Some(25),
        );
        let adjacent = assertion(
            "ceo-c",
            "person-a",
            "CEO_OF",
            GraphValue::Entity(EntityId::new("company-b")),
            20,
            Some(30),
        );

        assert_eq!(
            ontology.validate_assertions([first.clone(), adjacent].iter(), &entities),
            Ok(())
        );
        assert_eq!(
            ontology.validate_assertions([first, overlapping].iter(), &entities),
            Err(OntologyValidationError::MaxActiveObjectsPerSubject {
                predicate: PredicateId::new("CEO_OF"),
                subject: EntityId::new("person-a"),
                max: 1,
                assertion_ids: vec![AssertionId::new("ceo-a"), AssertionId::new("ceo-b")],
            })
        );
    }

    #[test]
    fn accepts_entity_wildcard_predicate_endpoints() {
        let ontology = ontology();
        let entities = entities();
        let owns = assertion(
            "owns",
            "company-a",
            "OWNS",
            GraphValue::Entity(EntityId::new("company-b")),
            10,
            None,
        );

        assert_eq!(ontology.validate_assertion(&owns, &entities), Ok(()));
    }
}
