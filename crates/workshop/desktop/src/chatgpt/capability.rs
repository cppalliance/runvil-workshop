//! Per-window capability gate guarding Tauri account commands.
//!
//! Embedded Gateway config surfaces run on their own dedicated loopback origin
//! excluded from Workshop's main-origin Tauri remote ACL. As defense in depth
//! on top of that origin boundary, [`ChatgptState`] enforces a single-bind
//! capability minted by the main frame at boot and cleared only when the main
//! frame undergoes a verified main-origin top-level navigation. Rejected calls
//! return factual errors naming the unmet state without echoing any stored or
//! presented token.

/// One boot's capability. The first non-empty token wins.
#[derive(Debug, Default)]
pub(crate) struct CapabilityGate {
    bound: Option<String>,
}

impl CapabilityGate {
    /// Binds `token` if none is bound yet.
    ///
    /// # Errors
    /// Returns an error when `token` is empty or a token is already bound.
    pub(crate) fn bind(&mut self, token: &str) -> Result<(), &'static str> {
        if token.is_empty() {
            return Err(
                "ChatGPT capability bind requires a non-empty token; actual token is empty",
            );
        }
        if self.bound.is_some() {
            return Err(
                "ChatGPT capability bind requires an unbound gate; actual gate already holds a capability",
            );
        }
        self.bound = Some(token.to_owned());
        Ok(())
    }

    /// Clears the bound token when the main top-level document begins a
    /// navigation/reload on the main origin so the reloaded document can
    /// bind a fresh capability once.
    pub(crate) fn reset_for_main_load(&mut self) {
        self.bound = None;
    }

    /// Checks `token` against the bound capability without disclosing either value.
    ///
    /// # Errors
    /// Returns an error naming whether no capability is bound or the presented token mismatched.
    pub(crate) fn verify(&self, token: &str) -> Result<(), &'static str> {
        match self.bound.as_deref() {
            None => Err("ChatGPT command requires a bound window capability; none is bound"),
            Some(bound) if bound == token => Ok(()),
            Some(_) => Err(
                "ChatGPT command requires the window-bound capability; presented capability did not match",
            ),
        }
    }

    /// Whether `token` is the bound capability.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn admit(&self, token: &str) -> bool {
        self.verify(token).is_ok()
    }
}

/// The bound capability for the process.
#[derive(Default)]
pub(crate) struct ChatgptState {
    gate: std::sync::Mutex<CapabilityGate>,
}

impl ChatgptState {
    /// Clears the bound capability when the main window begins loading
    /// the trusted main origin (top-level load / reload).
    pub(crate) fn reset_for_main_load(&self) {
        self.gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reset_for_main_load();
    }

    /// Binds `capability` for the current main-document load.
    ///
    /// # Errors
    /// Returns an error if `capability` is empty or a token is already bound.
    pub(super) fn bind(&self, capability: &str) -> Result<(), String> {
        self.gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .bind(capability)
            .map_err(str::to_owned)
    }

    /// Verifies that `capability` matches the currently bound window token.
    ///
    /// # Errors
    /// Returns an error if no capability is bound or `capability` does not match.
    pub(super) fn check(&self, capability: &str) -> Result<(), String> {
        self.gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .verify(capability)
            .map_err(str::to_owned)
    }
}

/// Checks `capability` against `state` before dispatching an account command.
///
/// # Errors
/// Propagates the capability verification error when unbound or mismatched.
pub(super) fn admit(state: &ChatgptState, capability: &str) -> Result<(), String> {
    state.check(capability)
}
