
use actix::prelude::*;

use connection::executor::DatabaseExecutor;
use actix::dev::MessageResponse;

use model::actions::Action;
use model::state::State;
use model::actions::ActionResult;
use model::actions::error::Error;
use scripting::Scripting;


pub struct ActionWrapper<A: Action + Send> {
    action: Result<A, serde_json::Error>,
}

impl<A: Action + Send> ActionWrapper<A> {
    pub fn new(action: Result<A, serde_json::Error>) -> Self {
        Self {
            action,
        }
    }
}

impl<A: Action + Send> Message for ActionWrapper<A>
    where
        A::Ret: 'static,
        ActionResult<A::Ret>: 'static,
{
    type Result = ActionResult<A::Ret>;
}

impl<A: Action + Send> Handler<ActionWrapper<A>> for DatabaseExecutor
    where
        A::Ret: 'static,
        ActionResult<A::Ret>: MessageResponse<DatabaseExecutor, ActionWrapper<A>> + 'static,
{
    type Result = ActionResult<A::Ret>;

    fn handle(&mut self, msg: ActionWrapper<A>, _: &mut Self::Context) -> Self::Result {
        let action_req = msg.action.or_else(|err| Err(Error::SerializationError(err)))?;

        let conn = self.get_connection();
        let scripting = Scripting::new(self.get_scripts_path());
        let state = State::new(conn, scripting);
        let result = action_req.call(&state);
        result
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use model::state::GetConnection;
    use connection::AppStateBuilder;
    use connection::AppState;
    use futures::Future;
    use actix_web::HttpResponse;
    use actix_web::AsyncResponder;
    use model::actions::ActionRes;

    struct TestAction;
    impl<S> Action<S> for TestAction
        where S: GetConnection
    {
        type Ret = String;

        fn call(&self, state: &S) -> ActionResult<Self::Ret> {
            ActionRes::new("Hello World!".to_string())
        }
    }

    fn mock_executor() -> AppState {
        AppStateBuilder::new()
            .host("localhost")
            .port(5432)
            .user("test")
            .pass("password")
            .done()
    }

    #[test]
    fn test_handle_action() {
        actix::System::run(|| {
            let executor = mock_executor();
            let action = TestAction;


            let f = executor
                .connect()
                .send(ActionWrapper::new(Ok(action)))
                .map_err(|_| ())
                .map(|res| {
                    assert_eq!(res.unwrap(), "Hello World!");

                    actix::System::current().stop();
                });

            tokio::spawn(f);
        });
    }
}