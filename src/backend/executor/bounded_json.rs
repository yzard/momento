use serde::de::{DeserializeSeed, Error as DeError, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fmt;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

pub(crate) const MAXIMUM_JSON_DEPTH: usize = 32;
pub(crate) const MAXIMUM_JSON_COLLECTION_ITEMS: usize = 8_192;
pub(crate) const MAXIMUM_JSON_OBJECT_FIELDS: usize = 1_024;
pub(crate) const MAXIMUM_JSON_STRING_BYTES: usize = 256 * 1024;
pub(crate) const MAXIMUM_NORMALIZED_JSON_BYTES: usize = 1024 * 1024;
const MAXIMUM_METADATA_SOURCE_BYTES: usize = 4 * 1024 * 1024;
const MAXIMUM_METADATA_DEPTH: usize = 16;
const MAXIMUM_METADATA_FIELDS: usize = 4_096;
const MAXIMUM_METADATA_COLLECTION_ITEMS: usize = 8_192;
const MAXIMUM_METADATA_STRING_BYTES: usize = 256 * 1024;

#[derive(Debug)]
pub struct ParsedSupplementalMetadata {
    pub payload_json: String,
    pub date_taken: Option<DateTime<Utc>>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub gps_altitude: Option<f64>,
    pub description: Option<String>,
}

#[derive(Debug)]
pub struct ParsedExifMetadata {
    pub payload_json: String,
    pub date_taken: Option<DateTime<Utc>>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub gps_altitude: Option<f64>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_make: Option<String>,
    pub lens_model: Option<String>,
    pub iso: Option<i32>,
    pub exposure_time: Option<String>,
    pub f_number: Option<f64>,
    pub focal_length: Option<f64>,
    pub focal_length_35mm: Option<f64>,
    pub keywords: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub mime_type: Option<String>,
}

#[derive(Debug)]
pub struct ParsedFfprobeMetadata {
    pub payload_json: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub video_codec: Option<String>,
    pub duration_seconds: Option<f64>,
    pub date_taken: Option<DateTime<Utc>>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
}

const VALUE_CHARGE_BYTES: usize = 32;
const ARRAY_ITEM_CHARGE_BYTES: usize = 16;
const OBJECT_FIELD_CHARGE_BYTES: usize = 64;

#[derive(Default)]
struct JsonBudget {
    normalized_bytes: usize,
    string_bytes: usize,
    collection_items: usize,
}

impl JsonBudget {
    fn charge_value<E: DeError>(&mut self) -> Result<(), E> {
        self.charge_normalized::<E>(VALUE_CHARGE_BYTES)
    }

    fn charge_string<E: DeError>(&mut self, length: usize) -> Result<(), E> {
        self.string_bytes = self
            .string_bytes
            .checked_add(length)
            .ok_or_else(|| E::custom("JSON decoded string size overflowed"))?;
        if self.string_bytes > MAXIMUM_JSON_STRING_BYTES {
            return Err(E::custom("JSON decoded text exceeds 262144 bytes"));
        }
        self.charge_normalized::<E>(length)
    }

    fn charge_collection_item<E: DeError>(&mut self, bytes: usize) -> Result<(), E> {
        self.collection_items = self
            .collection_items
            .checked_add(1)
            .ok_or_else(|| E::custom("JSON collection item count overflowed"))?;
        if self.collection_items > MAXIMUM_JSON_COLLECTION_ITEMS {
            return Err(E::custom("JSON contains more than 8192 collection items"));
        }
        self.charge_normalized::<E>(bytes)
    }

    fn charge_normalized<E: DeError>(&mut self, bytes: usize) -> Result<(), E> {
        self.normalized_bytes = self
            .normalized_bytes
            .checked_add(bytes)
            .ok_or_else(|| E::custom("JSON normalized size overflowed"))?;
        if self.normalized_bytes > MAXIMUM_NORMALIZED_JSON_BYTES {
            return Err(E::custom("JSON normalized value exceeds 1048576 bytes"));
        }
        Ok(())
    }
}

struct BoundedValueSeed<'a> {
    budget: &'a std::cell::RefCell<JsonBudget>,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for BoundedValueSeed<'_> {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if self.depth > MAXIMUM_JSON_DEPTH {
            return Err(D::Error::custom("JSON nesting exceeds 32 levels"));
        }
        self.budget.borrow_mut().charge_value::<D::Error>()?;
        deserializer.deserialize_any(BoundedValueVisitor {
            budget: self.budget,
            depth: self.depth,
        })
    }
}

struct BoundedValueVisitor<'a> {
    budget: &'a std::cell::RefCell<JsonBudget>,
    depth: usize,
}

impl<'de> Visitor<'de> for BoundedValueVisitor<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded JSON value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(Value::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("JSON number is not finite"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        self.budget.borrow_mut().charge_string::<E>(value.len())?;
        Ok(Value::String(value.to_string()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        self.budget.borrow_mut().charge_string::<E>(value.len())?;
        Ok(Value::String(value))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(BoundedValueSeed {
            budget: self.budget,
            depth: self.depth + 1,
        })? {
            self.budget
                .borrow_mut()
                .charge_collection_item::<A::Error>(ARRAY_ITEM_CHARGE_BYTES)?;
            values.try_reserve(1).map_err(|error| {
                A::Error::custom(format!("JSON array allocation failed: {error}"))
            })?;
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut fields = Map::new();
        let mut field_count = 0_usize;
        while let Some(key) = object.next_key_seed(BoundedStringSeed {
            budget: self.budget,
        })? {
            field_count += 1;
            if field_count > MAXIMUM_JSON_OBJECT_FIELDS {
                return Err(A::Error::custom(
                    "JSON object contains more than 1024 fields",
                ));
            }
            self.budget
                .borrow_mut()
                .charge_collection_item::<A::Error>(OBJECT_FIELD_CHARGE_BYTES)?;
            if fields.contains_key(&key) {
                return Err(A::Error::custom(format!(
                    "JSON object contains duplicate field {key:?}"
                )));
            }
            let value = object.next_value_seed(BoundedValueSeed {
                budget: self.budget,
                depth: self.depth + 1,
            })?;
            fields.insert(key, value);
        }
        Ok(Value::Object(fields))
    }
}

struct BoundedStringSeed<'a> {
    budget: &'a std::cell::RefCell<JsonBudget>,
}

impl<'de> DeserializeSeed<'de> for BoundedStringSeed<'_> {
    type Value = String;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_string(BoundedStringVisitor {
            budget: self.budget,
        })
    }
}

struct BoundedStringVisitor<'a> {
    budget: &'a std::cell::RefCell<JsonBudget>,
}

impl Visitor<'_> for BoundedStringVisitor<'_> {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded JSON object key")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        self.budget.borrow_mut().charge_string::<E>(value.len())?;
        Ok(value.to_string())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: DeError,
    {
        self.budget.borrow_mut().charge_string::<E>(value.len())?;
        Ok(value)
    }
}

pub(crate) fn parse_bounded_json(bytes: &[u8]) -> Result<Value, String> {
    let budget = std::cell::RefCell::new(JsonBudget::default());
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = BoundedValueSeed {
        budget: &budget,
        depth: 0,
    }
    .deserialize(&mut deserializer)
    .map_err(|error| error.to_string())?;
    deserializer.end().map_err(|error| error.to_string())?;
    Ok(value)
}

#[derive(Default)]
struct MetadataJsonBudget {
    normalized: String,
    string_bytes: usize,
    fields: usize,
    collection_items: usize,
}

impl MetadataJsonBudget {
    fn write<E: DeError>(&mut self, value: &str) -> Result<(), E> {
        let length = self
            .normalized
            .len()
            .checked_add(value.len())
            .ok_or_else(|| E::custom("metadata normalized JSON size overflowed"))?;
        if length > MAXIMUM_NORMALIZED_JSON_BYTES {
            return Err(E::custom("metadata normalized JSON exceeds 1048576 bytes"));
        }
        self.normalized
            .try_reserve(value.len())
            .map_err(|error| E::custom(format!("metadata JSON allocation failed: {error}")))?;
        self.normalized.push_str(value);
        Ok(())
    }

    fn charge_string<E: DeError>(&mut self, length: usize) -> Result<(), E> {
        self.string_bytes = self
            .string_bytes
            .checked_add(length)
            .ok_or_else(|| E::custom("metadata decoded string size overflowed"))?;
        if self.string_bytes > MAXIMUM_METADATA_STRING_BYTES {
            return Err(E::custom("metadata decoded text exceeds 262144 bytes"));
        }
        Ok(())
    }

    fn charge_field<E: DeError>(&mut self) -> Result<(), E> {
        self.fields = self
            .fields
            .checked_add(1)
            .ok_or_else(|| E::custom("metadata field count overflowed"))?;
        if self.fields > MAXIMUM_METADATA_FIELDS {
            return Err(E::custom("metadata contains more than 4096 fields"));
        }
        self.charge_item::<E>()
    }

    fn charge_item<E: DeError>(&mut self) -> Result<(), E> {
        self.collection_items = self
            .collection_items
            .checked_add(1)
            .ok_or_else(|| E::custom("metadata collection item count overflowed"))?;
        if self.collection_items > MAXIMUM_METADATA_COLLECTION_ITEMS {
            return Err(E::custom(
                "metadata contains more than 8192 collection items",
            ));
        }
        Ok(())
    }

    fn write_json_string<E: DeError>(&mut self, value: &str) -> Result<(), E> {
        self.charge_string::<E>(value.len())?;
        self.write::<E>("\"")?;
        for character in value.chars() {
            match character {
                '"' => self.write::<E>("\\\"")?,
                '\\' => self.write::<E>("\\\\")?,
                '\u{08}' => self.write::<E>("\\b")?,
                '\u{0c}' => self.write::<E>("\\f")?,
                '\n' => self.write::<E>("\\n")?,
                '\r' => self.write::<E>("\\r")?,
                '\t' => self.write::<E>("\\t")?,
                '\u{00}'..='\u{1f}' => {
                    let escaped = format!("\\u{:04x}", character as u32);
                    self.write::<E>(&escaped)?;
                }
                _ => {
                    let mut encoded = [0_u8; 4];
                    self.write::<E>(character.encode_utf8(&mut encoded))?;
                }
            }
        }
        self.write::<E>("\"")
    }
}

struct MetadataNormalizeSeed<'a> {
    budget: &'a std::cell::RefCell<MetadataJsonBudget>,
    depth: usize,
    prefix_comma: bool,
}

impl<'de> DeserializeSeed<'de> for MetadataNormalizeSeed<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if self.depth > MAXIMUM_METADATA_DEPTH {
            return Err(D::Error::custom("metadata JSON nesting exceeds 16 levels"));
        }
        if self.prefix_comma {
            self.budget.borrow_mut().write::<D::Error>(",")?;
        }
        deserializer.deserialize_any(MetadataNormalizeVisitor {
            budget: self.budget,
            depth: self.depth,
        })
    }
}

struct MetadataNormalizeVisitor<'a> {
    budget: &'a std::cell::RefCell<MetadataJsonBudget>,
    depth: usize,
}

impl<'de> Visitor<'de> for MetadataNormalizeVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded metadata JSON")
    }

    fn visit_unit<E: DeError>(self) -> Result<Self::Value, E> {
        self.budget.borrow_mut().write::<E>("null")
    }

    fn visit_none<E: DeError>(self) -> Result<Self::Value, E> {
        self.visit_unit()
    }

    fn visit_bool<E: DeError>(self, value: bool) -> Result<Self::Value, E> {
        self.budget
            .borrow_mut()
            .write::<E>(if value { "true" } else { "false" })
    }

    fn visit_i64<E: DeError>(self, value: i64) -> Result<Self::Value, E> {
        self.budget.borrow_mut().write::<E>(&value.to_string())
    }

    fn visit_u64<E: DeError>(self, value: u64) -> Result<Self::Value, E> {
        self.budget.borrow_mut().write::<E>(&value.to_string())
    }

    fn visit_f64<E: DeError>(self, value: f64) -> Result<Self::Value, E> {
        if !value.is_finite() {
            return Err(E::custom("metadata JSON number is not finite"));
        }
        self.budget.borrow_mut().write::<E>(&value.to_string())
    }

    fn visit_str<E: DeError>(self, value: &str) -> Result<Self::Value, E> {
        self.budget.borrow_mut().write_json_string::<E>(value)
    }

    fn visit_string<E: DeError>(self, value: String) -> Result<Self::Value, E> {
        self.visit_str(&value)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        self.budget.borrow_mut().write::<A::Error>("[")?;
        let mut first = true;
        while sequence
            .next_element_seed(MetadataNormalizeSeed {
                budget: self.budget,
                depth: self.depth + 1,
                prefix_comma: !first,
            })?
            .is_some()
        {
            first = false;
            self.budget.borrow_mut().charge_item::<A::Error>()?;
        }
        self.budget.borrow_mut().write::<A::Error>("]")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Self::Value, A::Error> {
        self.budget.borrow_mut().write::<A::Error>("{")?;
        let mut keys = HashSet::new();
        let mut first = true;
        while let Some(key) = object.next_key::<String>()? {
            self.budget.borrow_mut().charge_field::<A::Error>()?;
            keys.try_reserve(1).map_err(|error| {
                A::Error::custom(format!("metadata key allocation failed: {error}"))
            })?;
            if keys.contains(&key) {
                return Err(A::Error::custom(format!(
                    "metadata JSON object contains duplicate field {key:?}"
                )));
            }
            if !first {
                self.budget.borrow_mut().write::<A::Error>(",")?;
            }
            first = false;
            self.budget
                .borrow_mut()
                .write_json_string::<A::Error>(&key)?;
            self.budget.borrow_mut().write::<A::Error>(":")?;
            keys.insert(key);
            object.next_value_seed(MetadataNormalizeSeed {
                budget: self.budget,
                depth: self.depth + 1,
                prefix_comma: false,
            })?;
        }
        self.budget.borrow_mut().write::<A::Error>("}")
    }
}

fn normalize_metadata_json(bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("metadata JSON is empty".to_string());
    }
    if bytes.len() > MAXIMUM_METADATA_SOURCE_BYTES {
        return Err("metadata JSON exceeds 4194304 bytes".to_string());
    }
    let budget = std::cell::RefCell::new(MetadataJsonBudget::default());
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    MetadataNormalizeSeed {
        budget: &budget,
        depth: 0,
        prefix_comma: false,
    }
    .deserialize(&mut deserializer)
    .map_err(|error| error.to_string())?;
    deserializer.end().map_err(|error| error.to_string())?;
    Ok(budget.into_inner().normalized)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StringOrInteger {
    Text(String),
    Signed(i64),
    Unsigned(u64),
}

impl StringOrInteger {
    fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Text(value) => value.parse().ok(),
            Self::Signed(value) => Some(*value),
            Self::Unsigned(value) => i64::try_from(*value).ok(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSupplementalTimestamp {
    timestamp: Option<StringOrInteger>,
}

#[derive(Deserialize)]
struct RawSupplementalGps {
    latitude: Option<f64>,
    longitude: Option<f64>,
    altitude: Option<f64>,
}

impl RawSupplementalGps {
    fn coordinates(&self) -> Option<(f64, f64)> {
        let latitude = self.latitude?;
        let longitude = self.longitude?;
        (latitude.is_finite()
            && longitude.is_finite()
            && (-90.0..=90.0).contains(&latitude)
            && (-180.0..=180.0).contains(&longitude)
            && latitude != 0.0
            && longitude != 0.0)
            .then_some((latitude, longitude))
    }

    fn finite_altitude(&self) -> Option<f64> {
        self.altitude.filter(|value| value.is_finite())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSupplementalMetadata {
    photo_taken_time: Option<RawSupplementalTimestamp>,
    creation_time: Option<RawSupplementalTimestamp>,
    geo_data_exif: Option<RawSupplementalGps>,
    geo_data: Option<RawSupplementalGps>,
    description: Option<String>,
}

pub(crate) fn parse_supplemental_metadata(
    bytes: &[u8],
) -> Result<ParsedSupplementalMetadata, String> {
    let payload_json = normalize_metadata_json(bytes)?;
    let data: RawSupplementalMetadata =
        serde_json::from_str(&payload_json).map_err(|error| error.to_string())?;
    let date_taken = data
        .photo_taken_time
        .as_ref()
        .and_then(|value| value.timestamp.as_ref())
        .and_then(StringOrInteger::as_i64)
        .and_then(|value| DateTime::from_timestamp(value, 0))
        .or_else(|| {
            data.creation_time
                .as_ref()
                .and_then(|value| value.timestamp.as_ref())
                .and_then(StringOrInteger::as_i64)
                .and_then(|value| DateTime::from_timestamp(value, 0))
        });
    let coordinates = data
        .geo_data_exif
        .as_ref()
        .and_then(RawSupplementalGps::coordinates)
        .or_else(|| {
            data.geo_data
                .as_ref()
                .and_then(RawSupplementalGps::coordinates)
        });
    let gps_altitude = data
        .geo_data_exif
        .as_ref()
        .and_then(RawSupplementalGps::finite_altitude)
        .or_else(|| {
            data.geo_data
                .as_ref()
                .and_then(RawSupplementalGps::finite_altitude)
        });
    Ok(ParsedSupplementalMetadata {
        payload_json,
        date_taken,
        gps_latitude: coordinates.map(|value| value.0),
        gps_longitude: coordinates.map(|value| value.1),
        gps_altitude,
        description: data.description.filter(|value| !value.is_empty()),
    })
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum JsonNumber {
    Signed(i64),
    Unsigned(u64),
    Float(f64),
}

impl JsonNumber {
    fn as_f64(&self, field: &str) -> Result<f64, String> {
        let value = match self {
            Self::Signed(value) => *value as f64,
            Self::Unsigned(value) => *value as f64,
            Self::Float(value) => *value,
        };
        if !value.is_finite() {
            return Err(format!("metadata field {field} is not finite"));
        }
        Ok(value)
    }

    fn as_i32(&self, field: &str) -> Result<i32, String> {
        match self {
            Self::Signed(value) => i32::try_from(*value)
                .map_err(|_| format!("metadata field {field} exceeds the i32 range")),
            Self::Unsigned(value) => i32::try_from(*value)
                .map_err(|_| format!("metadata field {field} exceeds the i32 range")),
            Self::Float(value) => checked_i32_from_f64(*value, field),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum RawKeywords {
    Many(Vec<ExifScalar>),
    One(ExifScalar),
}

impl RawKeywords {
    fn joined(&self) -> Option<String> {
        match self {
            Self::One(value) => value.as_text(),
            Self::Many(values) => {
                let values = values
                    .iter()
                    .filter_map(ExifScalar::as_text)
                    .collect::<Vec<_>>();
                (!values.is_empty()).then(|| values.join(","))
            }
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum ExifScalar {
    Text(String),
    Number(JsonNumber),
    Boolean(bool),
    Unsupported(Value),
}

impl ExifScalar {
    fn as_text(&self) -> Option<String> {
        match self {
            Self::Text(value) => (!value.trim().is_empty()).then(|| value.clone()),
            Self::Number(JsonNumber::Signed(value)) => Some(value.to_string()),
            Self::Number(JsonNumber::Unsigned(value)) => Some(value.to_string()),
            Self::Number(JsonNumber::Float(value)) if value.is_finite() => Some(value.to_string()),
            Self::Number(JsonNumber::Float(_)) | Self::Boolean(_) | Self::Unsupported(_) => None,
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Text(value) => parse_exif_number(value),
            Self::Number(value) => value.as_f64("ExifScalar").ok(),
            Self::Boolean(_) | Self::Unsupported(_) => None,
        }
    }

    fn as_i32(&self) -> Option<i32> {
        self.as_f64()
            .and_then(|value| checked_i32_from_f64(value, "ExifScalar").ok())
    }
}

#[derive(Deserialize, Serialize)]
struct RawExifMetadata {
    #[serde(rename = "DateTimeOriginal")]
    date_time_original: Option<ExifScalar>,
    #[serde(rename = "CreateDate")]
    create_date: Option<ExifScalar>,
    #[serde(rename = "ModifyDate")]
    modify_date: Option<ExifScalar>,
    #[serde(rename = "GPSLatitude")]
    gps_latitude: Option<ExifScalar>,
    #[serde(rename = "GPSLongitude")]
    gps_longitude: Option<ExifScalar>,
    #[serde(rename = "GPSAltitude")]
    gps_altitude: Option<ExifScalar>,
    #[serde(rename = "Make")]
    make: Option<ExifScalar>,
    #[serde(rename = "Model")]
    model: Option<ExifScalar>,
    #[serde(rename = "HostComputer")]
    host_computer: Option<ExifScalar>,
    #[serde(rename = "LensMake")]
    lens_make: Option<ExifScalar>,
    #[serde(rename = "LensModel")]
    lens_model: Option<ExifScalar>,
    #[serde(rename = "LensID")]
    lens_id: Option<ExifScalar>,
    #[serde(rename = "ISO")]
    iso: Option<ExifScalar>,
    #[serde(rename = "FNumber")]
    f_number: Option<ExifScalar>,
    #[serde(rename = "Aperture")]
    aperture: Option<ExifScalar>,
    #[serde(rename = "FocalLength")]
    focal_length: Option<ExifScalar>,
    #[serde(rename = "FocalLengthIn35mmFormat")]
    focal_length_in_35mm_format: Option<ExifScalar>,
    #[serde(rename = "FocalLength35efl")]
    focal_length_35efl: Option<ExifScalar>,
    #[serde(rename = "ExposureTime")]
    exposure_time: Option<ExifScalar>,
    #[serde(rename = "ShutterSpeed")]
    shutter_speed: Option<ExifScalar>,
    #[serde(rename = "Keywords")]
    keywords: Option<RawKeywords>,
    #[serde(rename = "ImageWidth")]
    image_width: Option<ExifScalar>,
    #[serde(rename = "ExifImageWidth")]
    exif_image_width: Option<ExifScalar>,
    #[serde(rename = "SourceImageWidth")]
    source_image_width: Option<ExifScalar>,
    #[serde(rename = "ImageHeight")]
    image_height: Option<ExifScalar>,
    #[serde(rename = "ExifImageHeight")]
    exif_image_height: Option<ExifScalar>,
    #[serde(rename = "SourceImageHeight")]
    source_image_height: Option<ExifScalar>,
    #[serde(rename = "MIMEType")]
    mime_type: Option<ExifScalar>,
}

pub(crate) fn parse_exif_metadata(bytes: &[u8]) -> Result<ParsedExifMetadata, String> {
    let normalized = normalize_metadata_json(bytes)?;
    let records: Vec<RawExifMetadata> =
        serde_json::from_str(&normalized).map_err(|error| error.to_string())?;
    let data = records
        .into_iter()
        .next()
        .ok_or_else(|| "exiftool metadata has no object record".to_string())?;
    let exposure_time =
        first_exif_exposure_time([data.exposure_time.as_ref(), data.shutter_speed.as_ref()]);
    let date_time_original = data
        .date_time_original
        .as_ref()
        .and_then(ExifScalar::as_text);
    let create_date = data.create_date.as_ref().and_then(ExifScalar::as_text);
    let modify_date = data.modify_date.as_ref().and_then(ExifScalar::as_text);
    let camera_make = data.make.as_ref().and_then(ExifScalar::as_text);
    let camera_model = first_exif_text([data.model.as_ref(), data.host_computer.as_ref()]);
    let lens_make = data.lens_make.as_ref().and_then(ExifScalar::as_text);
    let lens_model = first_exif_text([data.lens_model.as_ref(), data.lens_id.as_ref()]);
    let payload_json = serde_json::to_string(&data).map_err(|error| error.to_string())?;
    validate_normalized_payload(&payload_json)?;
    Ok(ParsedExifMetadata {
        payload_json,
        date_taken: first_string_ref([
            date_time_original.as_ref(),
            create_date.as_ref(),
            modify_date.as_ref(),
        ])
        .and_then(|value| parse_exif_datetime(value)),
        gps_latitude: data.gps_latitude.as_ref().and_then(ExifScalar::as_f64),
        gps_longitude: data.gps_longitude.as_ref().and_then(ExifScalar::as_f64),
        gps_altitude: data.gps_altitude.as_ref().and_then(ExifScalar::as_f64),
        camera_make,
        camera_model,
        lens_make,
        lens_model,
        iso: data.iso.as_ref().and_then(ExifScalar::as_i32),
        exposure_time,
        f_number: first_exif_f64([data.f_number.as_ref(), data.aperture.as_ref()]),
        focal_length: data.focal_length.as_ref().and_then(ExifScalar::as_f64),
        focal_length_35mm: first_exif_f64([
            data.focal_length_in_35mm_format.as_ref(),
            data.focal_length_35efl.as_ref(),
        ]),
        keywords: data.keywords.as_ref().and_then(RawKeywords::joined),
        width: first_exif_i32([
            data.image_width.as_ref(),
            data.exif_image_width.as_ref(),
            data.source_image_width.as_ref(),
        ]),
        height: first_exif_i32([
            data.image_height.as_ref(),
            data.exif_image_height.as_ref(),
            data.source_image_height.as_ref(),
        ]),
        mime_type: data
            .mime_type
            .as_ref()
            .and_then(ExifScalar::as_text)
            .filter(|value| value.contains('/')),
    })
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StringOrNumber {
    Text(String),
    Number(JsonNumber),
}

impl StringOrNumber {
    fn as_f64(&self, field: &str) -> Result<f64, String> {
        match self {
            Self::Text(value) => value
                .parse::<f64>()
                .map_err(|_| format!("metadata field {field} is not numeric"))
                .and_then(|value| {
                    value
                        .is_finite()
                        .then_some(value)
                        .ok_or_else(|| format!("metadata field {field} is not finite"))
                }),
            Self::Number(value) => value.as_f64(field),
        }
    }
}

#[derive(Deserialize)]
struct RawFfprobeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<JsonNumber>,
    height: Option<JsonNumber>,
}

#[derive(Deserialize)]
struct RawFfprobeTags {
    creation_time: Option<String>,
    #[serde(rename = "com.apple.quicktime.creationdate")]
    quicktime_creation_date: Option<String>,
    location: Option<String>,
    #[serde(rename = "com.apple.quicktime.location.ISO6709")]
    quicktime_location: Option<String>,
}

#[derive(Deserialize)]
struct RawFfprobeFormat {
    duration: Option<StringOrNumber>,
    tags: Option<RawFfprobeTags>,
}

#[derive(Deserialize)]
struct RawFfprobeMetadata {
    streams: Option<Vec<RawFfprobeStream>>,
    format: Option<RawFfprobeFormat>,
}

pub(crate) fn parse_ffprobe_metadata(bytes: &[u8]) -> Result<ParsedFfprobeMetadata, String> {
    let payload_json = normalize_metadata_json(bytes)?;
    let data: RawFfprobeMetadata =
        serde_json::from_str(&payload_json).map_err(|error| error.to_string())?;
    let video_stream = data.streams.as_ref().and_then(|streams| {
        streams
            .iter()
            .find(|stream| stream.codec_type.as_deref() == Some("video"))
    });
    let format = data.format.as_ref();
    let tags = format.and_then(|format| format.tags.as_ref());
    let date_taken = tags
        .and_then(|tags| {
            first_string_ref([
                tags.creation_time.as_ref(),
                tags.quicktime_creation_date.as_ref(),
            ])
        })
        .and_then(|value| DateTime::parse_from_rfc3339(&value.replace('Z', "+00:00")).ok())
        .map(|value| value.with_timezone(&Utc));
    let coordinates = tags
        .and_then(|tags| {
            first_string_ref([tags.location.as_ref(), tags.quicktime_location.as_ref()])
        })
        .and_then(|value| parse_iso6709_location(value));
    let duration_seconds = format
        .and_then(|format| format.duration.as_ref())
        .map(|value| value.as_f64("duration"))
        .transpose()?
        .filter(|value| *value >= 0.0);
    Ok(ParsedFfprobeMetadata {
        payload_json,
        width: optional_i32(
            video_stream.and_then(|stream| stream.width.as_ref()),
            "width",
        )?,
        height: optional_i32(
            video_stream.and_then(|stream| stream.height.as_ref()),
            "height",
        )?,
        video_codec: video_stream.and_then(|stream| stream.codec_name.clone()),
        duration_seconds,
        date_taken,
        gps_latitude: coordinates.map(|value| value.0),
        gps_longitude: coordinates.map(|value| value.1),
    })
}

fn validate_normalized_payload(payload: &str) -> Result<(), String> {
    if payload.len() > MAXIMUM_NORMALIZED_JSON_BYTES {
        return Err("metadata JSON normalized payload exceeds 1048576 bytes".to_string());
    }
    Ok(())
}

fn first_exif_text<const N: usize>(values: [Option<&ExifScalar>; N]) -> Option<String> {
    values.into_iter().flatten().find_map(ExifScalar::as_text)
}

fn first_exif_i32<const N: usize>(values: [Option<&ExifScalar>; N]) -> Option<i32> {
    values.into_iter().flatten().find_map(ExifScalar::as_i32)
}

fn first_exif_f64<const N: usize>(values: [Option<&ExifScalar>; N]) -> Option<f64> {
    values.into_iter().flatten().find_map(ExifScalar::as_f64)
}

fn first_exif_exposure_time<const N: usize>(values: [Option<&ExifScalar>; N]) -> Option<String> {
    values.into_iter().flatten().find_map(|value| {
        value
            .as_f64()
            .and_then(|value| format_exposure_time(value).ok())
    })
}

fn first_string_ref<const N: usize>(values: [Option<&String>; N]) -> Option<&String> {
    values.into_iter().flatten().next()
}

fn parse_exif_number(value: &str) -> Option<f64> {
    let value = value.trim();
    if let Ok(number) = value.parse::<f64>() {
        return number.is_finite().then_some(number);
    }
    let (numerator, denominator) = value.split_once('/')?;
    let numerator = numerator.trim().parse::<f64>().ok()?;
    let denominator = denominator.trim().parse::<f64>().ok()?;
    let number = numerator / denominator;
    (denominator != 0.0 && number.is_finite()).then_some(number)
}

fn optional_i32(value: Option<&JsonNumber>, field: &str) -> Result<Option<i32>, String> {
    value.map(|value| value.as_i32(field)).transpose()
}

fn checked_i32_from_f64(value: f64, field: &str) -> Result<i32, String> {
    if !value.is_finite()
        || value.fract() != 0.0
        || value < i32::MIN as f64
        || value > i32::MAX as f64
    {
        return Err(format!("metadata field {field} is not a bounded integer"));
    }
    Ok(value as i32)
}

fn format_exposure_time(value: f64) -> Result<String, String> {
    if value <= 0.0 || value >= 1.0 {
        return Ok(value.to_string());
    }
    let denominator = (1.0 / value).round();
    if !denominator.is_finite() || denominator > u64::MAX as f64 {
        return Err("metadata exposure time denominator exceeds the u64 range".to_string());
    }
    Ok(format!("1/{}", denominator as u64))
}

fn parse_exif_datetime(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if let Ok(value) = DateTime::parse_from_rfc3339(value) {
        return Some(value.with_timezone(&Utc));
    }
    for format in ["%Y:%m:%d %H:%M:%S%.f%:z", "%Y-%m-%d %H:%M:%S%.f%:z"] {
        if let Ok(value) = DateTime::parse_from_str(value, format) {
            return Some(value.with_timezone(&Utc));
        }
    }
    for format in ["%Y:%m:%d %H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(value) = NaiveDateTime::parse_from_str(value, format) {
            return Some(DateTime::from_naive_utc_and_offset(value, Utc));
        }
    }
    for format in ["%Y:%m:%d", "%Y-%m-%d"] {
        if let Ok(value) = NaiveDate::parse_from_str(value, format) {
            return value
                .and_hms_opt(0, 0, 0)
                .map(|value| DateTime::from_naive_utc_and_offset(value, Utc));
        }
    }
    None
}

fn parse_iso6709_location(location: &str) -> Option<(f64, f64)> {
    let location = location.trim_end_matches('/');
    let split = location
        .char_indices()
        .skip(1)
        .find_map(|(index, character)| matches!(character, '+' | '-').then_some(index))?;
    let (latitude, longitude) = location.split_at(split);
    let longitude_end = longitude
        .char_indices()
        .skip(1)
        .find_map(|(index, character)| matches!(character, '+' | '-').then_some(index))
        .unwrap_or(longitude.len());
    let latitude = latitude.parse::<f64>().ok()?;
    let longitude = longitude[..longitude_end].parse::<f64>().ok()?;
    (latitude.is_finite()
        && longitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude))
    .then_some((latitude, longitude))
}
