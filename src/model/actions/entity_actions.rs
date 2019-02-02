use serde_json;

use std::result::Result;
use std::result::Result::Ok;
use std::marker::PhantomData;

use data;

use connection::py::PyRunner;

use model::entity;
use model::entity::RetrieverFunctions;
use model::entity::ModifierFunctions;
use model::entity::error::EntityError;

use data::schema;

use model::actions::results::*;
use model::actions::error::Error;
use data::utils::OnDuplicate;

use data::utils::OnNotFound;
use data::conversion;
use data::dbdata::RawEntityTypes;

use model::entity::results::Upserted;
use model::entity::results::Created;
use model::entity::results::Updated;
use model::entity::results::Deleted;
use data::utils::TableDataFormat;

use model::table;
use model::table::TableActionFunctions;
use model::query;
use model::query::QueryActionFunctions;
use model::script;
use model::script::ScriptActionFunctions;

use connection::executor::Conn;
use model::state::State;
use model::state::GetConnection;
use model::state::Channels;
use model::auth::permissions::*;
use std::iter::FromIterator;

use model::actions::decorator::*;
use std::fmt::Debug;

use model::auth::Auth;
use model::auth::AuthFunctions;
use model::actions::Action;
use model::actions::ActionRes;
use model::actions::ActionResult;
use model::auth::permissions::GetUserInfo;


///decorator for permission in listing items
/// Only defined for GetAllEntities
pub struct WithFilterListByPermission<A, T, S = State, ER = entity::Controller>
    where
        A: Action<S, Ret = GetAllEntitiesResult<T>>,
        T: RawEntityTypes,
        ER: RetrieverFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    action: A,
    phantom_data: PhantomData<(T, S, ER)>,
}

impl<A, T, S, ER> WithFilterListByPermission<A, T, S, ER>
    where
        A: Action<S, Ret = GetAllEntitiesResult<T>>,
        T: RawEntityTypes,
        ER: RetrieverFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    pub fn new(action: A) -> Self {
        Self {
            action,
            phantom_data: PhantomData,
        }
    }
}

impl<A, T, S, ER> Action<S> for WithFilterListByPermission<A, T, S, ER>
    where
        A: Action<S, Ret = GetAllEntitiesResult<T>>,
        T: RawEntityTypes,
        ER: RetrieverFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    type Ret = <GetAllEntities<T, S, ER> as Action<S>>::Ret;
    fn call(&self, state: &S) -> ActionResult<Self::Ret> {
        let user_permissions = S::get_permissions(state).unwrap_or_default();
        let raw_results = self.action.call(state)?;

        let GetAllEntitiesResult(inner_results) = raw_results;

        debug!("filtering list based on permissions");
        let filtered_results = inner_results.into_iter()
            .filter(|x| {
                let required = Permission::read_entity::<T>(x.get_name());
                user_permissions.contains(&required)
            })
            .collect();

        Ok(GetAllEntitiesResult(filtered_results))
    }
}

///get all tables
#[derive(Debug, Clone)]
pub struct GetAllEntities<T, S = State, ER = entity::Controller>
    where
        T: RawEntityTypes,
{
    pub show_deleted: bool,
    pub phantom_data: PhantomData<(T, S, ER)>,
}

impl<T, S, ER> GetAllEntities<T, S, ER>
    where
        T: RawEntityTypes,
        ER: RetrieverFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    pub fn new(show_deleted: bool) -> WithFilterListByPermission<WithTransaction<Self, S>, T, S, ER> {
        let action = Self {
            show_deleted,
            phantom_data: PhantomData,
        };

        let action_with_transaction = WithTransaction::new(action);
        let action_with_filter = WithFilterListByPermission::new(action_with_transaction);

        action_with_filter
    }
}

impl<T, S, ER> Action<S> for GetAllEntities<T, S, ER>
    where
        T: RawEntityTypes,
        ER: RetrieverFunctions<T, S> + Send,
        S: GetConnection + GetUserInfo,
{
    type Ret = GetAllEntitiesResult<T>;
    fn call(&self, state: &S) -> ActionResult<Self::Ret> {
        let entities: Vec<T> = ER::get_all(state)
            .or_else(|err| Err(Error::Entity(err)))?;
        ActionRes::new(GetAllEntitiesResult::<T>(entities))
    }
}

///get one table
#[derive(Debug, Clone)]
pub struct GetEntity<T, S = State, ER = entity::Controller>
    where
        T: RawEntityTypes,
{
    pub name: String,
    pub phantom_data: PhantomData<(T, S, ER)>,
}

impl<T, S, ER> GetEntity<T, S, ER>
    where
        T: RawEntityTypes,
        ER: RetrieverFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    pub fn new(name: String) -> WithPermissionRequired<WithTransaction<GetEntity<T, S, ER>, S>, S> { //weird syntax but ok
        let action = Self {
            name: name.to_owned(),
            phantom_data: PhantomData,
        };
        let action_with_transaction = WithTransaction::new(action);
        let action_with_permission =
            WithPermissionRequired::new(action_with_transaction, Permission::read_entity::<T>(name));

        action_with_permission
    }
}

impl<T, S, ER> Action<S> for GetEntity<T, S, ER>
    where
        T: RawEntityTypes,
        ER: RetrieverFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    type Ret = GetEntityResult<T>;
    fn call(&self, state: &S) -> ActionResult<Self::Ret> {
        let maybe_entity: Option<T> = ER::get_one(state, &self.name)
            .or_else(|err| Err(Error::Entity(err)))?;

        match maybe_entity {
            Some(entity) => ActionRes::new(GetEntityResult::<T>(entity)),
            None => Err(Error::NotFound),
        }
    }
}

///create one table
#[derive(Debug, Clone)]
pub struct CreateEntity<T, S = State, EM = entity::Controller>
    where
        T: RawEntityTypes,
        EM: ModifierFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    pub data: T,
    pub on_duplicate: OnDuplicate,
    pub phantom_data: PhantomData<(S, EM)>,
}

impl<T, S, EM> CreateEntity<T, S, EM>
    where
        T: RawEntityTypes,
        EM: ModifierFunctions<T, S>,
        S: GetConnection + GetUserInfo,
        <Self as Action<S>>::Ret: Clone,
{
    pub fn new(data: T) -> WithPermissionFor<WithDispatch<WithTransaction<Self, S>, S>, S> {

        let name = data.get_name();
        let create_permission = Permission::create_entity::<T>();
        let update_permission = Permission::modify_entity::<T>(name);
        let on_duplicate = OnDuplicate::Ignore; //TODO:...
        let channel = Channels::all_entities::<T>(); //TODO: on update this should have table as well

        let action = Self {
            data,
            on_duplicate: OnDuplicate::Ignore,  //TODO:...
            phantom_data: PhantomData,
        };

        let action_with_transaction = WithTransaction::new(action);
        let action_with_dispatch = WithDispatch::new(action_with_transaction, channel);
        let action_with_permission =
            WithPermissionFor::new(
                action_with_dispatch,
                move |user_permissions, all_permissions| {
                    match on_duplicate {
                        OnDuplicate::Update => if all_permissions.contains(&update_permission) {
                            user_permissions.contains(&update_permission)
                        } else {
                            user_permissions.contains(&create_permission)
                        },
                        _ => user_permissions.contains(&create_permission),
                    }
                });



        action_with_permission
    }
}

impl<T, S, EM> Action<S> for CreateEntity<T, S, EM>
    where
        T: RawEntityTypes,
        EM: ModifierFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    type Ret = CreateEntityResult<T>;
    fn call(&self, state: &S) -> ActionResult<Self::Ret> {
        match &self.on_duplicate {
            OnDuplicate::Update => {
                EM::upsert(state, self.data.clone())
                    .or_else(|err| Err(Error::Entity(err)))
                    .and_then(|res| {
                        match res {
                            Upserted::Update { old, new } => ActionRes::new(CreateEntityResult::Updated { old, new }),
                            Upserted::Create { new } => ActionRes::new(CreateEntityResult::Created(new)),
                        }
                    })
            },
            OnDuplicate::Ignore => {
                EM::create(state, self.data.clone())
                    .or_else(|err| Err(Error::Entity(err)))
                    .and_then(|res| {
                        match res {
                            Created::Success { new } => ActionRes::new(CreateEntityResult::Created(new)),
                            Created::Fail { existing } => ActionRes::new(CreateEntityResult::AlreadyExists { existing, requested: self.data.clone() } ),
                        }
                    })

            },
            OnDuplicate::Fail => {
                EM::create(state, self.data.clone())
                    .or_else(|err| Err(Error::Entity(err)))
                    .and_then(|res| {
                        match res {
                            Created::Success { new } => ActionRes::new(CreateEntityResult::Created(new)),
                            Created::Fail { .. } => Err(Error::AlreadyExists),
                        }
                    })
            },
        }
    }
}

///update table
#[derive(Debug)]
pub struct UpdateEntity<T, S = State, EM = entity::Controller>
    where
        T: RawEntityTypes,
        EM: ModifierFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    pub name: String,
    pub data: T,
    pub on_not_found: OnNotFound,
    pub phantom_data: PhantomData<(S, EM)>,
}

impl<T, S, EM> UpdateEntity<T, S, EM>
    where
        T: RawEntityTypes,
        EM: ModifierFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    pub fn new(name: String, data: T) -> WithPermissionRequired<WithDispatch<WithTransaction<Self, S>, S>, S> {
        let channels = vec![
            Channels::all_entities::<T>(),
            Channels::entity::<T>(&name),
        ];
        let action = Self {
            name: name.to_owned(),
            data,
            on_not_found: OnNotFound::Ignore,
            phantom_data: PhantomData,
        };

        let action_with_transaction = WithTransaction::new(action);
        let action_with_dispatch = WithDispatch::new_multi(action_with_transaction, channels);
        let action_with_permission =
            WithPermissionRequired::new(action_with_dispatch, Permission::modify_entity::<T>(name));

        action_with_permission
    }
}

impl<T, S, EM> Action<S> for UpdateEntity<T, S, EM>
    where
        T: RawEntityTypes,
        EM: ModifierFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    type Ret = UpdateEntityResult<T>;
    fn call(&self, state: &S) -> ActionResult<Self::Ret> {
        match &self.on_not_found {
            OnNotFound::Ignore => {
                EM::update(state, (&self.name, self.data.clone()))
                    .or_else(|err| Err(Error::Entity(err)))
                    .and_then(|res| {
                        match res {
                            Updated::Success { old, new } =>
                                ActionRes::new(UpdateEntityResult::Updated { id: self.name.to_owned(), old, new }),
                            Updated::Fail =>
                                ActionRes::new(UpdateEntityResult::NotFound { id: self.name.to_owned(), requested: self.data.clone() }),
                        }
                    })

            },
            OnNotFound::Fail => {
                EM::update(state, (&self.name, self.data.clone()))
                    .or_else(|err| Err(Error::Entity(err)))
                    .and_then(|res| {
                        match res {
                            Updated::Success { old, new } =>
                                ActionRes::new(UpdateEntityResult::Updated { id: self.name.to_owned(), old, new }),
                            Updated::Fail => Err(Error::NotFound),
                        }
                    })
            },
        }
    }
}

///delete table
#[derive(Debug)]
pub struct DeleteEntity<T, S = State, EM = entity::Controller>
    where
        T: RawEntityTypes,
        EM: ModifierFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    pub name: String,
    pub on_not_found: OnNotFound,
    pub phantom_data: PhantomData<(T, S, EM)>,
}

impl<T, S, EM> DeleteEntity<T, S, EM>
    where
        T: RawEntityTypes,
        EM: ModifierFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    pub fn new(name: String) -> WithPermissionRequired<WithDispatch<WithTransaction<Self, S>, S>, S> {
        let channels = vec![
            Channels::all_entities::<T>(),
            Channels::entity::<T>(&name),
        ];
        let action = Self {
            name: name.to_owned(),
            on_not_found: OnNotFound::Ignore,
            phantom_data: PhantomData,
        };

        let action_with_transaction = WithTransaction::new(action);
        let action_with_dispatch = WithDispatch::new_multi(action_with_transaction, channels);
        let action_with_permission =
            WithPermissionRequired::new(action_with_dispatch, Permission::modify_entity::<T>(name));

        action_with_permission
    }
}

impl<T, S, EM> Action<S> for DeleteEntity<T, S, EM>
    where
        T: RawEntityTypes,
        EM: ModifierFunctions<T, S>,
        S: GetConnection + GetUserInfo,
{
    type Ret = DeleteEntityResult<T>;
    fn call(&self, state: &S) -> ActionResult<Self::Ret> {
        match &self.on_not_found {
            OnNotFound::Ignore => {
                EM::delete(state, &self.name)
                    .or_else(|err| Err(Error::Entity(err)))
                    .and_then(|res| {
                        match res {
                            Deleted::Success { old } =>
                                ActionRes::new(DeleteEntityResult::Deleted { id: self.name.to_owned(), old } ),
                            Deleted::Fail => ActionRes::new(DeleteEntityResult::NotFound(self.name.to_owned())),
                        }
                    })

            },
            OnNotFound::Fail => {
                EM::delete(state, &self.name)
                    .or_else(|err| Err(Error::Entity(err)))
                    .and_then(|res| {
                        match res {
                            Deleted::Success { old } =>
                                ActionRes::new(DeleteEntityResult::Deleted { id: self.name.to_owned(), old } ),
                            Deleted::Fail => Err(Error::NotFound),
                        }
                    })
            },
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;


    use diesel::{r2d2::ConnectionManager, r2d2::PooledConnection};
    use diesel::pg::PgConnection;
    use diesel::Connection;

    use serde_json::from_value;
    use scripting::Scripting;
    use diesel::r2d2::Pool;

    fn get_state() -> State {
        let script_path = "./path/to/scripts".to_string();
        let conn_url ="postgres://test:password@localhost:5432/test".to_string();
        let conn_manager = ConnectionManager::new(conn_url);
        let pool = Pool::new(conn_manager).unwrap();
        let pooled_conn = pool.get().unwrap();
        State::new(pooled_conn, Scripting::new(script_path), None)
    }

    #[test]
    fn test_create_entity() {
        let conn = PgConnection::establish("postgres://test:password@localhost:5432/test").unwrap();
        let state = get_state();

        let new_query: data::Query = from_value(json!({
            "name": "my_query",
            "description": "blah blah blah",
            "statement": "SELECT * FROM a_table"
        })).unwrap();
        let create_action = CreateEntity::<data::Query>::new(new_query);

        let result = create_action.call(&state);
    }
}