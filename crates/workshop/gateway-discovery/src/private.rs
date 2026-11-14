//! The `gateway-private.json` file beside the discovery file.
//!
//! The Gateway mints it at each start. Workshop reads it only to pair a
//! plan proof with the boot the discovery file names. The file is not an
//! attach decision: a missing, unreadable, invalid, or unpaired file
//! leaves attachment unchanged and supplies no proof. This crate has no
//! production writer. The Gateway owns the file.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::SidecarError;
use crate::file::GatewayDiscoveryFile;
use crate::paths::gateway_private_file_path;

/// The on-disk proofs of one gateway boot.
///
/// Readers must tolerate unknown fields, the same way they tolerate them
/// on the discovery file.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayPrivateFile {
    /// The gateway process id, equal to the discovery file's.
    pub pid: u32,
    /// The gateway's boot instant in unix seconds, equal to the discovery
    /// file's.
    pub epoch: u64,
    /// Presented on a plan-model call. Not a bearer.
    pub plan_proof: String,
    /// Presented on a custody-affecting admin call. Not a bearer, and not
    /// sent by this crate.
    pub owner_proof: String,
}

impl fmt::Debug for GatewayPrivateFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayPrivateFile")
            .field("pid", &self.pid)
            .field("epoch", &self.epoch)
            .field("plan_proof", &"[REDACTED]")
            .field("owner_proof", &"[REDACTED]")
            .finish()
    }
}

impl GatewayPrivateFile {
    /// Whether these proofs belong to the boot `discovery` names.
    #[must_use]
    pub fn names_boot_of(&self, discovery: &GatewayDiscoveryFile) -> bool {
        self.pid == discovery.pid && self.epoch == discovery.epoch
    }

    /// The reason the file fails validation, or `None` when it is valid.
    #[must_use]
    pub fn validation_error(&self) -> Option<&'static str> {
        if self.pid == 0 {
            return Some("pid must not be 0");
        }
        for proof in [&self.plan_proof, &self.owner_proof] {
            if proof.is_empty() {
                return Some("a proof must not be empty");
            }
            if !proof.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Some("a proof must be printable ASCII with no spaces");
            }
        }
        if self.plan_proof == self.owner_proof {
            return Some("the two proofs must differ");
        }
        None
    }

    /// Reads and validates the private file in `run_dir`, returning `None`
    /// when no file exists.
    ///
    /// # Errors
    /// Returns [`SidecarError::Read`] when the file exists but cannot be
    /// read, [`SidecarError::Parse`] when it is not valid JSON, and
    /// [`SidecarError::Invalid`] when it fails validation.
    pub fn read(run_dir: &Path) -> Result<Option<GatewayPrivateFile>, SidecarError> {
        let path = gateway_private_file_path(run_dir);
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(SidecarError::Read {
                    path,
                    source: error,
                });
            }
        };
        let file: GatewayPrivateFile =
            serde_json::from_str(&raw).map_err(|source| SidecarError::Parse {
                path: path.clone(),
                source,
            })?;
        if let Some(reason) = file.validation_error() {
            return Err(SidecarError::Invalid {
                path,
                reason: reason.to_owned(),
            });
        }
        Ok(Some(file))
    }
}

/// The plan proof that belongs to the boot `pid` and `epoch` name.
///
/// A missing, unreadable, or invalid file yields `None`. A file whose pid
/// or epoch disagrees is read again, together with the discovery file, and
/// the proof is returned only when that second pair agrees with each other
/// and with `pid` and `epoch`. A proof from another boot is never returned.
#[must_use]
pub fn plan_proof_for_boot(run_dir: &Path, pid: u32, epoch: u64) -> Option<String> {
    if let Some(proof) = proof_matching(run_dir, pid, epoch) {
        return Some(proof);
    }
    let discovery = GatewayDiscoveryFile::read(run_dir).ok().flatten()?;
    let private = GatewayPrivateFile::read(run_dir).ok().flatten()?;
    paired_proof(&private, &discovery, pid, epoch)
}

/// The private file's plan proof when that file itself names `pid` and
/// `epoch`. An unreadable file is `None`, the same as a missing one.
fn proof_matching(run_dir: &Path, pid: u32, epoch: u64) -> Option<String> {
    let private = GatewayPrivateFile::read(run_dir).ok().flatten()?;
    (private.pid == pid && private.epoch == epoch).then_some(private.plan_proof)
}

/// The plan proof when `private` and `discovery` name one boot and that
/// boot is `pid` and `epoch`.
fn paired_proof(
    private: &GatewayPrivateFile,
    discovery: &GatewayDiscoveryFile,
    pid: u32,
    epoch: u64,
) -> Option<String> {
    if private.names_boot_of(discovery) && discovery.pid == pid && discovery.epoch == epoch {
        Some(private.plan_proof.clone())
    } else {
        None
    }
}

#[cfg(test)]
#[path = "private-tests.rs"]
mod tests;
