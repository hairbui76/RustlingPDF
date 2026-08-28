//! Optional in-process `PaddleOCR` engine ownership.
//!
//! The caller supplies every native runtime, model, dictionary, and font path.
//! This module downloads and caches nothing. One loaded engine is reused behind
//! a mutex because `PaddleOCR-Rust` intentionally makes `OcrEngine` non-`Sync`.

use std::{path::PathBuf, sync::Mutex};

#[cfg(feature = "paddle-ocr")]
use paddleocr_rust::api::{Artifacts, OcrEngine, OcrOptions, parse_dictionary};
#[cfg(feature = "paddle-ocr")]
use sha2::{Digest, Sha256};
use thiserror::Error;

#[cfg(feature = "paddle-ocr")]
const EXPECTED_DETECTOR_SHA256: &str =
    "eb13b44b25bb36f89528b68720af8a61d9cf381176107f465db1757b65d086e1";
#[cfg(feature = "paddle-ocr")]
const EXPECTED_RECOGNIZER_SHA256: &str =
    "9c09abf0957f7968c7586464b7397b84ad2387a0497a351af40e9acc71b673ba";
const EXPECTED_DICTIONARY_SHA256: &str =
    "b5f2bfe2bdd9448429e3e82b51c789775d9b42f2403d082b00662eb77e401c5d";
const EXPECTED_DICTIONARY_ENTRIES: usize = 18_708;
#[cfg(feature = "paddle-ocr")]
const MAX_DICTIONARY_BYTES: u64 = 4 * 1024 * 1024;

/// Complete local configuration for the one Paddle artifact pair RustlingPDF supports.
///
/// Every field names a local file, so the shared `_path` suffix is the point
/// rather than noise: these names mirror the `ocr.paddle.*` configuration keys
/// an operator sets.
#[allow(clippy::struct_field_names)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PaddleOcrConfig {
    pub(crate) onnx_runtime_path: PathBuf,
    pub(crate) detector_model_path: PathBuf,
    pub(crate) recognizer_model_path: PathBuf,
    pub(crate) dictionary_path: PathBuf,
    pub(crate) text_layer_font_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PaddleTextLine {
    pub(crate) text: String,
    /// Source-image points in Paddle's top-left-origin pixel coordinates.
    pub(crate) points: [(f32, f32); 4],
}

#[derive(Debug, Error)]
pub enum PaddleOcrError {
    #[error("invalid Paddle OCR configuration: {0}")]
    Configuration(String),
    #[cfg(not(feature = "paddle-ocr"))]
    #[error("this RustlingPDF build does not include the paddle-ocr feature")]
    NotCompiled,
    #[error("the Paddle OCR engine lock is poisoned because another OCR request panicked")]
    LockPoisoned,
    #[error("could not read the Paddle OCR dictionary '{path}': {source}")]
    ReadDictionary {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the Paddle OCR dictionary is {actual} bytes; the limit is {limit} bytes")]
    DictionaryTooLarge { actual: u64, limit: u64 },
    #[error("the Paddle OCR dictionary is not valid UTF-8: {0}")]
    DictionaryEncoding(#[from] std::str::Utf8Error),
    #[error("the Paddle OCR dictionary SHA-256 is {actual}, expected {EXPECTED_DICTIONARY_SHA256}")]
    DictionaryIdentity { actual: String },
    #[error(
        "the Paddle OCR dictionary has {actual} entries, expected {EXPECTED_DICTIONARY_ENTRIES}"
    )]
    DictionaryEntries { actual: usize },
    #[cfg(feature = "paddle-ocr")]
    #[error("Paddle OCR failed: {0}")]
    Engine(#[from] paddleocr_rust::error::Error),
}

#[derive(Clone, Debug)]
enum PaddleSelection {
    Disabled,
    Invalid(String),
    Configured(PaddleOcrConfig),
}

/// Process-owned, lazily loaded Paddle engine.
#[derive(Debug)]
pub(crate) struct PaddleOcrService {
    selection: PaddleSelection,
    #[cfg(feature = "paddle-ocr")]
    engine: Mutex<Option<OcrEngine>>,
    #[cfg(not(feature = "paddle-ocr"))]
    engine: Mutex<Option<()>>,
}

impl PaddleOcrService {
    pub(crate) fn from_config(config: Result<Option<PaddleOcrConfig>, String>) -> Self {
        let selection = match config {
            Ok(Some(config)) => PaddleSelection::Configured(config),
            Ok(None) => PaddleSelection::Disabled,
            Err(error) => PaddleSelection::Invalid(error),
        };
        Self {
            selection,
            engine: Mutex::new(None),
        }
    }

    pub(crate) fn is_selected(&self) -> bool {
        !matches!(self.selection, PaddleSelection::Disabled)
    }

    /// Returns the complete configuration, or why Paddle cannot run.
    fn configured(&self) -> Result<&PaddleOcrConfig, PaddleOcrError> {
        match &self.selection {
            PaddleSelection::Configured(config) => Ok(config),
            PaddleSelection::Invalid(error) => Err(PaddleOcrError::Configuration(error.clone())),
            PaddleSelection::Disabled => Err(PaddleOcrError::Configuration(
                "ocr.engine is not set to paddle".to_owned(),
            )),
        }
    }

    pub(crate) fn font_path(&self) -> Result<&PathBuf, PaddleOcrError> {
        Ok(&self.configured()?.text_layer_font_path)
    }

    #[cfg(feature = "paddle-ocr")]
    pub(crate) fn recognize_image(
        &self,
        encoded_image: &[u8],
    ) -> Result<Vec<PaddleTextLine>, PaddleOcrError> {
        let config = self.configured()?;
        let mut engine = self
            .engine
            .lock()
            .map_err(|_| PaddleOcrError::LockPoisoned)?;
        if engine.is_none() {
            *engine = Some(load_engine(config)?);
        }
        let lines = engine
            .as_ref()
            .ok_or(PaddleOcrError::LockPoisoned)?
            .recognize_image(encoded_image, &OcrOptions::default())?;
        Ok(lines
            .into_iter()
            .map(|line| PaddleTextLine {
                text: line.text,
                points: line
                    .quadrilateral
                    .points()
                    .map(|point| (point.x(), point.y())),
            })
            .collect())
    }

    #[cfg(not(feature = "paddle-ocr"))]
    pub(crate) fn recognize_image(
        &self,
        _encoded_image: &[u8],
    ) -> Result<Vec<PaddleTextLine>, PaddleOcrError> {
        self.configured()?;
        let _guard = self
            .engine
            .lock()
            .map_err(|_| PaddleOcrError::LockPoisoned)?;
        Err(PaddleOcrError::NotCompiled)
    }
}

#[cfg(feature = "paddle-ocr")]
fn load_engine(config: &PaddleOcrConfig) -> Result<OcrEngine, PaddleOcrError> {
    let dictionary_bytes = read_bounded_dictionary(&config.dictionary_path)?;
    let actual_digest = data_encoding::HEXLOWER.encode(&Sha256::digest(&dictionary_bytes));
    if actual_digest != EXPECTED_DICTIONARY_SHA256 {
        return Err(PaddleOcrError::DictionaryIdentity {
            actual: actual_digest,
        });
    }
    let dictionary = parse_dictionary(std::str::from_utf8(&dictionary_bytes)?, true)?;
    if dictionary.len() != EXPECTED_DICTIONARY_ENTRIES {
        return Err(PaddleOcrError::DictionaryEntries {
            actual: dictionary.len(),
        });
    }

    let library = configured_path(&config.onnx_runtime_path, "onnxRuntimePath")?;
    let detector = configured_path(&config.detector_model_path, "detectorModelPath")?;
    let recognizer = configured_path(&config.recognizer_model_path, "recognizerModelPath")?;
    let artifacts = Artifacts::new(library, detector, recognizer)
        .with_detector_sha256(EXPECTED_DETECTOR_SHA256)
        .with_recognizer_sha256(EXPECTED_RECOGNIZER_SHA256);
    OcrEngine::load(&artifacts, &dictionary).map_err(Into::into)
}

#[cfg(feature = "paddle-ocr")]
fn read_bounded_dictionary(path: &PathBuf) -> Result<Vec<u8>, PaddleOcrError> {
    let metadata = std::fs::metadata(path).map_err(|source| PaddleOcrError::ReadDictionary {
        path: path.clone(),
        source,
    })?;
    if metadata.len() > MAX_DICTIONARY_BYTES {
        return Err(PaddleOcrError::DictionaryTooLarge {
            actual: metadata.len(),
            limit: MAX_DICTIONARY_BYTES,
        });
    }
    let bytes = std::fs::read(path).map_err(|source| PaddleOcrError::ReadDictionary {
        path: path.clone(),
        source,
    })?;
    let actual = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if actual > MAX_DICTIONARY_BYTES {
        return Err(PaddleOcrError::DictionaryTooLarge {
            actual,
            limit: MAX_DICTIONARY_BYTES,
        });
    }
    Ok(bytes)
}

#[cfg(feature = "paddle-ocr")]
fn configured_path<'a>(path: &'a std::path::Path, field: &str) -> Result<&'a str, PaddleOcrError> {
    path.to_str().ok_or_else(|| {
        PaddleOcrError::Configuration(format!("ocr.paddle.{field} must be valid UTF-8"))
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{PaddleOcrConfig, PaddleOcrError, PaddleOcrService};

    fn complete_config() -> PaddleOcrConfig {
        PaddleOcrConfig {
            onnx_runtime_path: PathBuf::from("/opt/onnx/libonnxruntime.so"),
            detector_model_path: PathBuf::from("/models/detector.onnx"),
            recognizer_model_path: PathBuf::from("/models/recognizer.onnx"),
            dictionary_path: PathBuf::from("/models/ppocrv6_dict.txt"),
            text_layer_font_path: PathBuf::from("/fonts/NotoSansCJK-Regular.otf"),
        }
    }

    #[test]
    fn disabled_service_does_not_select_paddle() {
        let service = PaddleOcrService::from_config(Ok(None));
        assert!(!service.is_selected());
        assert!(matches!(
            service.font_path(),
            Err(PaddleOcrError::Configuration(message))
                if message == "ocr.engine is not set to paddle"
        ));
        assert!(matches!(
            service.recognize_image(b"not an image"),
            Err(PaddleOcrError::Configuration(message))
                if message == "ocr.engine is not set to paddle"
        ));
    }

    #[test]
    fn invalid_service_reports_configuration_before_loading() {
        let service = PaddleOcrService::from_config(Err("missing detectorModelPath".to_owned()));
        assert!(service.is_selected());
        assert!(matches!(
            service.font_path(),
            Err(PaddleOcrError::Configuration(message))
                if message == "missing detectorModelPath"
        ));
        assert!(matches!(
            service.recognize_image(b"not an image"),
            Err(PaddleOcrError::Configuration(message))
                if message == "missing detectorModelPath"
        ));
    }

    #[test]
    fn configured_service_exposes_the_operator_supplied_font() {
        let service = PaddleOcrService::from_config(Ok(Some(complete_config())));
        assert!(service.is_selected());
        assert!(matches!(
            service.font_path(),
            Ok(path) if path == Path::new("/fonts/NotoSansCJK-Regular.otf")
        ));
    }

    /// A build without the feature must refuse a complete configuration by
    /// naming the missing feature, never by silently recognising nothing.
    #[cfg(not(feature = "paddle-ocr"))]
    #[test]
    fn feature_off_build_reports_the_missing_feature_for_complete_configuration() {
        let service = PaddleOcrService::from_config(Ok(Some(complete_config())));
        assert!(matches!(
            service.recognize_image(b"not an image"),
            Err(PaddleOcrError::NotCompiled)
        ));
    }

    /// With the feature on, a complete configuration reaches artifact loading
    /// and fails on the absent operator files rather than on configuration.
    #[cfg(feature = "paddle-ocr")]
    #[test]
    fn feature_on_build_reaches_artifact_loading_for_complete_configuration() {
        let service = PaddleOcrService::from_config(Ok(Some(complete_config())));
        assert!(matches!(
            service.recognize_image(b"not an image"),
            Err(PaddleOcrError::ReadDictionary { path, .. })
                if path == Path::new("/models/ppocrv6_dict.txt")
        ));
    }
}
