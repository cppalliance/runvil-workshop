//! The `/prompts/*` route handlers: parsing prompt text into the Run
//! window's contract DTO.
//!
//! The parse route is pure - no state, no disk, no clock: it turns the
//! posted markdown into the contract the Run window renders, or a `422`
//! envelope naming the parse failure kind and line. The parser's own
//! types are `Deserialize`-only, so the wire DTO is hand-written
//! `Serialize` structs built from [`Frontmatter`] accessors. Its `args`
//! field is the JSON Schema that
//! [`ArgsSchema::as_json`](runvil::prompt::ArgsSchema::as_json) gives.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::{Deserialize, Serialize};

use runvil::Prompt;
use runvil::model::ModelRole;
use runvil::packs::PackRef;
use runvil::prompt::{Frontmatter, RoleRequirements};

use crate::error::AppError;

/// The `/prompts/contract` route. The parse is pure, so the router
/// carries no state; the server mounts it under the default deadline and
/// its cross-site guard.
pub(crate) fn routes() -> axum::Router {
    axum::Router::new().route("/prompts/contract", post(contract))
}

/// The JSON body of `POST /prompts/contract`.
#[derive(Debug, Deserialize)]
struct ContractRequest {
    /// The prompt's display name (the file's, when it came from one).
    name: String,
    /// The prompt markdown to parse.
    text: String,
}

/// The Run-window contract: everything the panel renders, built from
/// the parsed frontmatter.
#[derive(Debug, Serialize)]
struct ContractResponse {
    /// The prompt's identifier.
    name: String,
    /// The one-line description shown in listings.
    description: String,
    /// The Engine major the `runvil:` key declares; `null` when absent.
    runvil: Option<u32>,
    /// The declared tool-loop cap; `null` for the runtime default.
    max_tool_iterations: Option<u32>,
    /// The declared Packs, in declaration order.
    uses: Vec<PackDto>,
    /// The `args` declaration as JSON Schema. The root is an object node,
    /// and its `properties` are the prompt's arguments by name.
    args: serde_json::Value,
    /// The declared model roles, sorted by label.
    models: Vec<ModelDto>,
}

/// A declared Pack.
#[derive(Debug, Serialize)]
struct PackDto {
    /// The Pack id, such as `web`, which is `poop` for `acme/poop`.
    id: String,
}

impl From<&PackRef> for PackDto {
    fn from(entry: &PackRef) -> Self {
        Self {
            id: entry.id().to_string(),
        }
    }
}

/// The requirements a prompt declares for one model role.
#[derive(Debug, Serialize)]
struct ModelDto {
    /// The role name.
    label: String,
    /// The minimum context window in tokens; `null` when absent.
    min_context: Option<u32>,
    /// The thinking requirement; `null` when absent.
    thinking: Option<bool>,
}

impl ModelDto {
    /// Builds the DTO for the requirements declared for `role`.
    fn new(role: ModelRole, declared: RoleRequirements) -> Self {
        Self {
            label: role.as_str().to_owned(),
            min_context: declared.min_context().map(std::num::NonZeroU32::get),
            thinking: declared.thinking(),
        }
    }
}

impl From<&Frontmatter> for ContractResponse {
    fn from(frontmatter: &Frontmatter) -> Self {
        Self {
            name: frontmatter.name().to_owned(),
            description: frontmatter.description().to_owned(),
            runvil: frontmatter.runvil(),
            max_tool_iterations: frontmatter
                .max_tool_iterations()
                .map(std::num::NonZeroU32::get),
            uses: frontmatter.uses().iter().map(PackDto::from).collect(),
            args: frontmatter.args().as_json().clone(),
            models: ModelRole::ALL
                .into_iter()
                .filter_map(|role| {
                    frontmatter
                        .role_requirements(role)
                        .map(|declared| ModelDto::new(role, *declared))
                })
                .collect(),
        }
    }
}

/// Parses the posted prompt text and answers the contract DTO, or a
/// `422` envelope when the text is not a valid prompt.
async fn contract(Json(body): Json<ContractRequest>) -> Response {
    match Prompt::parse(&|_: &str| Ok(body.text.clone()), &body.name) {
        Ok(prompt) => (
            StatusCode::OK,
            Json(ContractResponse::from(prompt.frontmatter())),
        )
            .into_response(),
        Err(error) => AppError::prompt_parse(&error).into_response(),
    }
}

#[cfg(test)]
#[path = "prompts-tests.rs"]
mod tests;
