use std::env;
use std::path::PathBuf;

use super::model::{CargoHomeError, CargoHomeInput, CargoHomeSource};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CargoHomeResolveRequest {
    pub explicit_path: Option<PathBuf>,
}

pub fn resolve_cargo_home(
    request: CargoHomeResolveRequest,
) -> Result<CargoHomeInput, CargoHomeError> {
    if let Some(root) = request.explicit_path {
        return Ok(CargoHomeInput {
            root,
            source: CargoHomeSource::Explicit,
        });
    }
    if let Some(root) = env::var_os("CARGO_HOME").map(PathBuf::from) {
        return Ok(CargoHomeInput {
            root,
            source: CargoHomeSource::CargoHomeEnv,
        });
    }
    let Some(home) = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    else {
        return Err(CargoHomeError::NoCargoHome);
    };
    Ok(CargoHomeInput {
        root: home.join(".cargo"),
        source: CargoHomeSource::HomeDefault,
    })
}
