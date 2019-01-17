
use actix::prelude::*;

use serde_json;

use std::result::Result;
use std::result::Result::Ok;
use std::marker::PhantomData;

use diesel::{r2d2::ConnectionManager, r2d2::PooledConnection};
use diesel::pg::PgConnection;
use diesel::Connection;

use data;

use connection::py::PyRunner;

use model::entity;
use model::entity::RetrieverFunctions;
use model::entity::ModifierFunctions;
use model::entity::error::EntityError;

use model::schema;

use model::actions::results::*;
use model::actions::error::Error;
use data::utils::OnDuplicate;

use data::utils::OnNotFound;
use model::entity::conversion;
use model::dbdata::RawEntityTypes;

use model::entity::results::Upserted;
use model::entity::results::Created;
use model::entity::results::Updated;
use model::entity::results::Deleted;
use data::utils::TableDataFormat;
use model::table;
use model::table::TableActionFunctions;
use connection::executor::Conn;
use model::state::State;
use model::state::GetConnection;
use model::state::Channels;
use model::auth::permissions::*;
use std::iter::FromIterator;

use model::actions::Action;

///decorator for permission
pub struct WithPermissionRequired<A, S = State, AU = AuthPermissions>
    where
        A: Action<S>,
        AU: AuthPermissionFunctions + Send,
{
    action: A,
    permission: Permission,
    phantom_data: PhantomData<(S, AU)>,
}

impl<A, S, AU> WithPermissionRequired<A, S, AU>
    where
        A: Action<S>,
        AU: AuthPermissionFunctions + Send,
{
    pub fn new(action: A, permission: Permission) -> Self {
        Self {
            action,
            permission,
            phantom_data: PhantomData,
        }
    }
}

impl<A, AU> Action<State> for WithPermissionRequired<A, State, AU>
    where
        A: Action<State>,
        AU: AuthPermissionFunctions + Send,
{
    type Ret = A::Ret;
    fn call(&self, state: &State) -> Result<Self::Ret, Error> {
        if AU::is_admin(state) {
            return self.action.call(state);
        }

        let user_permissions = AU::get_permissions(state).unwrap_or_default();
        if user_permissions.contains(&self.permission) {
            self.action.call(state)
        } else {
            debug!("Permission denied, required permission: {:?}", &self.permission);
            Err(Error::Unauthorized)
        }

    }
}

///decorator for login
pub struct WithLoginRequired<A, S = State, AU = AuthPermissions>
    where
        A: Action<S>,
        AU: AuthPermissionFunctions + Send,
{
    action: A,
    phantom_data: PhantomData<(S, AU)>,
}

impl<A, S, AU> WithLoginRequired<A, S, AU>
    where
        A: Action<S>,
        AU: AuthPermissionFunctions + Send,
{
    pub fn new(action: A) -> Self {
        Self {
            action,
            phantom_data: PhantomData,
        }
    }
}

impl<A, AU> Action<State> for WithLoginRequired<A, State, AU>
    where
        A: Action<State>,
        AU: AuthPermissionFunctions + Send,
{
    type Ret = A::Ret;
    fn call(&self, state: &State) -> Result<Self::Ret, Error> {
        if AU::is_admin(state) {
            return self.action.call(state);
        }

        let user_permissions = AU::get_permissions(state);
        match user_permissions {
            None => {
                debug!("Permission denied, required login");
                Err(Error::Unauthorized)
            },
            Some(_) => self.action.call(state)
        }
    }
}

///decorator for permission after the value is returned
/// Warning: this should always be wrapped in a transaction decorator, otherwise, you will modify the state
pub struct WithPermissionRequiredOnReturn<A, S = State, AU = AuthPermissions>
    where
        A: Action<S>,
        AU: AuthPermissionFunctions + Send,
{
    action: A,
    initial_permission: Permission,
    required_permission: Box<Fn(&A::Ret) -> Option<Permission> + Send>,
    phantom_data: PhantomData<(S, AU)>,
}

impl<A, S, AU> WithPermissionRequiredOnReturn<A, S, AU>
    where
        A: Action<S>,
        Self: Action<S>,
        S: GetConnection + Send,
        AU: AuthPermissionFunctions + Send,
{
    pub fn new<F>(action: A, permission: Permission, required_permission: F) -> Self
        where
            F: Send + Fn(&A::Ret) -> Option<Permission> + 'static,
    {
        Self {
            action,
            initial_permission: permission,
            required_permission: Box::new(required_permission),
            phantom_data: PhantomData,
        }
    }
}

impl<A, AU> Action<State> for WithPermissionRequiredOnReturn<A, State, AU>
    where
        A: Action<State>,
        AU: AuthPermissionFunctions + Send,
{
    type Ret = A::Ret;
    fn call(&self, state: &State) -> Result<Self::Ret, Error> {
        if AU::is_admin(state) {
            return self.action.call(state);
        }

        let user_permissions = AU::get_permissions(state).unwrap_or_default();
        if user_permissions.contains(&self.initial_permission) {
            let result = self.action.call(state)?;
            match (self.required_permission)(&result) {
                None => Ok(result),
                Some(next_permission) => if user_permissions.contains(&self.initial_permission) {
                    Ok(result)
                } else {
                    Err(Error::Unauthorized)
                }
            }
        } else {
            debug!("Permission denied, required permission: {:?}", &self.initial_permission);
            Err(Error::Unauthorized)
        }

    }
}

///decorator for transactions
pub struct WithTransaction<A, S = State>
    where
        A: Action<S>,
        S: GetConnection + Send,
{
    action: A,
    phantom_data: PhantomData<S>,
}

impl<A, S> WithTransaction<A, S>
    where
        A: Action<S>,
        S: GetConnection + Send,
        Self: Action<S>,
{
    pub fn new(action: A) -> Self {
        Self {
            action,
            phantom_data: PhantomData,
        }
    }
}

impl<A> Action<State> for WithTransaction<A, State>
    where
        A: Action<State>,
{
    type Ret = A::Ret;
    fn call(&self, state: &State) -> Result<Self::Ret, Error> {
        debug!("started transaction");
        let conn = state.get_conn();
        conn.transaction::<Self::Ret, Error, _>(||
            self.action.call(state)
        )

    }
}

///decorator for dispatching to channel
pub struct WithDispatch<A, S = State>
    where
        A: Action<S>,
{
    action: A,
    channel: Channels,
    phantom_data: PhantomData<S>,
}

impl<A, S> WithDispatch<A, S>
    where
        A: Action<S>,
        S: GetConnection + Send,
{
    pub fn new(action: A, channel: Channels) -> Self {
        Self {
            action,
            channel,
            phantom_data: PhantomData,
        }
    }
}

impl<A, S> Action<S> for WithDispatch<A, S>
    where
        A: Action<S>,
        S: GetConnection + Send,
{
    type Ret = A::Ret;
    fn call(&self, state: &S) -> Result<Self::Ret, Error> {
        debug!("dispatching action");

        let result = self.action.call(state)?;

        Ok(result)
    }
}
