

use actix::prelude::*;

use connection::executor::DatabaseExecutor;
use connection::py::PyRunner;

#[derive(Clone)]
pub struct AppState {
    db_connections: Addr<DatabaseExecutor>,
    py_runner: PyRunner,
    pub app_name: String,
}

impl AppState {
    pub fn new(connections: Addr<DatabaseExecutor>, script_path: &str, app_name: &str) -> Self {
        AppState {
            db_connections: connections,
            py_runner: PyRunner::new(script_path.to_string()),
            app_name: app_name.to_string(),
        }
    }

    pub fn connect<'a>(&'a self) -> &'a Addr<DatabaseExecutor> {
        &self.db_connections
    }

    pub fn get_py_runner(&self) -> PyRunner {
        self.py_runner.to_owned()
    }
}
