mod dispatch;
mod error;
pub mod parsers;
mod schema;
mod summary;

pub use dispatch::{detect_format, inspect_path, scan_paths};
pub use error::{Result, SimparseError};
pub use parsers::abaqus::inspect_abaqus_inp;
pub use parsers::comsol::inspect_comsol_mph;
pub use parsers::hfss::inspect_hfss_aedt;
pub use parsers::inspect_fluent_hdf5;
pub use schema::*;
pub use summary::*;
