
pub mod auth;
pub mod permission_store;
pub mod error;

use serde_json;

use std::fmt::Debug;
use std::fmt;
use std::sync::Arc;

use diesel::Connection;
use scripting::Scripting;
use connection::Broadcaster;
use serde::Serialize;
use model::actions::error::Error;

use connection::executor::Conn;
use connection::executor::Secrets;

use metastore::auth_modifier::Auth;
use metastore::permission_store::PermissionStore;

use model::entity::EntityRetrieverController;
use model::entity::EntityModifierController;
use model::entity::RetrieverFunctions;
use model::entity::ModifierFunctions;
use model::table::TableAction;
use model::table::TableActionFunctions;
use model::auth::GetUserInfo;
use model::auth::UserInfo;
use model::auth::send_mail::EmailSender;
use model::auth::send_mail::EmailOps;

use scripting::ScriptFunctions;

use data::claims::AuthClaims;
use Channels;
use model::state::auth::AuthFunctions;
use model::state::permission_store::PermissionStoreFunctions;

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
        Self::Scripting: ScriptFunctions,
        //TODO: managementstore
        Self::EntityRetrieverFunctions: RetrieverFunctions,
        Self::EntityModifierFunctions: ModifierFunctions,
        //managementstore
        Self::AuthFunctions: AuthFunctions,
        Self::PermissionStore: PermissionStoreFunctions,
        Self::EmailSender: EmailOps,
{
    // user managment
    type UserInfo;
    fn get_user_info(&'a self) -> Self::UserInfo;

    type AuthFunctions;
    fn get_auth_functions(&'a self) -> Self::AuthFunctions;

    type PermissionStore;
    fn get_permission(&'a self) -> Self::PermissionStore;

    // tables management
    type EntityRetrieverFunctions;
    fn get_entity_retreiver_functions(&'a self) -> Self::EntityRetrieverFunctions;

    type EntityModifierFunctions;
    fn get_entity_modifier_function(&'a self) -> Self::EntityModifierFunctions;

    // table actions
    type TableController;
    fn get_table_controller(&'a self) -> Self::TableController;

    type Scripting;
    fn get_script_runner(&'a self) -> Self::Scripting;

    type Database;
    fn get_database(&'a self) -> Self::Database;

    type EmailSender;
    fn get_email_sender(&'a self) -> Self::EmailSender;

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

    type EntityRetrieverFunctions = EntityRetrieverController<'a>;
    fn get_entity_retreiver_functions(&'a self) -> Self::EntityRetrieverFunctions {
        EntityRetrieverController {
            conn: &self.database,
            claims: &self.claims,
        }
    }

    type EntityModifierFunctions = EntityModifierController<'a>;
    fn get_entity_modifier_function(&'a self) -> Self::EntityModifierFunctions {
        let password_secret = self.get_password_secret();
        let auth = Auth::new(
            &self.database,
            password_secret.to_owned(),
        );

        EntityModifierController {
            conn: &self.database,
            claims: &self.claims,
            scripting: &self.scripting,
            auth_permissions: auth,
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

    type EmailSender = EmailSender;
    fn get_email_sender(&'a self) -> Self::EmailSender {
        EmailSender {}
    }

    fn transaction<G, E, F>(&self, f: F) -> Result<G, E> //TODO: should work for all state actions
        where F: FnOnce() -> Result<G, E>, E: From<diesel::result::Error> {
        let conn = &self.database;
        conn.transaction::<G, E, _>(f)
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