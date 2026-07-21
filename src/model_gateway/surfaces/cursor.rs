use std::path::Path;

use anyhow::Result;

use super::{FieldState, OwnedConfigField};
use crate::model_gateway::registry::ModelRoute;

fn unavailable() -> anyhow::Error {
    anyhow::anyhow!(
        "Cursor model-path setup is unavailable: M0 found no documented programmable model-routing boundary; use standard hook/MCP coverage while live capture remains held"
    )
}

pub(super) fn prepare(_route: &ModelRoute, _home: &Path) -> Result<OwnedConfigField> {
    Err(unavailable())
}

pub(super) fn apply(_route: &ModelRoute, _home: &Path, _field: &OwnedConfigField) -> Result<()> {
    Err(unavailable())
}

pub(super) fn restore(_route: &ModelRoute, _home: &Path, _field: &OwnedConfigField) -> Result<()> {
    Err(unavailable())
}

pub(super) fn inspect(_route: &ModelRoute, _home: &Path, _field: &OwnedConfigField) -> FieldState {
    FieldState::Unsupported
}
