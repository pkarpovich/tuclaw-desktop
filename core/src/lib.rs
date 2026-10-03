mod fixtures;
/// Grouping of messages into day sections.
pub mod grouping;
/// Domain types and the message body encoding.
pub mod model;
/// Filesystem locations shared by the app and its tests.
pub mod paths;
mod schema;
/// The SQLite store behind the workspace.
pub mod store;
/// The client of the daemon's `/api/v3`.
pub mod v3;
