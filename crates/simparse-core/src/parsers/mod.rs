pub mod abaqus;
pub mod comsol;
pub mod flotherm;
pub mod hfss;
pub mod icepak;
pub mod step;

use std::path::Path;

use crate::{Result, SimparseError};

pub fn inspect_fluent_hdf5(path: &Path) -> Result<simparse_hdf5::FluentHdf5Summary> {
    simparse_hdf5::inspect_fluent_hdf5(path).map_err(|err| SimparseError::Hdf5(err.to_string()))
}

pub fn inspect_mechanical_mechdb(
    path: &Path,
    max_text_bytes: usize,
) -> Result<simparse_hdf5::MechanicalMechdbSummary> {
    simparse_hdf5::inspect_mechanical_mechdb(path, max_text_bytes)
        .map_err(|err| SimparseError::Hdf5(err.to_string()))
}
