
use actix::prelude::*;

use actix_web::{
    http,
    FromRequest, Json, Query,
    HttpRequest,
};

use actix_web::middleware::cors::CorsBuilder;
use actix_web::dev::JsonConfig;

use super::action_wrapper::ActionWrapper;

use super::procedure::ProcedureBuilder;
use super::procedure::ProcedureHandler;
use super::procedure::procedure_handler_function;
use super::procedure::procedure_bad_request_handler_function;

use model::actions::Action;
use std::fmt::Debug;
use serde::Serialize;
use connection::executor::Executor;
use actix_web::test::TestApp;
use connection::AppStateLike;

// use actix_web::dev::QueryConfig; //NOTE: for some reason this can't be imported, probably actix_web issue


/// extend actix cors routes to handle RPC
pub trait ProcedureExt<S>
    where
        S: AppStateLike + 'static,
{

    /// Create an RPC call
    ///
    /// # Arguments
    /// * `path` - A string representing the url path
    /// * `procedure_builder` - An object extending `ProcedureBuilder` for building a message
    ///
    fn procedure<JP, QP, A, PB>(&mut self, path: &str, procedure_builder: PB) -> &mut Self
        where
            Executor: Handler<ActionWrapper<A>>,
            JP: Debug + 'static,
            QP: Debug + 'static,
            A: Action + Send + 'static,
            PB: ProcedureBuilder<S, JP, QP, A> + Clone + 'static,
            Json<JP>: FromRequest<S, Config = JsonConfig<S>>,
            Query<QP>: FromRequest<S>,
            <A as Action>::Ret: Send + Serialize;

}

macro_rules! implement_router {

    ($App:ident) => {
        impl<S> ProcedureExt<S> for $App<S>
            where
                S: AppStateLike + 'static,
        {
            fn procedure<JP, QP, A, PB>(&mut self, path: &str, procedure_builder: PB) -> &mut Self
                where
                    Executor: Handler<ActionWrapper<A>>,
                    A: Action + Send + 'static,
                    PB: ProcedureBuilder<S, JP, QP, A> + Clone + 'static,
                    JP: Debug + 'static,
                    QP: Debug + 'static,
                    Json<JP>: FromRequest<S, Config = JsonConfig<S>>,
                    Query<QP>: FromRequest<S>,
                    <A as Action>::Ret: Send + Serialize,
            {
                self.resource(path, move |r| {
                    r.method(http::Method::POST).with_config(
                        move |(req, json_params, query_params): (HttpRequest<S>, Json<JP>, Query<QP>)| {
                            let proc = ProcedureHandler::<S, JP, QP, PB, A>::setup(&procedure_builder);
                            procedure_handler_function(proc, req, json_params, query_params)
                        },
                        |((_, json_cfg, _query_cfg),)| {
                            json_cfg
                                .error_handler(|err, _req| {
                                    procedure_bad_request_handler_function(err)
                                });
                        }
                    );
                })
            }
        }
    }
}

implement_router!(CorsBuilder);
implement_router!(TestApp);