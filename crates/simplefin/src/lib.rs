//! SimpleFIN Bridge integration: fetch linked accounts and write their
//! transactions into the wealth database.

pub mod client;
pub mod mapping;
pub mod setup;
pub mod sync;

pub use client::{AccountSet, Feed, FeedAccount, FeedTransaction, HttpFeed};
pub use setup::{claim_access_url, is_access_url};
pub use sync::{run_sync, AccountSyncResult, MappedAccount, SyncReport, UnmappedAccount, UNCATEGORIZED};
