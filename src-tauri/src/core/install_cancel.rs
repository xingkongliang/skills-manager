use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Registry of per-operation cancel tokens.
/// Each install operation gets a unique key; setting the flag cancels it.
pub struct InstallCancelRegistry {
    tokens: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl InstallCancelRegistry {
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Register a new operation and return its cancel flag.
    pub fn register(&self, key: &str) -> Arc<AtomicBool> {
        let token = Arc::new(AtomicBool::new(false));
        self.tokens
            .lock()
            .unwrap()
            .insert(key.to_string(), token.clone());
        token
    }

    /// Signal cancellation for the given operation.
    pub fn cancel(&self, key: &str) -> bool {
        if let Some(token) = self.tokens.lock().unwrap().get(key) {
            token.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    /// Remove a completed/cancelled operation from the registry.
    pub fn remove(&self, key: &str) {
        self.tokens.lock().unwrap().remove(key);
    }
}
