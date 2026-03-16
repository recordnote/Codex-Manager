use super::types::{ThreadSessionConfig, ThreadTokenUsageWire, ThreadWire, TurnWire};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{OnceLock, RwLock};

#[derive(Debug, Clone)]
pub(crate) struct StoredTurn {
    pub(crate) wire: TurnWire,
}

#[derive(Debug, Clone)]
pub(crate) struct StoredThread {
    pub(crate) wire: ThreadWire,
    pub(crate) session: ThreadSessionConfig,
    pub(crate) token_usage: ThreadTokenUsageWire,
    pub(crate) subscribers: BTreeSet<String>,
    pub(crate) turns: BTreeMap<String, StoredTurn>,
    pub(crate) turn_order: Vec<String>,
    pub(crate) active_turn_id: Option<String>,
}

fn thread_store() -> &'static RwLock<BTreeMap<String, StoredThread>> {
    static STORE: OnceLock<RwLock<BTreeMap<String, StoredThread>>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(BTreeMap::new()))
}

pub(crate) fn read_store() -> &'static RwLock<BTreeMap<String, StoredThread>> {
    thread_store()
}

pub(crate) fn clear_for_tests() {
    crate::lock_utils::write_recover(thread_store(), "thread_turn_store").clear();
}
