pub mod abaqus;
pub mod comsol;
pub mod hfss;

use std::path::Path;

use crate::{Result, SimparseError};

pub fn inspect_fluent_hdf5(path: &Path) -> Result<simparse_hdf5::FluentHdf5Summary> {
    simparse_hdf5::inspect_fluent_hdf5(path).map_err(|err| SimparseError::Hdf5(err.to_string()))
}
