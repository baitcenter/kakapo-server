
mod input;
mod routes;

use std::marker::PhantomData;
use std::collections::HashSet;
use std::time::Duration;
use std::time::Instant;
use std::iter;

use uuid::Uuid;

use futures::Future;

use actix_web::ws;
use actix_web::HttpResponse;

use actix::ActorContext;
use actix::StreamHandler;
use actix::Actor;
use actix::fut;
use actix::WrapFuture;
use actix::ActorFuture;
use actix::ContextFutureSpawner;
use actix::AsyncContext;
use actix::Handler;
use actix::SystemService;

use chrono;

use AppStateLike;
use view::action_wrapper::ActionWrapper;
use view::procedure::ProcedureBuilder;
use view::error::Error::TooManyConnections;
use view::bearer_token::to_bearer_token;

use model::actions::Action;

use data::claims::AuthClaims;
use data::channels::Channels;

use broker::input::WsInputData;
use broker::routes::CallAction;
use broker::routes::CallParams;
use actix::System;


const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60); // 1 minute
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(600); // 10 minutes
const HEARTBEAT_MESSAGE: &'static str = "Hello";

const MESSAGE_INTERVAL: Duration = Duration::from_millis(500); // 500 milliseconds
// How much time it should lag from now, This is so that if there is a time mismatch between the db and the server, it doesn't skip messages
const MESSAGE_LAG: Duration = Duration::from_micros(50);


impl<S> Actor for WsClientSession<S>
    where
        S: AppStateLike + 'static,
{
    type Context = ws::WebsocketContext<Self, S>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("WsSession [{}] opened ", &self.id.to_hyphenated_ref());
        self.start_heartbeat_process(ctx);
        self.start_message_process(ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("WsSession[{}] closed ", &self.id.to_hyphenated_ref());
    }
}

impl<S> WsClientSession<S>
    where
        S: AppStateLike + 'static
{
    fn start_heartbeat_process(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_later(HEARTBEAT_INTERVAL, Self::heartbeat_process);
    }

    fn heartbeat_process(&mut self, ctx: &mut ws::WebsocketContext<Self, S>) {
        if Instant::now().duration_since(self.last_beat) > HEARTBEAT_TIMEOUT {
            info!("WsSession [{}] timed out",  &self.id.to_hyphenated_ref());
            ctx.stop();
        } else {
            ctx.ping(HEARTBEAT_MESSAGE);
        }

        ctx.run_later(HEARTBEAT_INTERVAL, Self::heartbeat_process);
    }

    fn start_message_process(&mut self, ctx: &mut <Self as Actor>::Context) {

        ctx.run_later(MESSAGE_INTERVAL, Self::message_process);
    }

    fn process_message_when_callback_is_ok(ctx: &mut ws::WebsocketContext<Self, S>, res: serde_json::Value) {
        let messages = res
            .as_array() //Assumes that the getMessages returns an array
            .unwrap_or(&vec![])
            .into_iter()
            .for_each(|message_res| {
                //TODO: need the action name
                let message = serde_json::to_string(&message_res).unwrap_or_default();
                ctx.text(message);
            });
    }

    fn message_process(&mut self, ctx: &mut ws::WebsocketContext<Self, S>) {
        let lag = chrono::Duration::from_std(MESSAGE_LAG)
            .unwrap_or_else(|err| {
                warn!("Could not understand MESSAGE_LAG, setting to 0: err: {:?}", &err);
                chrono::Duration::milliseconds(0)
            });

        let now = chrono::Utc::now().naive_utc() - lag;
        let last = self.last_message;
        self.last_message = now;

        let data = json!({});
        let params = json!({
            "start": last,
            "end": now,
        });

        {
            let mut call_params = CallParams {
                data, params, ctx,
                on_received: &Self::process_message_when_callback_is_ok
            };

            routes::call_procedure("getMessages", self, &mut call_params);
        }

        ctx.run_later(MESSAGE_INTERVAL, Self::message_process);
    }
}


impl<S> StreamHandler<ws::Message, ws::ProtocolError> for WsClientSession<S>
    where
        S: AppStateLike + 'static,
{
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {

        //updating the heartbeat
        self.last_beat = Instant::now();

        match msg {
            ws::Message::Text(text) => {
                let _ = serde_json::from_str(&text)
                    .or_else(|err| {
                        warn!("could not understand incoming message, must be `WsInputData`");
                        let message = json!({
                            "error": "Could not understand message"
                        });
                        let message = serde_json::to_string(&message).unwrap_or_default();
                        ctx.text(message);
                        Err(())
                    })
                    .and_then(move |res: WsInputData| {
                        debug!("handling message");
                        self.handle_message(ctx, res);
                        Ok(())
                    });
            },
            ws::Message::Close(_) => {
                info!("Closing connection");
                ctx.stop();
            },
            ws::Message::Binary(_) => {
                warn!("binary websocket messages not currently supported");
                let message = json!({
                    "error": "Binary format not supported"
                });
                let message = serde_json::to_string(&message).unwrap_or_default();
                ctx.text(message);
            },
            ws::Message::Ping(x) => {
                ctx.pong(&x);
            },
            ws::Message::Pong(message) => {
                if message != HEARTBEAT_MESSAGE {
                    warn!("message out of sync, closing connection");
                    ctx.stop();
                }
            },
        }
    }
}


#[derive(Clone, Debug)]
pub struct WsClientSession<S>
    where
        S: AppStateLike + 'static,
{
    pub id: Uuid,
    subscriptions: HashSet<Channels>,

    last_beat: Instant,
    last_message: chrono::NaiveDateTime,
    auth_header: Option<Vec<u8>>,

    phantom_data: PhantomData<(S)>,
}

impl<S> WsClientSession<S>
    where
        S: AppStateLike + 'static,
{
    pub fn new() -> Self {
        let id = Uuid::new_v4();
        Self {
            id,
            subscriptions: HashSet::new(),
            last_beat: Instant::now(),
            last_message: chrono::Utc::now().naive_utc(),
            auth_header: None,
            phantom_data: PhantomData,
        }
    }

    fn callback_when_action_is_ok(ctx: &mut ws::WebsocketContext<Self, S>, res: serde_json::Value) {
        //TODO: need the action name
        let message = serde_json::to_string(&res).unwrap_or_default();
        ctx.text(message);
    }

    fn handle_message(&mut self, ctx: &mut ws::WebsocketContext<Self, S>, input: WsInputData) {
        match input {
            WsInputData::Authenticate { token } => {
                info!("Authenticating ws user");
                self.authenticating_user(token, ctx);
            },
            WsInputData::Call { procedure, params, data } => {
                debug!("calling procedure: {:?}", &procedure);
                let mut call_params = CallParams {
                    data, params, ctx,
                    on_received: &Self::callback_when_action_is_ok,
                };

                let result = routes::call_procedure(&procedure, self, &mut call_params);
                debug!("finished calling procedure {:?}", &result);
            },
        };
    }
}

impl<S> CallAction<S> for WsClientSession<S>
    where S: AppStateLike
{
    /// For use by the websockets
    fn call<'a, PB, A, F>(&mut self, procedure_builder: PB, call_params: &mut CallParams<'a, S, F>)
        where
            PB: ProcedureBuilder<S, serde_json::Value, serde_json::Value, A> + Clone + 'static,
            S: AppStateLike + 'static,
            A: Action + 'static,
            for<'b> F: Fn(&'b mut ws::WebsocketContext<WsClientSession<S>, S>, serde_json::Value) -> () + 'static,
    {

        let action = procedure_builder
            .build(call_params.data.to_owned(), call_params.params.to_owned());

        let mut action_wrapper = ActionWrapper::new(action);

        if let Some(ref auth) = self.auth_header {
            action_wrapper = action_wrapper.with_auth(&auth);
        }

        let on_received = call_params.on_received;

        call_params
            .ctx
            .state()
            .connect()
            .send(action_wrapper)
            .into_actor(self)
            .then(move |res, actor, ctx| {
                match res {
                    Ok(ok_res) => match ok_res {
                        Ok(res) => {
                            info!("action message ok");
                            let res_value = serde_json::to_value(&res.get_data()).unwrap_or_default();
                            (&on_received)(ctx, res_value);
                        },
                        Err(err) => {
                            info!("action message error");
                            let message = serde_json::to_string(&json!({"error": err.to_string()})).unwrap_or_default();
                            ctx.text(message)
                        }
                    },
                    Err(err) => {
                        error!("websocket error occurred with error message: {:?}", &err);
                        let message = serde_json::to_string(&json!({"error": err.to_string()})).unwrap_or_default();
                        ctx.text(message)
                    }
                }

                fut::ok(())
            })
            .wait(&mut call_params.ctx); //TODO: is spawn better here?
    }

    fn error<'a, F>(&mut self, call_params: &'a mut CallParams<'a, S, F>)
        where
            S: AppStateLike + 'static,
            for<'b> F: Fn(&'b mut ws::WebsocketContext<WsClientSession<S>, S>, serde_json::Value) -> () + 'static,
    {
        let message = serde_json::to_string(&json!({"error": "Did not understand procedure"})).unwrap_or_default();
        call_params.ctx.text(message)
    }
}


impl<S> WsClientSession<S>
    where S: AppStateLike
{

    fn authenticating_user(&mut self, token: String, ctx: &mut ws::WebsocketContext<Self, S>) {
        let token_secret = ctx.state().get_token_secret();
        let decoded = jsonwebtoken::decode::<AuthClaims>(
            &token,
            token_secret.as_ref(),
            &jsonwebtoken::Validation::default());

        match decoded {
            Ok(x) => {
                let bearer_token = to_bearer_token(token); //need it to be a bearer token for the action wrapper to handle it
                self.auth_header = Some(bearer_token.as_bytes().to_vec());

                let message = json!("authenticated");
                let message = serde_json::to_string(&message).unwrap_or_default();
                ctx.text(message);
            },
            Err(err) => {
                error!("encountered error trying to decode token: {:?}", &err);
                let message = json!({
                    "error": "Could not authenticate token"
                });
                let message = serde_json::to_string(&message).unwrap_or_default();
                ctx.text(message);
            }
        }
    }
}