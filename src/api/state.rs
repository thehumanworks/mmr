use duckdb::Connection;
use std::sync::{Arc, Mutex};

pub type AppState = Arc<Mutex<Connection>>;
