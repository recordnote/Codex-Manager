use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

#[derive(Debug, Clone)]
pub(crate) struct ActiveTurnControl {
    cancel_requested: Arc<AtomicBool>,
}

impl ActiveTurnControl {
    pub(crate) fn new() -> Self {
        Self {
            cancel_requested: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn request_cancel(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.cancel_requested.load(Ordering::SeqCst)
    }
}

fn active_turn_controls() -> &'static RwLock<BTreeMap<String, ActiveTurnControl>> {
    static CONTROLS: OnceLock<RwLock<BTreeMap<String, ActiveTurnControl>>> = OnceLock::new();
    CONTROLS.get_or_init(|| RwLock::new(BTreeMap::new()))
}

pub(crate) fn register_turn(turn_id: String) -> ActiveTurnControl {
    let control = ActiveTurnControl::new();
    crate::lock_utils::write_recover(active_turn_controls(), "active_turn_controls")
        .insert(turn_id, control.clone());
    control
}

pub(crate) fn turn_control(turn_id: &str) -> Option<ActiveTurnControl> {
    crate::lock_utils::read_recover(active_turn_controls(), "active_turn_controls")
        .get(turn_id)
        .cloned()
}

pub(crate) fn clear_turn(turn_id: &str) {
    crate::lock_utils::write_recover(active_turn_controls(), "active_turn_controls")
        .remove(turn_id);
}

pub(crate) fn clear_for_tests() {
    crate::lock_utils::write_recover(active_turn_controls(), "active_turn_controls").clear();
}
