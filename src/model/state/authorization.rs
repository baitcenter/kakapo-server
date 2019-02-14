use std::collections::HashSet;
use data::permissions::Permission;
use model::state::error::UserManagementError;

pub trait AuthorizationOps {
    fn user_id(&self) -> Option<i64>;

    fn is_admin(&self) -> bool;

    /// returns a hashset of permissions if the user is logged in
    /// otherwise returns none
    fn permissions(&self) -> Option<HashSet<Permission>>;

    fn all_permissions(&self) -> HashSet<Permission>;

    fn username(&self) -> Option<String>;

}