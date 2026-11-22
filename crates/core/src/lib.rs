//! Pure domain logic of the Mooring portal.
//!
//! Nothing here touches the network, the file system or a database: the
//! only things read from the operating system are random bytes (tokens,
//! salts, account codes) and, through [`time::SystemClock`], the time. The
//! store persists these types and the web crate presents them.

pub mod account;
pub mod auth;
pub mod feedback;
pub mod json;
pub mod payment;
pub mod profile;
pub mod rate_limit;
pub mod release;
pub mod rental;
pub mod snapshot;
pub mod time;
pub mod translation;
