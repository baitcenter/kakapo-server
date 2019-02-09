
use serde_json;

use std::result::Result;
use std::result::Result::Ok;

use diesel::{r2d2::ConnectionManager, r2d2::PooledConnection};
use diesel::pg::PgConnection;

use connection::executor::Conn;
use diesel::Connection;
use model::entity::RawEntityTypes;
use scripting::Scripting;
use connection::Broadcaster;
use std::sync::Arc;
use serde::Serialize;
use model::actions::error::Error;
use std::fmt::Debug;
use std::fmt;
use connection::executor::Secrets;
use metastore::auth_modifier::AuthFunctions;
use metastore::auth_modifier::Auth;
use model::entity::Controller;
use model::entity::RetrieverFunctions;
use model::entity::ModifierFunctions;
use model::table::TableAction;
use model::table::TableActionFunctions;
use std::marker::PhantomData;
use metastore::permission_store::PermissionStoreFunctions;
use data::auth::Permission;
use std::collections::HashSet;
use std::iter::FromIterator;
use metastore::permission_store::PermissionStore;

pub struct ActionState {
    pub database: Conn, //TODO: this should be templated
    pub scripting: Scripting,
    pub claims: Option<AuthClaims>,
    pub broadcaster: Arc<Broadcaster>,
    pub secrets: Secrets,
}

impl fmt::Debug for ActionState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ActionState")
    }
}

pub trait StateFunctions<'a>
    where
        Self: Debug + Send,
        Self::TableController: TableActionFunctions,
        Self::UserInfo: GetUserInfo,
        //TODO: managementstore
        Self::EntityRetrieverFunctions: RetrieverFunctions,
        Self::EntityModifierFunctions: ModifierFunctions,
        //managementstore
        Self::AuthFunctions: AuthFunctions,
        Self::PermissionStore: PermissionStoreFunctions,
{
    type UserInfo;
    fn get_user_info(&'a self) -> Self::UserInfo;

    type AuthFunctions;
    fn get_auth_functions(&'a self) -> Self::AuthFunctions;

    type PermissionStore;
    fn get_permission(&'a self) -> Self::PermissionStore;

    type EntityRetrieverFunctions;
    fn get_entity_retreiver_functions(&'a self) -> Self::EntityRetrieverFunctions;

    type EntityModifierFunctions;
    fn get_entity_modifier_function(&'a self) -> Self::EntityModifierFunctions;

    type TableController;
    fn get_table_controller(&'a self) -> Self::TableController;

    type Scripting;
    fn get_script_runner(&'a self) -> Self::Scripting;

    type Database;
    fn get_database(&'a self) -> Self::Database;

    fn transaction<G, E, F>(&self, f: F) -> Result<G, E> //TODO: why is it a diesel::result::Error?
        where F: FnOnce() -> Result<G, E>, E: From<diesel::result::Error>;
}

impl<'a> StateFunctions<'a> for ActionState {
    type UserInfo = UserInfo<'a, Self::PermissionStore>;
    fn get_user_info(&'a self) -> Self::UserInfo {
        let permission_store: PermissionStore<'a> = PermissionStore {
            conn: &self.database,
        };

        UserInfo {
            permission_store,
            claims: &self.claims,
        }
    }

    type AuthFunctions = Auth<'a>;
    fn get_auth_functions(&'a self) -> Auth<'a> {
        let password_secret = self.get_password_secret();
        Auth::new(
            &self.database,
            password_secret.to_owned(),
        )
    }

    type PermissionStore = PermissionStore<'a>;
    fn get_permission(&'a self) -> Self::PermissionStore {
        PermissionStore {
            conn: &self.database,
        }
    }

    type EntityRetrieverFunctions = Controller<'a>;
    fn get_entity_retreiver_functions(&'a self) -> Self::EntityRetrieverFunctions {
        Controller {
            conn: &self.database,
            claims: &self.claims,
        }
    }

    type EntityModifierFunctions = Controller<'a>;
    fn get_entity_modifier_function(&'a self) -> Self::EntityModifierFunctions {
        Controller {
            conn: &self.database,
            claims: &self.claims,
        }
    }

    type TableController = TableAction<'a>;
    fn get_table_controller(&'a self) -> Self::TableController {
        TableAction {
            conn: &self.database,
        }
    }

    type Scripting = Scripting;
    fn get_script_runner(&'a self) -> Self::Scripting {
        self.scripting.clone()
    }

    type Database = &'a Conn;
    fn get_database(&'a self) -> Self::Database {
        &self.database
    }

    fn transaction<G, E, F>(&self, f: F) -> Result<G, E> //TODO: should work for all state actions
        where F: FnOnce() -> Result<G, E>, E: From<diesel::result::Error> {
        let conn = &self.database;
        conn.transaction::<G, E, _>(f)
    }
}

#[derive(Debug, Clone, Serialize)]
pub enum Channels {
    AllTables,
    AllQueries,
    AllScripts,
    Table(String),
    Query(String),
    Script(String),
    TableData(String),
}

impl Channels {
    pub fn all_entities<T>() -> Self
        where T: RawEntityTypes,
    {
        Channels::AllTables
    }

    pub fn entity<T>(name: &str) -> Self
        where T: RawEntityTypes,
    {
        Channels::Table(name.to_string())
    }

    pub fn table(table_name: &str) -> Self {
        Channels::TableData(table_name.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthClaims {
    iss: String,
    sub: i64, // == user_id
    iat: i64,
    exp: i64,
    username: String,
    is_admin: bool,
    role: Option<String>, //the default role that the user is interacting with
}

impl AuthClaims {
    pub fn get_user_id(&self) -> i64 {
        self.sub
    }

    pub fn get_username(&self) -> String {
        self.username.to_owned()
    }

    pub fn is_user_admin(&self) -> bool {
        self.is_admin
    }
}

impl ActionState {
    //TODO: this has too many parameters
    pub fn new(
        database: Conn,
        scripting: Scripting,
        claims: Option<AuthClaims>,
        broadcaster: Arc<Broadcaster>,
        secrets: Secrets,
    ) -> Self {
        Self {
            database,
            scripting,
            claims,
            broadcaster,
            secrets,
        }
    }
}

pub struct UserInfo<'a, P> {
    permission_store: P,
    claims: &'a Option<AuthClaims>,
}

pub trait GetUserInfo {
    fn user_id(&self) -> Option<i64>;

    fn is_admin(&self) -> bool;

    /// returns a hashset of permissions if the user is logged in
    /// otherwise returns none
    fn permissions(&self) -> Option<HashSet<Permission>>;

    fn all_permissions(&self) -> HashSet<Permission>;

    fn username(&self) -> Option<String>;

}

/// Note that the permissions here are grabbed from either the jwt, or the
/// database
impl<'a, P> GetUserInfo for UserInfo<'a, P>
    where P: PermissionStoreFunctions
{
    fn user_id(&self) -> Option<i64> {
        self.claims.to_owned().map(|x| x.get_user_id())
    }

    fn is_admin(&self) -> bool {
        self.claims.to_owned().map(|x| x.is_user_admin()).unwrap_or(false)
    }

    fn permissions(&self) -> Option<HashSet<Permission>> {
        self.user_id().map(|user_id| {
            let raw_permissions_result = self.permission_store.get_user_permissions(user_id);
            let raw_permissions = match raw_permissions_result {
                Ok(res) => res,
                Err(err) => {
                    error!("encountered an error when trying to get all permissions: {:?}", err);
                    vec![]
                }
            };

            let permissions = raw_permissions.into_iter()
                .flat_map(|raw_permission| {
                    raw_permission.as_permission()
                });

            HashSet::from_iter(permissions)
        })
    }

    fn all_permissions(&self) -> HashSet<Permission> {
        let raw_permissions_result = self.permission_store.get_all_permissions();
        let raw_permissions = match raw_permissions_result {
            Ok(res) => res,
            Err(err) => {
                error!("encountered an error when trying to get all permissions: {:?}", err);
                vec![]
            }
        };

        let permissions = raw_permissions.into_iter()
            .flat_map(|raw_permission| {
                raw_permission.as_permission()
            });

        HashSet::from_iter(permissions)
    }

    fn username(&self) -> Option<String> {
        self.claims.to_owned().map(|x| x.get_username())
    }
}

pub trait GetBroadcaster {
    fn publish<R>(&self, channels: Vec<Channels>, action_name: String, action_result: &R) -> Result<(), Error>
        where R: Serialize;
}

impl GetBroadcaster for ActionState {
    fn publish<R>(&self, channels: Vec<Channels>, action_name: String, action_result: &R) -> Result<(), Error>
        where R: Serialize
    {
        let payload = serde_json::to_value(action_result)
            .or_else(|err| Err(Error::SerializationError(err.to_string())))?;


        self.broadcaster.publish(channels, action_name, payload)
            .or_else(|err| Err(Error::PublishError(err)))?;

        Ok(())
    }
}

pub trait GetSecrets {
    fn get_token_secret(&self) -> String;
    fn get_password_secret(&self) -> String;
}

impl GetSecrets for ActionState {
    fn get_token_secret(&self) -> String {
        self.secrets.token_secret.to_owned()
    }

    fn get_password_secret(&self) -> String {
        self.secrets.password_secret.to_owned()

    }
}
