use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use lopdf::{
    Dictionary, Document, Object, ObjectId, Stream,
    content::{Content, Operation},
};
use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};
use thiserror::Error;

use crate::{
    pdf_flatten::configured_max_render_dpi,
    pdfium_backend::{PdfiumInvertAttempt, PdfiumInvertError, try_invert_pdf_to_file},
};

/// Upper bound on the samples of a single image the CMYK conversion will rewrite.
/// Larger images keep their original colour space instead of being buffered.
const MAX_CMYK_IMAGE_SAMPLES: usize = 256 * 1024 * 1024;

/// Color transformation requested by the `replace-invert-pdf` endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceAndInvert {
    HighContrastColor,
    CustomColor,
    FullInversion,
    ColorSpaceConversion,
}

/// Preset text/background combinations accepted by the Java request model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighContrastColorCombination {
    WhiteTextOnBlack,
    BlackTextOnWhite,
    YellowTextOnBlack,
    GreenTextOnBlack,
}

impl HighContrastColorCombination {
    /// Parses `highContrastColorCombination`.
    ///
    /// # Errors
    ///
    /// Returns [`ReplaceInvertError::InvalidHighContrastCombination`] for unknown values.
    pub fn parse(value: &str) -> Result<Self, ReplaceInvertError> {
        match value.trim() {
            "WHITE_TEXT_ON_BLACK" => Ok(Self::WhiteTextOnBlack),
            "BLACK_TEXT_ON_WHITE" => Ok(Self::BlackTextOnWhite),
            "YELLOW_TEXT_ON_BLACK" => Ok(Self::YellowTextOnBlack),
            "GREEN_TEXT_ON_BLACK" => Ok(Self::GreenTextOnBlack),
            _ => Err(ReplaceInvertError::InvalidHighContrastCombination),
        }
    }

    fn colors(self) -> ([u8; 3], [u8; 3]) {
        match self {
            Self::WhiteTextOnBlack => ([255, 255, 255], [0, 0, 0]),
            Self::BlackTextOnWhite => ([0, 0, 0], [255, 255, 255]),
            Self::YellowTextOnBlack => ([255, 255, 0], [0, 0, 0]),
            Self::GreenTextOnBlack => ([0, 255, 0], [0, 0, 0]),
        }
    }
}

/// Complete request options for the replace/invert endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceInvertOptions {
    pub option: ReplaceAndInvert,
    pub high_contrast_combination: HighContrastColorCombination,
    pub background_color: Option<String>,
    pub text_color: Option<String>,
}

impl ReplaceAndInvert {
    /// Parses the `replaceAndInvertOption` form value.
    ///
    /// # Errors
    ///
    /// Returns [`ReplaceInvertError::InvalidOption`] for unknown values.
    pub fn parse(value: &str) -> Result<Self, ReplaceInvertError> {
        match value.trim() {
            "HIGH_CONTRAST_COLOR" => Ok(Self::HighContrastColor),
            "CUSTOM_COLOR" => Ok(Self::CustomColor),
            "FULL_INVERSION" => Ok(Self::FullInversion),
            "COLOR_SPACE_CONVERSION" => Ok(Self::ColorSpaceConversion),
            _ => Err(ReplaceInvertError::InvalidOption),
        }
    }
}

#[derive(Debug, Error)]
pub enum ReplaceInvertError {
    #[error(
        "replaceAndInvertOption must be HIGH_CONTRAST_COLOR, CUSTOM_COLOR, FULL_INVERSION, or COLOR_SPACE_CONVERSION"
    )]
    InvalidOption,
    #[error("highContrastColorCombination is invalid")]
    InvalidHighContrastCombination,
    #[error("{0} must be a Java Color.decode-compatible value")]
    InvalidColor(&'static str),
    #[error("could not read '{filename}' as a PDF: {source}")]
    ReadPdf {
        filename: String,
        #[source]
        source: lopdf::Error,
    },
    #[error("could not recolor PDF content: {0}")]
    Pdf(#[from] lopdf::Error),
    #[error("could not write recolored PDF: {0}")]
    Write(#[from] std::io::Error),
    #[error("PDFium is required to invert PDF colors: {details}")]
    PdfiumUnavailable {
        explicitly_configured: bool,
        details: String,
    },
    #[error(transparent)]
    Pdfium(#[from] PdfiumInvertError),
}

/// Applies the requested color transformation and writes the result to `output_path`.
///
/// `FULL_INVERSION` rasterizes each page and inverts its colors. `COLOR_SPACE_CONVERSION`
/// rewrites device colors and images into `DeviceCMYK` in process. High-contrast and custom
/// modes prepend a page background and recolor text-showing operations in page/Form content
/// streams.
///
/// # Errors
///
/// Returns [`ReplaceInvertError`] when the option is unsupported, `PDFium` is unavailable,
/// or the PDF cannot be read, rewritten, or saved.
pub fn replace_invert_color_to_file(
    input_path: &Path,
    filename: &str,
    options: &ReplaceInvertOptions,
    output_path: &Path,
) -> Result<(), ReplaceInvertError> {
    match options.option {
        ReplaceAndInvert::FullInversion => invert_full_color(input_path, filename, output_path),
        ReplaceAndInvert::ColorSpaceConversion => {
            convert_color_space_cmyk(input_path, filename, output_path)
        }
        ReplaceAndInvert::HighContrastColor => {
            let (text_color, background_color) = options.high_contrast_combination.colors();
            recolor_text_content(
                input_path,
                filename,
                text_color,
                background_color,
                output_path,
            )
        }
        ReplaceAndInvert::CustomColor => {
            let text_color = decode_java_color(options.text_color.as_deref(), "textColor")?;
            let background_color =
                decode_java_color(options.background_color.as_deref(), "backGroundColor")?;
            recolor_text_content(
                input_path,
                filename,
                text_color,
                background_color,
                output_path,
            )
        }
    }
}

#[derive(Debug, Clone, Default)]
struct NonStrokingColorState {
    color_space: Option<Operation>,
    color: Option<Operation>,
}

fn recolor_text_content(
    input_path: &Path,
    filename: &str,
    text_color: [u8; 3],
    background_color: [u8; 3],
    output_path: &Path,
) -> Result<(), ReplaceInvertError> {
    let mut document =
        Document::load(input_path).map_err(|source| ReplaceInvertError::ReadPdf {
            filename: filename.to_owned(),
            source,
        })?;
    let page_ids = document.page_iter().collect::<Vec<_>>();
    let mut visited_forms = HashSet::new();
    for page_id in page_ids {
        let form_ids = page_form_xobject_ids(&document, page_id)?;
        recolor_page_content(&mut document, page_id, text_color, background_color)?;
        for form_id in form_ids {
            recolor_form_xobject(&mut document, form_id, text_color, &mut visited_forms)?;
        }
    }
    document.save(output_path)?;
    Ok(())
}

fn recolor_page_content(
    document: &mut Document,
    page_id: ObjectId,
    text_color: [u8; 3],
    background_color: [u8; 3],
) -> Result<(), ReplaceInvertError> {
    let content_data = document.get_page_content(page_id);
    let mut content = Content::decode(&content_data)?;
    recolor_text_operations(&mut content.operations, text_color);
    let media_box = inherited_page_box(document, page_id)?;
    let mut operations = background_operations(media_box, background_color);
    operations.extend(content.operations);
    document.change_page_content(page_id, Content { operations }.encode()?)?;
    Ok(())
}

fn recolor_form_xobject(
    document: &mut Document,
    form_id: ObjectId,
    text_color: [u8; 3],
    visited_forms: &mut HashSet<ObjectId>,
) -> Result<(), ReplaceInvertError> {
    if !visited_forms.insert(form_id) {
        return Ok(());
    }
    let child_forms = form_xobject_ids(document, form_id)?;
    let content_data = document
        .get_object(form_id)?
        .as_stream()?
        .decompressed_content()?;
    let mut content = Content::decode(&content_data)?;
    recolor_text_operations(&mut content.operations, text_color);
    document
        .get_object_mut(form_id)?
        .as_stream_mut()?
        .set_plain_content(content.encode()?);
    for child_id in child_forms {
        recolor_form_xobject(document, child_id, text_color, visited_forms)?;
    }
    Ok(())
}

fn recolor_text_operations(operations: &mut Vec<Operation>, text_color: [u8; 3]) {
    let mut output = Vec::with_capacity(operations.len());
    let mut state = NonStrokingColorState::default();
    let mut stack = Vec::new();
    for operation in operations.drain(..) {
        match operation.operator.as_str() {
            "q" => stack.push(state.clone()),
            "Q" => state = stack.pop().unwrap_or_default(),
            "cs" => {
                state.color_space = Some(operation.clone());
                state.color = None;
            }
            "g" | "rg" | "k" => {
                state.color_space = None;
                state.color = Some(operation.clone());
            }
            "sc" | "scn" => state.color = Some(operation.clone()),
            "Tj" | "TJ" | "'" | "\"" => {
                output.push(rgb_operation(text_color));
                output.push(operation);
                append_color_restore(&mut output, &state);
                continue;
            }
            _ => {}
        }
        output.push(operation);
    }
    *operations = output;
}

fn append_color_restore(output: &mut Vec<Operation>, state: &NonStrokingColorState) {
    if let Some(color_space) = &state.color_space {
        output.push(color_space.clone());
    }
    if let Some(color) = &state.color {
        output.push(color.clone());
    } else if state.color_space.is_none() {
        output.push(Operation::new("g", vec![Object::Real(0.0)]));
    }
}

fn rgb_operation(color: [u8; 3]) -> Operation {
    Operation::new(
        "rg",
        color
            .into_iter()
            .map(|channel| Object::Real(f32::from(channel) / 255.0))
            .collect(),
    )
}

fn background_operations(media_box: [f32; 4], color: [u8; 3]) -> Vec<Operation> {
    vec![
        Operation::new("q", Vec::new()),
        rgb_operation(color),
        Operation::new(
            "re",
            vec![
                Object::Real(media_box[0]),
                Object::Real(media_box[1]),
                Object::Real(media_box[2] - media_box[0]),
                Object::Real(media_box[3] - media_box[1]),
            ],
        ),
        Operation::new("f", Vec::new()),
        Operation::new("Q", Vec::new()),
    ]
}

fn inherited_page_box(
    document: &Document,
    mut object_id: ObjectId,
) -> Result<[f32; 4], ReplaceInvertError> {
    loop {
        let dictionary = document.get_dictionary(object_id)?;
        if let Ok(media_box) = dictionary.get(b"MediaBox") {
            let (_, media_box) = document.dereference(media_box)?;
            let values = media_box.as_array()?;
            if values.len() == 4 {
                return Ok([
                    object_number(&values[0])?,
                    object_number(&values[1])?,
                    object_number(&values[2])?,
                    object_number(&values[3])?,
                ]);
            }
        }
        object_id = dictionary.get(b"Parent")?.as_reference()?;
    }
}

#[allow(clippy::cast_precision_loss)]
fn object_number(object: &Object) -> Result<f32, ReplaceInvertError> {
    match object {
        Object::Integer(value) => Ok(*value as f32),
        Object::Real(value) => Ok(*value),
        _ => Err(lopdf::Error::ObjectType {
            expected: "Integer or Real",
            found: object.enum_variant(),
        }
        .into()),
    }
}

fn page_form_xobject_ids(
    document: &Document,
    page_id: ObjectId,
) -> Result<Vec<ObjectId>, ReplaceInvertError> {
    let (resources, resource_ids) = document.get_page_resources(page_id)?;
    let mut forms = HashSet::new();
    if let Some(resources) = resources {
        forms.extend(form_xobject_ids_from_resources(document, resources));
    }
    for resource_id in resource_ids {
        if let Ok(resources) = document.get_dictionary(resource_id) {
            forms.extend(form_xobject_ids_from_resources(document, resources));
        }
    }
    Ok(forms.into_iter().collect())
}

fn form_xobject_ids(
    document: &Document,
    form_id: ObjectId,
) -> Result<Vec<ObjectId>, ReplaceInvertError> {
    let form = document.get_object(form_id)?.as_stream()?;
    Ok(form
        .dict
        .get(b"Resources")
        .ok()
        .and_then(|resources| resource_dictionary(document, resources))
        .map_or_else(Vec::new, |resources| {
            form_xobject_ids_from_resources(document, resources)
        }))
}

fn form_xobject_ids_from_resources(document: &Document, resources: &Dictionary) -> Vec<ObjectId> {
    resources
        .get(b"XObject")
        .ok()
        .and_then(|xobjects| resource_dictionary(document, xobjects))
        .map(|xobjects| {
            xobjects
                .iter()
                .filter_map(|(_, object)| object.as_reference().ok())
                .filter(|object_id| is_form_xobject(document, *object_id))
                .collect()
        })
        .unwrap_or_default()
}

fn resource_dictionary<'a>(document: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    match object {
        Object::Reference(object_id) => document.get_dictionary(*object_id).ok(),
        Object::Dictionary(dictionary) => Some(dictionary),
        _ => None,
    }
}

fn is_form_xobject(document: &Document, object_id: ObjectId) -> bool {
    document
        .get_object(object_id)
        .ok()
        .and_then(|object| object.as_stream().ok())
        .and_then(|stream| stream.dict.get(b"Subtype").ok())
        .is_some_and(|subtype| subtype.as_name().is_ok_and(|name| name == b"Form"))
}

fn decode_java_color(
    value: Option<&str>,
    field: &'static str,
) -> Result<[u8; 3], ReplaceInvertError> {
    let value = value
        .filter(|value| !value.is_empty())
        .ok_or(ReplaceInvertError::InvalidColor(field))?;
    let decoded = decode_java_integer(value).ok_or(ReplaceInvertError::InvalidColor(field))?;
    let bytes = u32::from_ne_bytes(decoded.to_ne_bytes()).to_be_bytes();
    Ok([bytes[1], bytes[2], bytes[3]])
}

fn decode_java_integer(value: &str) -> Option<i32> {
    let (negative, unsigned) = match value.as_bytes().first().copied() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    if unsigned.is_empty() {
        return None;
    }
    let (radix, digits) = if let Some(digits) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        (16, digits)
    } else if let Some(digits) = unsigned.strip_prefix('#') {
        (16, digits)
    } else if unsigned.len() > 1 && unsigned.starts_with('0') {
        (8, &unsigned[1..])
    } else {
        (10, unsigned)
    };
    let magnitude = i64::from_str_radix(digits, radix).ok()?;
    let signed = if negative { -magnitude } else { magnitude };
    i32::try_from(signed).ok()
}

fn invert_full_color(
    input_path: &Path,
    filename: &str,
    output_path: &Path,
) -> Result<(), ReplaceInvertError> {
    let render_dpi = configured_max_render_dpi();
    match try_invert_pdf_to_file(input_path, filename, render_dpi, output_path)? {
        PdfiumInvertAttempt::Inverted => Ok(()),
        PdfiumInvertAttempt::Unavailable {
            explicitly_configured,
            details,
        } => Err(ReplaceInvertError::PdfiumUnavailable {
            explicitly_configured,
            details,
        }),
    }
}

/// Converts a PDF into `DeviceCMYK` without any external process.
///
/// Two layers are rewritten:
///
/// * **Content streams** — page contents and (nested, shared) Form `XObjects`. Every
///   `DeviceGray`/`DeviceRGB` colour operator (`g`, `G`, `rg`, `RG`, and `sc`/`scn`/`SC`/`SCN`
///   under a `cs`/`CS`-selected device space) is replaced by the equivalent `k`/`K` operator
///   using the PDF device conversion of ISO 32000-1 §10.4. Colours already in `DeviceCMYK`
///   are left untouched, and non-device spaces (`ICCBased`, Indexed, Separation, `DeviceN`, Lab,
///   Pattern) are preserved verbatim rather than guessed at.
/// * **Image `XObjects`** — 8-bit-per-component gray/RGB images, including `ICCBased` ones,
///   are resampled into `DeviceCMYK`. An embedded ICC profile is first converted to sRGB with
///   `moxcms` (the same bounded ICC handling `pdf_json` uses) before the device conversion.
///
/// # Errors
///
/// Returns [`ReplaceInvertError`] when the PDF cannot be read, a content stream cannot be
/// decoded or re-encoded, or the result cannot be written.
fn convert_color_space_cmyk(
    input_path: &Path,
    filename: &str,
    output_path: &Path,
) -> Result<(), ReplaceInvertError> {
    let mut document =
        Document::load(input_path).map_err(|source| ReplaceInvertError::ReadPdf {
            filename: filename.to_owned(),
            source,
        })?;
    let page_ids = document.page_iter().collect::<Vec<_>>();
    let mut visited_forms = HashSet::new();
    for page_id in page_ids {
        let color_spaces = page_color_space_names(&document, page_id);
        let form_ids = page_form_xobject_ids(&document, page_id)?;
        let content_data = document.get_page_content(page_id);
        let mut content = Content::decode(&content_data)?;
        convert_content_to_cmyk(&mut content.operations, &color_spaces);
        document.change_page_content(page_id, content.encode()?)?;
        for form_id in form_ids {
            convert_form_xobject_to_cmyk(&mut document, form_id, &mut visited_forms)?;
        }
    }
    convert_images_to_cmyk(&mut document);
    document.save(output_path)?;
    Ok(())
}

fn convert_form_xobject_to_cmyk(
    document: &mut Document,
    form_id: ObjectId,
    visited_forms: &mut HashSet<ObjectId>,
) -> Result<(), ReplaceInvertError> {
    if !visited_forms.insert(form_id) {
        return Ok(());
    }
    let child_forms = form_xobject_ids(document, form_id)?;
    let form = document.get_object(form_id)?.as_stream()?;
    let color_spaces = form
        .dict
        .get(b"Resources")
        .ok()
        .and_then(|resources| resource_dictionary(document, resources))
        .map(|resources| device_color_space_names(document, resources))
        .unwrap_or_default();
    let content_data = form.decompressed_content()?;
    let mut content = Content::decode(&content_data)?;
    convert_content_to_cmyk(&mut content.operations, &color_spaces);
    document
        .get_object_mut(form_id)?
        .as_stream_mut()?
        .set_plain_content(content.encode()?);
    for child_id in child_forms {
        convert_form_xobject_to_cmyk(document, child_id, visited_forms)?;
    }
    Ok(())
}

/// The device colour space a `cs`/`CS` operand selects, as far as the conversion cares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DeviceColorSpace {
    Gray,
    Rgb,
    /// `DeviceCMYK` or any space this conversion deliberately leaves alone.
    #[default]
    Other,
}

#[derive(Clone, Copy, Debug, Default)]
struct ColorSpaceState {
    non_stroking: DeviceColorSpace,
    stroking: DeviceColorSpace,
}

/// Rewrites the device colour operators of one content stream into `DeviceCMYK`.
fn convert_content_to_cmyk(
    operations: &mut Vec<Operation>,
    color_spaces: &HashMap<Vec<u8>, DeviceColorSpace>,
) {
    let mut output = Vec::with_capacity(operations.len());
    let mut state = ColorSpaceState::default();
    let mut stack: Vec<ColorSpaceState> = Vec::new();
    for operation in operations.drain(..) {
        match operation.operator.as_str() {
            "q" => stack.push(state),
            "Q" => state = stack.pop().unwrap_or_default(),
            "g" | "G" => {
                let stroking = operation.operator == "G";
                if let Some(gray) = single_component(&operation.operands) {
                    output.push(cmyk_operation(gray_to_cmyk(gray), stroking));
                    set_space(&mut state, stroking, DeviceColorSpace::Other);
                    continue;
                }
            }
            "rg" | "RG" => {
                let stroking = operation.operator == "RG";
                if let Some(rgb) = three_components(&operation.operands) {
                    output.push(cmyk_operation(rgb_to_cmyk(rgb), stroking));
                    set_space(&mut state, stroking, DeviceColorSpace::Other);
                    continue;
                }
            }
            "k" | "K" => set_space(
                &mut state,
                operation.operator == "K",
                DeviceColorSpace::Other,
            ),
            "cs" | "CS" => {
                let stroking = operation.operator == "CS";
                let space = operation
                    .operands
                    .first()
                    .and_then(|operand| operand.as_name().ok())
                    .map_or(DeviceColorSpace::Other, |name| {
                        named_device_color_space(name, color_spaces)
                    });
                set_space(&mut state, stroking, space);
                if space == DeviceColorSpace::Other {
                    output.push(operation);
                } else {
                    output.push(Operation::new(
                        if stroking { "CS" } else { "cs" },
                        vec![Object::Name(b"DeviceCMYK".to_vec())],
                    ));
                }
                continue;
            }
            "sc" | "scn" | "SC" | "SCN" => {
                let stroking = matches!(operation.operator.as_str(), "SC" | "SCN");
                let space = if stroking {
                    state.stroking
                } else {
                    state.non_stroking
                };
                let converted = match space {
                    DeviceColorSpace::Gray => {
                        single_component(&operation.operands).map(gray_to_cmyk)
                    }
                    DeviceColorSpace::Rgb => three_components(&operation.operands).map(rgb_to_cmyk),
                    DeviceColorSpace::Other => None,
                };
                if let Some(cmyk) = converted {
                    output.push(cmyk_operation(cmyk, stroking));
                    continue;
                }
            }
            _ => {}
        }
        output.push(operation);
    }
    *operations = output;
}

fn set_space(state: &mut ColorSpaceState, stroking: bool, space: DeviceColorSpace) {
    if stroking {
        state.stroking = space;
    } else {
        state.non_stroking = space;
    }
}

fn cmyk_operation(cmyk: [f32; 4], stroking: bool) -> Operation {
    Operation::new(
        if stroking { "K" } else { "k" },
        cmyk.into_iter().map(Object::Real).collect(),
    )
}

/// ISO 32000-1 §10.4: `DeviceGray` to `DeviceCMYK`.
fn gray_to_cmyk(gray: f32) -> [f32; 4] {
    [0.0, 0.0, 0.0, (1.0 - gray).clamp(0.0, 1.0)]
}

/// ISO 32000-1 §10.4: `DeviceRGB` to `DeviceCMYK` with full black generation and
/// undercolour removal (`k = min(c, m, y)`).
fn rgb_to_cmyk(rgb: [f32; 3]) -> [f32; 4] {
    let cyan = (1.0 - rgb[0]).clamp(0.0, 1.0);
    let magenta = (1.0 - rgb[1]).clamp(0.0, 1.0);
    let yellow = (1.0 - rgb[2]).clamp(0.0, 1.0);
    let black = cyan.min(magenta).min(yellow);
    [cyan - black, magenta - black, yellow - black, black]
}

fn rgb_bytes_to_cmyk_bytes(rgb: &[u8]) -> Vec<u8> {
    let mut cmyk = Vec::with_capacity(rgb.len() / 3 * 4);
    for pixel in rgb.chunks_exact(3) {
        let components = rgb_to_cmyk([
            f32::from(pixel[0]) / 255.0,
            f32::from(pixel[1]) / 255.0,
            f32::from(pixel[2]) / 255.0,
        ]);
        for component in components {
            cmyk.push(sample_byte(component));
        }
    }
    cmyk
}

fn gray_bytes_to_cmyk_bytes(gray: &[u8]) -> Vec<u8> {
    let mut cmyk = Vec::with_capacity(gray.len() * 4);
    for value in gray {
        cmyk.extend_from_slice(&[0, 0, 0, 255 - *value]);
    }
    cmyk
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn sample_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn single_component(operands: &[Object]) -> Option<f32> {
    match operands {
        [value] => operand_number(value),
        _ => None,
    }
}

fn three_components(operands: &[Object]) -> Option<[f32; 3]> {
    match operands {
        [red, green, blue] => Some([
            operand_number(red)?,
            operand_number(green)?,
            operand_number(blue)?,
        ]),
        _ => None,
    }
}

#[allow(clippy::cast_precision_loss)]
fn operand_number(object: &Object) -> Option<f32> {
    match object {
        Object::Integer(value) => Some(*value as f32),
        Object::Real(value) => Some(*value),
        _ => None,
    }
}

fn named_device_color_space(
    name: &[u8],
    color_spaces: &HashMap<Vec<u8>, DeviceColorSpace>,
) -> DeviceColorSpace {
    match name {
        b"DeviceGray" | b"G" | b"CalGray" => DeviceColorSpace::Gray,
        b"DeviceRGB" | b"RGB" | b"CalRGB" => DeviceColorSpace::Rgb,
        _ => color_spaces
            .get(name)
            .copied()
            .unwrap_or(DeviceColorSpace::Other),
    }
}

fn page_color_space_names(
    document: &Document,
    page_id: ObjectId,
) -> HashMap<Vec<u8>, DeviceColorSpace> {
    let mut names = HashMap::new();
    if let Ok((resources, resource_ids)) = document.get_page_resources(page_id) {
        if let Some(resources) = resources {
            names.extend(device_color_space_names(document, resources));
        }
        for resource_id in resource_ids {
            if let Ok(resources) = document.get_dictionary(resource_id) {
                names.extend(device_color_space_names(document, resources));
            }
        }
    }
    names
}

/// Maps `/ColorSpace` resource names onto the device space they resolve to.
/// Only direct `/DeviceGray`, `/DeviceRGB`, `/CalGray`, and `/CalRGB` entries are
/// classified; anything else stays [`DeviceColorSpace::Other`] and is untouched.
fn device_color_space_names(
    document: &Document,
    resources: &Dictionary,
) -> HashMap<Vec<u8>, DeviceColorSpace> {
    let Some(color_spaces) = resources
        .get(b"ColorSpace")
        .ok()
        .and_then(|color_spaces| resource_dictionary(document, color_spaces))
    else {
        return HashMap::new();
    };
    color_spaces
        .iter()
        .filter_map(|(name, object)| {
            let (_, resolved) = document.dereference(object).ok()?;
            let space = match resolved.as_name().ok()? {
                b"DeviceGray" | b"CalGray" => DeviceColorSpace::Gray,
                b"DeviceRGB" | b"CalRGB" => DeviceColorSpace::Rgb,
                _ => return None,
            };
            Some((name.clone(), space))
        })
        .collect()
}

/// Rewrites eligible image `XObjects` into `DeviceCMYK`.
///
/// Images that act as soft masks or stencil masks, carry a `/Decode` array, use a
/// non-8-bit depth, use a colour space this conversion does not model, or whose data
/// cannot be decoded with `lopdf` alone (DCT, JPX, CCITT, JBIG2) are left untouched:
/// a wrong rewrite would corrupt them, and leaving them is visible and reversible.
fn convert_images_to_cmyk(document: &mut Document) {
    let mask_ids = mask_object_ids(document);
    let image_ids = document
        .objects
        .iter()
        .filter(|(object_id, object)| {
            !mask_ids.contains(*object_id)
                && object.as_stream().is_ok_and(|stream| {
                    stream
                        .dict
                        .get(b"Subtype")
                        .and_then(Object::as_name)
                        .is_ok_and(|subtype| subtype == b"Image")
                })
        })
        .map(|(object_id, _)| *object_id)
        .collect::<Vec<_>>();
    for image_id in image_ids {
        let Ok(stream) = document.get_object(image_id).and_then(Object::as_stream) else {
            continue;
        };
        let Some(samples) = image_cmyk_samples(document, stream) else {
            continue;
        };
        if let Ok(stream) = document
            .get_object_mut(image_id)
            .and_then(Object::as_stream_mut)
        {
            write_cmyk_image(stream, samples);
        }
    }
}

fn write_cmyk_image(stream: &mut Stream, samples: Vec<u8>) {
    stream.dict.remove(b"Filter");
    stream.dict.remove(b"DecodeParms");
    stream
        .dict
        .set("ColorSpace", Object::Name(b"DeviceCMYK".to_vec()));
    stream.dict.set("BitsPerComponent", 8);
    stream.set_plain_content(samples);
    let _ = stream.compress();
}

/// Object ids used as soft masks or stencil masks, which must keep their own
/// single-channel colour space.
fn mask_object_ids(document: &Document) -> HashSet<ObjectId> {
    document
        .objects
        .values()
        .filter_map(|object| object.as_stream().ok())
        .flat_map(|stream| {
            [b"SMask".as_slice(), b"Mask"]
                .into_iter()
                .filter_map(|key| stream.dict.get(key).ok()?.as_reference().ok())
        })
        .collect()
}

fn image_cmyk_samples(document: &Document, stream: &lopdf::Stream) -> Option<Vec<u8>> {
    if stream.dict.get(b"ImageMask").is_ok() || stream.dict.get(b"Decode").is_ok() {
        return None;
    }
    if stream
        .dict
        .get(b"BitsPerComponent")
        .and_then(Object::as_i64)
        .ok()?
        != 8
    {
        return None;
    }
    if !decodable_with_lopdf(&stream.dict) {
        return None;
    }
    let width = usize::try_from(stream.dict.get(b"Width").and_then(Object::as_i64).ok()?).ok()?;
    let height = usize::try_from(stream.dict.get(b"Height").and_then(Object::as_i64).ok()?).ok()?;
    let color_space = image_color_space(document, stream)?;
    let channels = color_space.channels();
    let expected = width.checked_mul(height)?.checked_mul(channels)?;
    if expected == 0 || expected > MAX_CMYK_IMAGE_SAMPLES {
        return None;
    }
    let data = stream.decompressed_content().ok()?;
    if data.len() < expected {
        return None;
    }
    let data = &data[..expected];
    match color_space {
        ImageColorSpace::Gray => Some(gray_bytes_to_cmyk_bytes(data)),
        ImageColorSpace::Rgb => Some(rgb_bytes_to_cmyk_bytes(data)),
        ImageColorSpace::Icc { channels, profile } => {
            let rgb = icc_samples_to_rgb(data, channels, &profile)?;
            Some(rgb_bytes_to_cmyk_bytes(&rgb))
        }
    }
}

/// Whether the stream's filter chain is one `lopdf` can decode on its own.
fn decodable_with_lopdf(dictionary: &Dictionary) -> bool {
    let filters: Vec<&[u8]> = match dictionary.get(b"Filter") {
        Err(_) => return true,
        Ok(Object::Name(name)) => vec![name.as_slice()],
        Ok(Object::Array(names)) => names
            .iter()
            .filter_map(|name| name.as_name().ok())
            .collect(),
        Ok(_) => return false,
    };
    filters.iter().all(|filter| {
        matches!(
            *filter,
            b"FlateDecode" | b"Fl" | b"LZWDecode" | b"LZW" | b"ASCII85Decode" | b"ASCIIHexDecode"
        )
    })
}

enum ImageColorSpace {
    Gray,
    Rgb,
    Icc { channels: usize, profile: Vec<u8> },
}

impl ImageColorSpace {
    fn channels(&self) -> usize {
        match self {
            Self::Gray => 1,
            Self::Rgb => 3,
            Self::Icc { channels, .. } => *channels,
        }
    }
}

fn image_color_space(document: &Document, stream: &lopdf::Stream) -> Option<ImageColorSpace> {
    let (_, color_space) = document
        .dereference(stream.dict.get(b"ColorSpace").ok()?)
        .ok()?;
    match color_space {
        Object::Name(name) => match name.as_slice() {
            b"DeviceGray" | b"G" | b"CalGray" => Some(ImageColorSpace::Gray),
            b"DeviceRGB" | b"RGB" | b"CalRGB" => Some(ImageColorSpace::Rgb),
            _ => None,
        },
        Object::Array(values) => icc_image_color_space(document, values),
        _ => None,
    }
}

fn icc_image_color_space(document: &Document, values: &[Object]) -> Option<ImageColorSpace> {
    if values.first()?.as_name().ok()? != b"ICCBased" {
        return None;
    }
    let (_, profile_stream) = document.dereference(values.get(1)?).ok()?;
    let profile_stream = profile_stream.as_stream().ok()?;
    let channels = usize::try_from(
        profile_stream
            .dict
            .get(b"N")
            .and_then(Object::as_i64)
            .ok()?,
    )
    .ok()?;
    if !matches!(channels, 1 | 3) {
        return None;
    }
    let profile = profile_stream.decompressed_content().ok()?;
    Some(ImageColorSpace::Icc { channels, profile })
}

/// Converts ICC-tagged samples to sRGB, mirroring the bounded ICC handling in
/// `pdf_json`: an unusable profile makes the caller skip the image rather than
/// guess at its colours.
fn icc_samples_to_rgb(samples: &[u8], channels: usize, profile: &[u8]) -> Option<Vec<u8>> {
    if channels == 0 || !samples.len().is_multiple_of(channels) {
        return None;
    }
    let source_profile = ColorProfile::new_from_slice(profile).ok()?;
    let source_layout = match (channels, source_profile.color_space) {
        (1, DataColorSpace::Gray) => Layout::Gray,
        (3, DataColorSpace::Rgb) => Layout::Rgb,
        _ => return None,
    };
    let destination_profile = ColorProfile::new_srgb();
    let transform = source_profile
        .create_transform_8bit(
            source_layout,
            &destination_profile,
            Layout::Rgb,
            TransformOptions::default(),
        )
        .ok()?;
    let pixel_count = samples.len().checked_div(channels)?;
    let mut rgb = vec![0; pixel_count.checked_mul(3)?];
    transform.transform(samples, &mut rgb).ok()?;
    Some(rgb)
}

#[cfg(test)]
mod tests {
    use lopdf::{Object, content::Operation};

    use super::{
        HighContrastColorCombination, ReplaceAndInvert, ReplaceInvertError, decode_java_color,
        recolor_text_operations,
    };

    #[test]
    fn parses_every_known_option() {
        assert!(matches!(
            ReplaceAndInvert::parse("FULL_INVERSION"),
            Ok(ReplaceAndInvert::FullInversion)
        ));
        assert!(matches!(
            ReplaceAndInvert::parse(" COLOR_SPACE_CONVERSION "),
            Ok(ReplaceAndInvert::ColorSpaceConversion)
        ));
        assert!(matches!(
            ReplaceAndInvert::parse("HIGH_CONTRAST_COLOR"),
            Ok(ReplaceAndInvert::HighContrastColor)
        ));
        assert!(matches!(
            ReplaceAndInvert::parse("CUSTOM_COLOR"),
            Ok(ReplaceAndInvert::CustomColor)
        ));
    }

    #[test]
    fn rejects_unknown_option() {
        assert!(matches!(
            ReplaceAndInvert::parse("SEPIA"),
            Err(ReplaceInvertError::InvalidOption)
        ));
    }

    #[test]
    fn parses_high_contrast_presets_and_java_colors() {
        assert!(matches!(
            HighContrastColorCombination::parse("YELLOW_TEXT_ON_BLACK"),
            Ok(HighContrastColorCombination::YellowTextOnBlack)
        ));
        assert!(matches!(
            decode_java_color(Some("#12ABEF"), "textColor"),
            Ok([0x12, 0xAB, 0xEF])
        ));
        assert!(matches!(
            decode_java_color(Some("010"), "textColor"),
            Ok([0, 0, 8])
        ));
        assert!(matches!(
            decode_java_color(None, "textColor"),
            Err(ReplaceInvertError::InvalidColor("textColor"))
        ));
    }

    #[test]
    fn text_recoloring_restores_the_previous_non_stroking_color() {
        let original = Operation::new(
            "rg",
            vec![Object::Real(1.0), Object::Real(0.0), Object::Real(0.0)],
        );
        let mut operations = vec![
            original.clone(),
            Operation::new("Tj", vec![Object::string_literal("text")]),
            Operation::new("re", vec![0.into(), 0.into(), 10.into(), 10.into()]),
        ];
        recolor_text_operations(&mut operations, [0, 0, 255]);
        assert_eq!(
            operations
                .iter()
                .map(|operation| operation.operator.as_str())
                .collect::<Vec<_>>(),
            ["rg", "rg", "Tj", "rg", "re"]
        );
        assert_eq!(operations[3].operator, original.operator);
        assert_eq!(operations[3].operands, original.operands);
    }
}
