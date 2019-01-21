use connection::executor::Conn;
use data;

pub mod error;

use model::table::error::TableError;

use model::state::State;
use model::entity::error::EntityError;

use database::Database;
use database::DatabaseFunctions;
use model::state::GetConnection;
use database::DbError;


pub struct TableAction;
pub trait TableActionFunctions<S>
    where Self: Send,
{
    fn query(conn: &S, table: &data::Table) -> Result<data::RawTableData, TableError>;

    fn insert_row(conn: &S, table: &data::Table, data: &data::ObjectValues, fail_on_duplicate: bool) -> Result<data::RawTableData, TableError>;

    fn upsert_row(conn: &S, table: &data::Table, data: &data::ObjectValues) -> Result<data::RawTableData, TableError>;

    fn update_row(conn: &S, table: &data::Table, keys: &data::ObjectKeys, data: &data::ObjectValues, fail_on_not_found: bool) -> Result<data::RawTableData, TableError>;

    fn delete_row(conn: &S, table: &data::Table, keys: &data::ObjectKeys, fail_on_not_found: bool) -> Result<data::RawTableData, TableError>;
}

impl TableActionFunctions<State> for TableAction {
    fn query(conn: &State, table: &data::Table) -> Result<data::RawTableData, TableError> {

        let query = format!("SELECT * FROM {}", &table.name);
        Database::exec(conn.get_conn(), &query, vec![])
            .or_else(|err| Err(TableError::db_error(err)))
    }

    fn insert_row(conn: &State, table: &data::Table, data: &data::ObjectValues, fail_on_duplicate: bool) -> Result<data::RawTableData, TableError> {
        let raw_data = data.as_list();
        let mut results = data::RawTableData::new();

        for row in raw_data {
            let column_names: Vec<String> = row.keys().map(|x| x.to_owned()).collect();
            let column_counts: Vec<String> = column_names.iter().enumerate()
                .map(|(i, _)| format!("${}", i+1))
                .collect();
            let values = row.values().map(|x| x.to_owned()).collect();
            let query = format!(
                "INSERT INTO {name} ({columns}) VALUES ({params}) RETURNING *",
                name=table.name,
                columns=column_names.join(","),
                params=column_counts.join(","),
            );

            let new_row = Database::exec(conn.get_conn(), &query, values)
                .or_else(|err| {
                    match err {
                        DbError::AlreadyExists => if !fail_on_duplicate {
                            Ok(data::RawTableData::new())
                        } else {
                            Err(TableError::db_error(err))
                        },
                        _ => Err(TableError::db_error(err)),
                    }
                })?;

            results.append(new_row);
        }

        Ok(results)
    }

    fn upsert_row(conn: &State, table: &data::Table, data: &data::ObjectValues) -> Result<data::RawTableData, TableError> {
        //TODO: doing this because I want to know whether it was an insert or update so that I can put in the correct data in the transactions table
        // otherise, maybe ON CONFLICT with triggers would have been the proper choice
        Database::exec(conn.get_conn(), "SELECT id FROM table WHERE id = my_id", vec![]);
        Database::exec(conn.get_conn(), "INSERT INTO table (value1, value2, value3) VALUES (1, 2, 3)", vec![]);
        Database::exec(conn.get_conn(), "UPDATE table SET value1 = 1, value2 = 2 WHERE id = my_id", vec![]);
        unimplemented!()
    }

    fn update_row(conn: &State, table: &data::Table, keys: &data::ObjectKeys, data: &data::ObjectValues, fail_on_not_found: bool) -> Result<data::RawTableData, TableError> {

        let raw_keys = keys.as_list();
        let raw_data = data.as_list();
        let mut results = data::RawTableData::new();

        for (key, row) in raw_keys.iter().zip(raw_data) {
            let column_names: Vec<String> = row.keys().map(|x| x.to_owned()).collect();
            let key_names: Vec<String> = key.keys().map(|x| x.to_owned()).collect();

            let mut values: Vec<data::Value> = row.values().map(|x| x.to_owned()).collect();
            let key_values: Vec<data::Value> = key.values().map(|x| x.to_owned().into_value()).collect();
            values.extend(key_values);

            let val_index = 1;
            let key_index = column_names.len() + 1;

            let query = format!(
                "UPDATE {name} SET {sets} WHERE {id} RETURNING *", //"UPDATE table SET value1 = 1, value2 = 2 WHERE id = my_id"
                name=table.name,
                sets=column_names.iter().enumerate()
                    .map(|(i, x)| format!("{} = ${}", x, i+val_index))
                    .collect::<Vec<String>>()
                    .join(","),
                id=key_names.iter().enumerate()
                    .map(|(i, x)| format!("{} = ${}", x, i+key_index))
                    .collect::<Vec<String>>()
                    .join(" AND "),
            );

            let new_row = Database::exec(conn.get_conn(), &query, values)
                .or_else(|err| {
                    match err {
                        DbError::NotFound => if !fail_on_not_found {
                            Ok(data::RawTableData::new())
                        } else {
                            Err(TableError::db_error(err))
                        },
                        _ => Err(TableError::db_error(err)),
                    }
                })?;

            results.append(new_row);
        }

        Ok(results)

    }

    fn delete_row(conn: &State, table: &data::Table, keys: &data::ObjectKeys, fail_on_not_found: bool) -> Result<data::RawTableData, TableError> {
        let raw_keys = keys.as_list();
        let mut results = data::RawTableData::new();

        for key in raw_keys {
            let key_names: Vec<String> = key.keys().map(|x| x.to_owned()).collect();
            let values: Vec<data::Value> = key.values().map(|x| x.to_owned().into_value()).collect();

            let query = format!(
                "DELETE {name} WHERE {id} RETURNING *", //"DELETE table WHERE id = my_id"
                name=table.name,
                id=key_names.iter().enumerate()
                    .map(|(i, x)| format!("{} = ${}", x, i+1))
                    .collect::<Vec<String>>()
                    .join(" AND "),
            );

            let new_row = Database::exec(conn.get_conn(), &query, values)
                .or_else(|err| {
                    match err {
                        DbError::NotFound => if !fail_on_not_found {
                            Ok(data::RawTableData::new())
                        } else {
                            Err(TableError::db_error(err))
                        },
                        _ => Err(TableError::db_error(err)),
                    }
                })?;

            results.append(new_row);
        }

        Ok(results)
    }
}