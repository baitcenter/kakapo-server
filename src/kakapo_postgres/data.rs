
use linked_hash_map::LinkedHashMap;
use plugins::v1::DataStoreEntity;
use plugins::v1::DatastoreError;
use plugins::v1::DataQueryEntity;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DataType {
    SmallInteger, //TODO: + Serial
    Integer, //TODO: + Serial
    BigInteger, //TODO: + Serial
    //TODO: Decimal { precision: u32, scale: u32 },
    Float,
    DoubleFloat,

    //TODO: Monetary

    String,
    VarChar { length: u32 },
    //Char is not going to be supported

    Byte,

    Timestamp { //TODO: precision?
    #[serde(default, rename = "withTZ")]
    with_tz: bool
    },
    Date,
    Time { //TODO: precision?
    #[serde(default, rename = "withTZ")]
    with_tz: bool
    },
    //TODO: TimeInterval,

    Boolean,
    //TODO: enum + geometric + net address + bit string + uuid +  ...
    Json, //TODO: binary?
    //TODO: arrays
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum IndexableValue {
    Integer(i64),
    String(String),
}



mod date_time_serde {
    use serde::{Deserializer, Deserialize, Serializer, Serialize};

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    struct DateTimeSerde {
        #[serde(rename = "$timestamp")]
        datetime: chrono::NaiveDateTime
    }

    pub fn serialize<S: Serializer>(data: &chrono::NaiveDateTime, serializer: S) -> Result<S::Ok, S::Error> {
        let input = DateTimeSerde { datetime: *data };
        input.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<chrono::NaiveDateTime, D::Error> {
        let res = DateTimeSerde::deserialize(deserializer)?;
        Ok(res.datetime)
    }
}

mod date_serde {
    use serde::{Deserializer, Deserialize, Serializer, Serialize};

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    struct DateSerde {
        #[serde(rename = "$date")]
        date: chrono::NaiveDate
    }

    pub fn serialize<S: Serializer>(data: &chrono::NaiveDate, serializer: S) -> Result<S::Ok, S::Error> {
        let input = DateSerde { date: *data };
        input.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<chrono::NaiveDate, D::Error> {
        let res = DateSerde::deserialize(deserializer)?;
        Ok(res.date)
    }
}

mod binary_serde {
    use base64;
    use serde::{Deserializer, Deserialize, Serializer, Serialize};
    use serde::de::Error;

    #[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
    struct BinarySerde {
        #[serde(rename = "$binary")]
        base64: String
    }

    pub fn serialize<S: Serializer>(data: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error> {
        let input = BinarySerde { base64: base64::encode(data) };
        input.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let res = BinarySerde::deserialize(deserializer)?;
        let res = base64::decode(&res.base64)
            .map_err(|err| D::Error::custom(err))?;
        Ok(res)
    }
}


/// Using a modified MongoDB Format https://docs.mongodb.com/manual/reference/mongodb-extended-json/
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum Value {
    Null,
    String(String),
    Integer(i64), //TODO: should be bigdecimal?
    Float(f64), //TODO: should be bigdecimal?
    Boolean(bool),
    #[serde(with = "date_time_serde")]
    DateTime(chrono::NaiveDateTime),
    #[serde(with = "date_serde")]
    Date(chrono::NaiveDate),
    #[serde(with = "binary_serde")]
    Binary(Vec<u8>),
    Json(serde_json::Value),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawTableDataColumns {
    pub keys: Vec<String>,
    pub values: Vec<String>
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawTableDataData {
    pub keys: Vec<IndexableValue>,
    pub values: Vec<Value>
}

/// Default return value from a query
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawTableData  {
    pub columns: RawTableDataColumns,
    pub data: Vec<RawTableDataData>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValuePairObject {
    pub keys: LinkedHashMap<String, IndexableValue>,
    pub values: LinkedHashMap<String, Value>,
}


#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum KeyedTableData {
    ///```json
    /// {
    ///   42: {
    ///     "message": "hello world",
    ///     "category": "greeting",
    ///   },
    ///   43: {
    ///     "message": "goodbye world",
    ///     "category": "farewell",
    ///   }
    /// }
    ///```
    Simplified(LinkedHashMap<
        IndexableValue,
        LinkedHashMap<String, Value>>), //can only be used if only one key exists
    ///```json
    /// [
    ///   {
    ///     "keys": {
    ///       "id": 42,
    ///     },
    ///     "values": {
    ///       "message": "hello world",
    ///       "category": "greeting",
    ///     }
    ///   },
    ///   {
    ///     "keys": {
    ///       "id": 43,
    ///     },
    ///     "values": {
    ///       "message": "goodbye world",
    ///       "category": "farewell",
    ///     }
    ///   }
    /// ]
    ///```
    Data(Vec<KeyValuePairObject>),
    ///```json
    /// {
    ///   "columns": {
    ///     "keys": [ "id" ],
    ///     "values": [ "message", "category" ]
    ///   },
    ///   "data": [
    ///     {
    ///       "keys": [ 42 ],
    ///       "values": [ "hello world", "greeting" ]
    ///     },
    ///     {
    ///       "keys": [ 43 ],
    ///       "values": [ "goodbye world", "farewell" ]
    ///     }
    ///   ]
    /// }
    ///```
    FlatData(RawTableData), //default output format
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum KeyData {
    Data(ObjectKeys),
    FlatData(TabularKeys),
    Keyed(KeyedTableData),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum TableData {
    ///```json
    /// [
    ///   {
    ///     "id": 42,
    ///     "message": "hello world",
    ///     "category": "greeting",
    ///   },
    ///   {
    ///     "id": 43,
    ///     "message": "goodbye world",
    ///     "category": "farewell",
    ///   }
    /// ]
    ///```
    Data(ObjectValues),
    //ColumnData(BTreeMap<String, Vec<Value>>),
    ///```json
    /// {
    ///   "columns": [ "id", "message", "category" ],
    ///   "data": [
    ///     [ 42, "hello world", "greeting" ],
    ///     [ 43, "goodbye world", "farewell" ],
    ///  ]
    /// }
    ///```
    FlatData(TabularValues),
    Keyed(KeyedTableData),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabularValues {
    pub columns: Vec<String>,
    pub data: Vec<Vec<Value>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabularKeys {
    pub columns: Vec<String>,
    pub data: Vec<Vec<IndexableValue>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectValues(pub Vec<LinkedHashMap<String, Value>>);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectKeys(pub Vec<LinkedHashMap<String, IndexableValue>>);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub name: String,
    pub data_type: DataType,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub nullable: bool,
}

impl Column {
    pub fn get_name(&self) -> String {
        self.name.to_owned()
    }
}


#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "op")]
pub enum Expression {
    Equals {
        column: String,
        value: Value
    },
    NotEqual {
        column: String,
        value: Value
    },
    GreaterThan {
        column: String,
        value: Value,
    },
    LessThan {
        column: String,
        value: Value,
    },
    In {
        column: String,
        values: Vec<Value>,
    },
}


#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Constraint {
    Key(String),
    //KeyTogether(Vec<String>),
    Unique(String),
    UniqueTogether(Vec<String>),

    Check(Expression),

    Reference {
        column: String,
        #[serde(rename = "foreignTable")]
        foreign_table: String,
        #[serde(rename = "foreignColumn")]
        foreign_column: String,
    },
    ReferenceTogether {
        columns: Vec<String>,
        #[serde(rename = "foreignTable")]
        foreign_table: String,
        #[serde(rename = "foreignColumns")]
        foreign_columns: Vec<String>,
    },

}


// This is the same as SchemaModification::Create
#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SchemaState {
    pub columns: Vec<Column>,
    pub constraint: Vec<Constraint>,
}

impl SchemaState {
    pub fn get_column_names(&self) -> Vec<String> {
        self.columns
            .iter()
            .map(|col| col.get_name())
            .collect()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub name: String, //TODO: make sure this is an alphanumeric, otherwise SQL injection!
    pub description: String,
    pub schema: SchemaState,
}

impl Table {
    fn get_name(&self) -> &str {
        &self.name
    }
}

impl From<&DataStoreEntity> for Result<Table, DatastoreError> {
    fn from(item: &DataStoreEntity) -> Result<Table, DatastoreError> {
        Ok(Table {
            name: item.name.to_owned(),
            description: item.description.to_owned(),
            schema: serde_json::from_value(item.schema.to_owned())
                .map_err(|_| DatastoreError::SerializationError)?, //TODO: shouldn't copy this here
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum QueryParams {
    //TODO: implement named parameters, unfortunately postgres doesn't have named parameters so...
    //Named(BTreeMap<String, Value>),
    Unnamed(Vec<Value>),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    pub name: String, //TODO: make sure this is an alphanumeric
    pub description: String,
    pub statement: String,
}

impl From<&DataQueryEntity> for Result<Query, DatastoreError> {
    fn from(item: &DataQueryEntity) -> Result<Query, DatastoreError> {
        Ok(Query {
            name: item.name.to_owned(),
            description: item.description.to_owned(),
            statement: item.statement.to_owned(),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use serde_json::from_value;

    #[test]
    fn test_deserialize_value() {
        let val: Value = from_value(json!(null)).unwrap();
        assert_eq!(val, Value::Null);

        let val: Value = from_value(json!("Hello World")).unwrap();
        assert_eq!(val, Value::String("Hello World".to_string()));

        let val: Value = from_value(json!(42)).unwrap();
        assert_eq!(val, Value::Integer(42));

        let val: Value = from_value(json!(3.141592)).unwrap();
        assert_eq!(val, Value::Float(3.141592));

        let val: Value = from_value(json!(true)).unwrap();
        assert_eq!(val, Value::Boolean(true));

        let date = chrono::NaiveDate::from_ymd(2019, 04, 20).and_hms(16, 20, 00);
        let val: Value = from_value(json!({"$timestamp" : "2019-04-20T16:20:00"})).unwrap();
        assert_eq!(val, Value::DateTime(date));

        let date = chrono::NaiveDate::from_ymd(2019, 04, 20);
        let val: Value = from_value(json!({"$date" : "2019-04-20"})).unwrap();
        assert_eq!(val, Value::Date(date));

        let data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let val: Value = from_value(json!({"$binary" : "3q2+7w=="})).unwrap();
        assert_eq!(val, Value::Binary(data));

        let data = json!({"hello" : "world"});
        let val: Value = from_value(json!({"hello" : "world"})).unwrap();
        assert_eq!(val, Value::Json(data));
    }

    #[test]
    fn test_serialize_value() {
        let date = Value::DateTime(chrono::NaiveDate::from_ymd(2019, 04, 20).and_hms(16, 20, 00));
        let val = serde_json::to_value(&date).unwrap();
        assert_eq!(val, json!({"$timestamp" : "2019-04-20T16:20:00"}));

        let date = Value::Date(chrono::NaiveDate::from_ymd(2019, 04, 20));
        let val = serde_json::to_value(&date).unwrap();
        assert_eq!(val, json!({"$date" : "2019-04-20"}));

        let data = Value::Binary(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        let val = serde_json::to_value(&data).unwrap();
        assert_eq!(val, json!({"$binary" : "3q2+7w=="}));

        let data = Value::Json(json!({"hello" : "world"}));
        let val = serde_json::to_value(&data).unwrap();
        assert_eq!(val, json!({"hello" : "world"}));
    }
}