use std::{error::Error, io};

use pyo3::{PyErr, create_exception, exceptions::PyException};
use thiserror::Error;

#[derive(Error, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum PyPixiBuildBackendError {
    #[error(transparent)]
    Cli(Box<dyn Error>),

    // #[error("CLI error: {0}")]
    // Cli(String),
    #[error(transparent)]
    GeneratedRecipe(Box<dyn Error>),
    // #[error("Failed to serialize/deserialize Python object")]
    // PythonSerialization,

    // #[error("Invalid URL format: {0}")]
    // InvalidUrl(String),

    // #[error("Failed to append item to Python list")]
    // PythonListAppend,

    // #[error("Failed to convert Python type")]
    // PythonTypeConversion,

    // #[error("Failed to call Python method")]
    // PythonMethodCall,

    // #[error("Failed to import Python module")]
    // PythonModuleImport,

    // #[error("Failed to extract Python object")]
    // PythonObjectExtract,

    // #[error("Failed to get Python attribute")]
    // PythonAttributeAccess,
}

fn pretty_print_error(mut err: &dyn Error) -> String {
    let mut result = err.to_string();
    while let Some(source) = err.source() {
        result.push_str(&format!("\nCaused by: {source}"));
        err = source;
    }
    result
}

impl From<PyPixiBuildBackendError> for PyErr {
    fn from(value: PyPixiBuildBackendError) -> Self {
        match value {
            PyPixiBuildBackendError::Cli(err) => CliException::new_err(pretty_print_error(&*err)),
            PyPixiBuildBackendError::GeneratedRecipe(err) => {
                GeneratedRecipeException::new_err(pretty_print_error(&*err))
            }
        }
    }
}

create_exception!(exceptions, CliException, PyException);
create_exception!(exceptions, GeneratedRecipeException, PyException);

// create_exception!(exceptions, PythonSerializationException, PyException);
// create_exception!(exceptions, InvalidUrlException, PyException);
// create_exception!(exceptions, PythonListAppendException, PyException);
// create_exception!(exceptions, PythonTypeConversionException, PyException);
// create_exception!(exceptions, PythonMethodCallException, PyException);
// create_exception!(exceptions, PythonModuleImportException, PyException);
// create_exception!(exceptions, PythonObjectExtractException, PyException);
// create_exception!(exceptions, PythonAttributeAccessException, PyException);
// create_exception!(exceptions, InvalidMatchSpecException, PyException);
// create_exception!(exceptions, InvalidPackageNameException, PyException);
// create_exception!(exceptions, InvalidUrlException, PyException);
// create_exception!(exceptions, InvalidChannelException, PyException);
// create_exception!(exceptions, ActivationException, PyException);
// create_exception!(exceptions, ParsePlatformException, PyException);
// create_exception!(exceptions, ParseArchException, PyException);
// create_exception!(exceptions, FetchRepoDataException, PyException);
// create_exception!(exceptions, CacheDirException, PyException);
// create_exception!(exceptions, DetectVirtualPackageException, PyException);
// create_exception!(exceptions, IoException, PyException);
// create_exception!(exceptions, SolverException, PyException);
// create_exception!(exceptions, TransactionException, PyException);
// create_exception!(exceptions, LinkException, PyException);
// create_exception!(exceptions, ConvertSubdirException, PyException);
// create_exception!(exceptions, VersionBumpException, PyException);
// create_exception!(exceptions, VersionExtendException, PyException);
// create_exception!(exceptions, ParseCondaLockException, PyException);
// create_exception!(exceptions, ConversionException, PyException);
// create_exception!(exceptions, RequirementException, PyException);
// create_exception!(exceptions, EnvironmentCreationException, PyException);
// create_exception!(exceptions, ExtractException, PyException);
// create_exception!(exceptions, ActivationScriptFormatException, PyException);
// create_exception!(exceptions, GatewayException, PyException);
// create_exception!(exceptions, InstallerException, PyException);
// create_exception!(
//     exceptions,
//     ParseExplicitEnvironmentSpecException,
//     PyException
// );
// create_exception!(exceptions, ValidatePackageRecordsException, PyException);
// create_exception!(exceptions, AuthenticationStorageException, PyException);
// create_exception!(exceptions, ShellException, PyException);
// create_exception!(exceptions, InvalidHeaderNameException, PyException);
// create_exception!(exceptions, InvalidHeaderValueError, PyException);
