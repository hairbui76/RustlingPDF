use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write as _,
    path::Path,
};

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use serde::Deserialize;
use thiserror::Error;

use crate::{
    pdf_form_mutation::{create_text_appearance, pdf_text_string},
    pdf_page_geometry::inherited_value,
};

const FLAG_READ_ONLY: i64 = 1;
const FLAG_REQUIRED: i64 = 1 << 1;
const FLAG_MULTILINE: i64 = 1 << 12;
const FLAG_RADIO: i64 = 1 << 15;
const FLAG_PUSH_BUTTON: i64 = 1 << 16;
const FLAG_COMBO: i64 = 1 << 17;
const FLAG_MULTI_SELECT: i64 = 1 << 21;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)] // Independent flags in the public form-creation contract.
pub struct FormFieldCreation {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub label: Option<String>,
    pub tooltip: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub multiline: bool,
    #[serde(default)]
    pub multi_select: bool,
    #[serde(default)]
    pub options: Vec<String>,
    pub default_value: Option<String>,
    pub font_size: Option<f32>,
    pub tab_order: Option<i64>,
    pub widgets: Vec<FormWidgetCreation>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormWidgetCreation {
    pub page_index: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub export_value: Option<String>,
}

#[derive(Debug, Error)]
pub enum FormCreationError {
    #[error("could not read PDF '{filename}': {source}")]
    ReadPdf {
        filename: String,
        #[source]
        source: lopdf::Error,
    },
    #[error("invalid field definition at index {index}: {details}")]
    InvalidDefinition { index: usize, details: String },
    #[error("PDF structure error: {0}")]
    Pdf(#[from] lopdf::Error),
    #[error("could not write PDF: {0}")]
    Write(std::io::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FieldType {
    Text,
    Checkbox,
    Radio,
    ComboBox,
    ListBox,
    Button,
    Signature,
}

#[derive(Clone, Copy, Debug)]
struct PageBox {
    id: ObjectId,
    left: f32,
    bottom: f32,
    width: f32,
    height: f32,
}

#[derive(Clone, Debug)]
struct ValidatedWidget {
    page_index: usize,
    rect: [f32; 4],
    export_value: Option<String>,
}

#[derive(Clone, Debug)]
#[allow(clippy::struct_excessive_bools)] // Validated counterparts of the independent request flags.
struct ValidatedField {
    name: String,
    field_type: FieldType,
    label: Option<String>,
    alternate_name: Option<String>,
    required: bool,
    read_only: bool,
    multiline: bool,
    multi_select: bool,
    options: Vec<String>,
    default_value: Option<String>,
    font_size: f32,
    tab_order: Option<i64>,
    widgets: Vec<ValidatedWidget>,
}

#[derive(Clone, Debug)]
struct PendingWidget {
    reference: Object,
    tab_order: Option<i64>,
    field_index: usize,
    widget_index: usize,
}

/// Creates logical `AcroForm` fields and their widgets in an existing PDF.
///
/// Validation is completed before the document is mutated, so an invalid
/// definition never produces a partial output file.
///
/// # Errors
///
/// Returns [`FormCreationError`] when the source PDF or any requested field is
/// invalid, or when the result cannot be written.
pub fn create_fields_to_file(
    input_path: &Path,
    filename: &str,
    definitions: &[FormFieldCreation],
    output_path: &Path,
) -> Result<(), FormCreationError> {
    let mut document = Document::load(input_path).map_err(|source| FormCreationError::ReadPdf {
        filename: filename.to_owned(),
        source,
    })?;
    let page_boxes = collect_page_boxes(&document)?;
    let existing_names = collect_existing_field_names(&document)?;
    let fields = validate_fields(definitions, &page_boxes, existing_names)?;

    let (acroform_id, mut acroform, mut root_fields) = read_or_create_acroform(&mut document)?;
    ensure_default_resources(&document, &mut acroform)?;
    let mut additions: BTreeMap<usize, Vec<PendingWidget>> = BTreeMap::new();

    for (field_index, field) in fields.iter().enumerate() {
        let field_id = document.new_object_id();
        let mut widget_references = Vec::with_capacity(field.widgets.len());

        for (widget_index, widget) in field.widgets.iter().enumerate() {
            let page = page_boxes[widget.page_index];
            let widget_id = document.new_object_id();
            let mut dictionary = dictionary! {
                "Type" => "Annot",
                "Subtype" => "Widget",
                "Parent" => field_id,
                "P" => page.id,
                "Rect" => widget.rect.map(Object::Real).to_vec(),
                "F" => 4,
            };
            if let Some(alternate_name) = &field.alternate_name {
                dictionary.set("TU", pdf_text_string(alternate_name));
            }
            set_widget_appearance(&mut document, &mut dictionary, field, widget, widget_index)?;
            document
                .objects
                .insert(widget_id, Object::Dictionary(dictionary));

            let reference = Object::Reference(widget_id);
            widget_references.push(reference.clone());
            additions
                .entry(widget.page_index)
                .or_default()
                .push(PendingWidget {
                    reference,
                    tab_order: field.tab_order,
                    field_index,
                    widget_index,
                });
        }

        let dictionary = create_field_dictionary(field, widget_references);
        document
            .objects
            .insert(field_id, Object::Dictionary(dictionary));
        root_fields.push(Object::Reference(field_id));
    }

    for (page_index, mut widgets) in additions {
        widgets.sort_by_key(|widget| {
            (
                widget.tab_order.unwrap_or(i64::MAX),
                widget.field_index,
                widget.widget_index,
            )
        });
        let page = page_boxes[page_index];
        let page_dictionary = document.get_dictionary(page.id)?.clone();
        let mut annotations = page_dictionary
            .get(b"Annots")
            .ok()
            .map(|annotations| resolved_array(&document, annotations))
            .transpose()?
            .unwrap_or_default();
        let has_explicit_order = widgets.iter().any(|widget| widget.tab_order.is_some());
        annotations.extend(widgets.into_iter().map(|widget| widget.reference));
        let page_dictionary = document.get_dictionary_mut(page.id)?;
        page_dictionary.set("Annots", annotations);
        if has_explicit_order {
            page_dictionary.set("Tabs", Object::Name(b"A".to_vec()));
        }
    }

    acroform.set("Fields", root_fields);
    acroform.set("NeedAppearances", false);
    document
        .objects
        .insert(acroform_id, Object::Dictionary(acroform));
    document.catalog_mut()?.set("AcroForm", acroform_id);
    document
        .save(output_path)
        .map_err(FormCreationError::Write)?;
    Ok(())
}

fn collect_page_boxes(document: &Document) -> Result<Vec<PageBox>, lopdf::Error> {
    document
        .get_pages()
        .into_values()
        .map(|id| {
            let page_box = inherited_value(document, id, b"CropBox")
                .or_else(|_| inherited_value(document, id, b"MediaBox"))?;
            let page_box = resolved_array(document, &page_box)?;
            if page_box.len() < 4 {
                return Err(lopdf::Error::Syntax(
                    "page CropBox or MediaBox requires four coordinates".to_owned(),
                ));
            }
            let left = page_box[0].as_float()?;
            let bottom = page_box[1].as_float()?;
            let right = page_box[2].as_float()?;
            let top = page_box[3].as_float()?;
            Ok(PageBox {
                id,
                left,
                bottom,
                width: right - left,
                height: top - bottom,
            })
        })
        .collect()
}

#[allow(clippy::too_many_lines)]
fn validate_fields(
    definitions: &[FormFieldCreation],
    pages: &[PageBox],
    mut existing_names: HashSet<String>,
) -> Result<Vec<ValidatedField>, FormCreationError> {
    let mut fields = Vec::with_capacity(definitions.len());
    for (index, definition) in definitions.iter().enumerate() {
        let field_type = parse_field_type(&definition.field_type).ok_or_else(|| {
            invalid(
                index,
                format!("unsupported type '{}'", definition.field_type),
            )
        })?;
        let requested_name = definition.name.trim();
        if requested_name.is_empty() {
            return Err(invalid(index, "name must not be blank"));
        }
        if definition.widgets.is_empty() {
            return Err(invalid(
                index,
                "widgets must contain at least one rectangle",
            ));
        }
        if definition.multiline && field_type != FieldType::Text {
            return Err(invalid(
                index,
                "multiline is only supported for text fields",
            ));
        }
        if definition.multi_select && field_type != FieldType::ListBox {
            return Err(invalid(
                index,
                "multiSelect is only supported for listbox fields",
            ));
        }
        let font_size = definition.font_size.unwrap_or(12.0);
        if !font_size.is_finite() || font_size <= 0.0 {
            return Err(invalid(index, "fontSize must be a positive finite number"));
        }
        let options = definition
            .options
            .iter()
            .map(|option| option.trim().to_owned())
            .collect::<Vec<_>>();
        if matches!(field_type, FieldType::ComboBox | FieldType::ListBox) && options.is_empty() {
            return Err(invalid(
                index,
                "combobox and listbox fields require at least one option",
            ));
        }
        if options.iter().any(String::is_empty) {
            return Err(invalid(index, "options must not contain blank values"));
        }
        if options.iter().collect::<HashSet<_>>().len() != options.len() {
            return Err(invalid(index, "options must not contain duplicate values"));
        }
        if !matches!(
            field_type,
            FieldType::Checkbox | FieldType::Radio | FieldType::ComboBox | FieldType::ListBox
        ) && !options.is_empty()
        {
            return Err(invalid(
                index,
                "options are only supported for button and choice fields",
            ));
        }

        let mut widgets = Vec::with_capacity(definition.widgets.len());
        for widget in &definition.widgets {
            let Some(page) = pages.get(widget.page_index).copied() else {
                return Err(invalid(
                    index,
                    format!("pageIndex {} is outside the document", widget.page_index),
                ));
            };
            if ![
                widget.x,
                widget.y,
                widget.width,
                widget.height,
                page.width,
                page.height,
            ]
            .iter()
            .all(|value| value.is_finite())
                || page.width <= 0.0
                || page.height <= 0.0
            {
                return Err(invalid(index, "page and widget coordinates must be finite"));
            }
            if widget.x < 0.0
                || widget.y < 0.0
                || widget.width <= 0.0
                || widget.height <= 0.0
                || widget.x + widget.width > page.width
                || widget.y + widget.height > page.height
            {
                return Err(invalid(
                    index,
                    format!(
                        "widget rectangle must be fully inside page {} CropBox",
                        widget.page_index
                    ),
                ));
            }
            let left = page.left + widget.x;
            let top = page.bottom + page.height - widget.y;
            widgets.push(ValidatedWidget {
                page_index: widget.page_index,
                rect: [left, top - widget.height, left + widget.width, top],
                export_value: widget
                    .export_value
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned),
            });
        }

        validate_widget_states(index, field_type, &options, &widgets)?;
        let default_value = validate_default(
            index,
            field_type,
            definition.multi_select,
            &options,
            &widgets,
            definition.default_value.as_deref(),
        )?;
        let name = unique_name(requested_name, &existing_names);
        existing_names.insert(name.clone());
        let label = trimmed(definition.label.as_deref());
        let alternate_name = trimmed(definition.tooltip.as_deref()).or_else(|| label.clone());
        fields.push(ValidatedField {
            name,
            field_type,
            label,
            alternate_name,
            required: definition.required,
            read_only: definition.read_only,
            multiline: definition.multiline,
            multi_select: definition.multi_select,
            options,
            default_value,
            font_size,
            tab_order: definition.tab_order,
            widgets,
        });
    }
    Ok(fields)
}

fn validate_widget_states(
    index: usize,
    field_type: FieldType,
    options: &[String],
    widgets: &[ValidatedWidget],
) -> Result<(), FormCreationError> {
    if field_type != FieldType::Radio {
        if widgets.iter().any(|widget| widget.export_value.is_some())
            && field_type != FieldType::Checkbox
        {
            return Err(invalid(
                index,
                "exportValue is only supported for checkbox and radio widgets",
            ));
        }
        return Ok(());
    }
    let explicit = widgets
        .iter()
        .filter(|widget| widget.export_value.is_some())
        .count();
    if explicit != 0 && explicit != widgets.len() {
        return Err(invalid(
            index,
            "radio widgets must either all specify exportValue or all use options",
        ));
    }
    if explicit == 0 && options.len() != widgets.len() {
        return Err(invalid(
            index,
            "radio options must contain exactly one state per widget",
        ));
    }
    let states = radio_states(options, widgets);
    if states.iter().any(|state| state.eq_ignore_ascii_case("Off"))
        || states.iter().collect::<HashSet<_>>().len() != states.len()
    {
        return Err(invalid(
            index,
            "radio export states must be unique and must not use Off",
        ));
    }
    Ok(())
}

fn validate_default(
    index: usize,
    field_type: FieldType,
    multi_select: bool,
    options: &[String],
    widgets: &[ValidatedWidget],
    default_value: Option<&str>,
) -> Result<Option<String>, FormCreationError> {
    let default_value = default_value
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(default_value) = default_value else {
        return Ok(None);
    };
    match field_type {
        FieldType::Button | FieldType::Signature => {
            return Err(invalid(
                index,
                "button and signature fields cannot have a defaultValue",
            ));
        }
        FieldType::ComboBox => {
            if !options.iter().any(|option| option == default_value) {
                return Err(invalid(index, "defaultValue must match a combobox option"));
            }
        }
        FieldType::ListBox => {
            let selected = if multi_select {
                default_value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>()
            } else {
                vec![default_value]
            };
            if selected
                .iter()
                .any(|value| !options.iter().any(|option| option == value))
            {
                return Err(invalid(index, "defaultValue must match listbox options"));
            }
        }
        FieldType::Radio => {
            if !radio_states(options, widgets)
                .iter()
                .any(|state| state == default_value)
            {
                return Err(invalid(
                    index,
                    "defaultValue must match a radio export state",
                ));
            }
        }
        FieldType::Checkbox => {
            let on_state = checkbox_state(options, widgets);
            if !is_false_value(default_value)
                && !is_true_value(default_value)
                && default_value != on_state
            {
                return Err(invalid(
                    index,
                    "checkbox defaultValue must be false, true, Off, or its on-state",
                ));
            }
        }
        FieldType::Text => {}
    }
    Ok(Some(default_value.to_owned()))
}

fn create_field_dictionary(field: &ValidatedField, kids: Vec<Object>) -> Dictionary {
    let mut flags = 0_i64;
    if field.read_only {
        flags |= FLAG_READ_ONLY;
    }
    if field.required {
        flags |= FLAG_REQUIRED;
    }
    if field.multiline {
        flags |= FLAG_MULTILINE;
    }
    flags |= match field.field_type {
        FieldType::Radio => FLAG_RADIO,
        FieldType::Button => FLAG_PUSH_BUTTON,
        FieldType::ComboBox => FLAG_COMBO,
        FieldType::ListBox if field.multi_select => FLAG_MULTI_SELECT,
        _ => 0,
    };
    let pdf_type = match field.field_type {
        FieldType::Text => b"Tx".as_slice(),
        FieldType::Checkbox | FieldType::Radio | FieldType::Button => b"Btn".as_slice(),
        FieldType::ComboBox | FieldType::ListBox => b"Ch".as_slice(),
        FieldType::Signature => b"Sig".as_slice(),
    };
    let mut dictionary = dictionary! {
        "FT" => Object::Name(pdf_type.to_vec()),
        "T" => pdf_text_string(&field.name),
        "Ff" => flags,
        "Kids" => kids,
    };
    if let Some(label) = &field.label {
        dictionary.set("TM", pdf_text_string(label));
    }
    if let Some(alternate_name) = &field.alternate_name {
        dictionary.set("TU", pdf_text_string(alternate_name));
    }
    if matches!(
        field.field_type,
        FieldType::Text | FieldType::ComboBox | FieldType::ListBox
    ) {
        dictionary.set(
            "DA",
            Object::string_literal(format!("/Helv {} Tf 0 g", field.font_size)),
        );
    }
    if matches!(field.field_type, FieldType::ComboBox | FieldType::ListBox) {
        dictionary.set(
            "Opt",
            field
                .options
                .iter()
                .map(|option| pdf_text_string(option))
                .collect::<Vec<_>>(),
        );
    }
    set_field_default(&mut dictionary, field);
    dictionary
}

fn set_field_default(dictionary: &mut Dictionary, field: &ValidatedField) {
    let Some(default_value) = &field.default_value else {
        return;
    };
    match field.field_type {
        FieldType::ListBox if field.multi_select => {
            let selected = default_value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            let values = selected
                .iter()
                .map(|value| pdf_text_string(value))
                .collect::<Vec<_>>();
            let indexes = field
                .options
                .iter()
                .enumerate()
                .filter_map(|(index, option)| {
                    selected
                        .contains(&option.as_str())
                        .then(|| i64::try_from(index).ok())
                        .flatten()
                        .map(Object::Integer)
                })
                .collect::<Vec<_>>();
            dictionary.set("V", values.clone());
            dictionary.set("DV", values);
            dictionary.set("I", indexes);
        }
        FieldType::Text | FieldType::ComboBox | FieldType::ListBox => {
            let value = pdf_text_string(default_value);
            dictionary.set("V", value.clone());
            dictionary.set("DV", value);
        }
        FieldType::Checkbox => {
            let state = if is_false_value(default_value) {
                "Off".to_owned()
            } else {
                checkbox_state(&field.options, &field.widgets)
            };
            dictionary.set("V", Object::Name(state.as_bytes().to_vec()));
            dictionary.set("DV", Object::Name(state.as_bytes().to_vec()));
        }
        FieldType::Radio => {
            dictionary.set("V", Object::Name(default_value.as_bytes().to_vec()));
            dictionary.set("DV", Object::Name(default_value.as_bytes().to_vec()));
        }
        FieldType::Button | FieldType::Signature => {}
    }
}

fn set_widget_appearance(
    document: &mut Document,
    widget_dictionary: &mut Dictionary,
    field: &ValidatedField,
    widget: &ValidatedWidget,
    widget_index: usize,
) -> Result<(), lopdf::Error> {
    match field.field_type {
        FieldType::Text | FieldType::ComboBox | FieldType::ListBox => {
            let value = field.default_value.as_deref().unwrap_or_default();
            if let Some(appearance) = create_text_appearance(
                document,
                widget_dictionary,
                value,
                field.multiline,
                field.font_size,
            )? {
                widget_dictionary.set("AP", dictionary! { "N" => appearance });
            }
        }
        FieldType::Checkbox => {
            let on_state = checkbox_state(&field.options, &field.widgets);
            set_button_state_appearance(
                document,
                widget_dictionary,
                &on_state,
                matches!(
                    field.default_value.as_deref(),
                    Some(value) if !is_false_value(value)
                ),
                false,
            )?;
        }
        FieldType::Radio => {
            let state = radio_states(&field.options, &field.widgets)[widget_index].clone();
            set_button_state_appearance(
                document,
                widget_dictionary,
                &state,
                field.default_value.as_deref() == Some(state.as_str()),
                true,
            )?;
        }
        FieldType::Button => {
            let caption = field.label.as_deref().unwrap_or(&field.name);
            widget_dictionary.set("MK", dictionary! { "CA" => pdf_text_string(caption) });
            let appearance = create_caption_appearance(document, widget_dictionary, caption)?;
            widget_dictionary.set("AP", dictionary! { "N" => appearance });
        }
        FieldType::Signature => {
            let appearance = create_empty_appearance(document, widget_dictionary)?;
            widget_dictionary.set("AP", dictionary! { "N" => appearance });
        }
    }
    if let Some(export_value) = &widget.export_value {
        widget_dictionary.set("Opt", pdf_text_string(export_value));
    }
    Ok(())
}

fn set_button_state_appearance(
    document: &mut Document,
    widget: &mut Dictionary,
    on_state: &str,
    selected: bool,
    radio: bool,
) -> Result<(), lopdf::Error> {
    let dimensions = widget_dimensions(document, widget)?;
    let off = button_appearance(document, dimensions, false, radio);
    let on = button_appearance(document, dimensions, true, radio);
    let mut normal = Dictionary::new();
    normal.set("Off", off);
    normal.set(on_state.as_bytes(), on);
    widget.set("AP", dictionary! { "N" => normal });
    widget.set(
        "AS",
        Object::Name(
            if selected {
                on_state.as_bytes()
            } else {
                b"Off"
            }
            .to_vec(),
        ),
    );
    Ok(())
}

fn button_appearance(
    document: &mut Document,
    (width, height): (f32, f32),
    selected: bool,
    radio: bool,
) -> Object {
    let mut content = format!(
        "q 1 g 0 0 {width:.3} {height:.3} re f 0 G 1 w 0.5 0.5 {:.3} {:.3} re S",
        (width - 1.0).max(0.0),
        (height - 1.0).max(0.0)
    );
    if selected && radio {
        let radius = width.min(height) / 4.0;
        let center_x = width / 2.0;
        let center_y = height / 2.0;
        let control = radius * 0.552_284_8;
        let _ = write!(
            content,
            concat!(
                " 0 g {:.3} {:.3} m ",
                "{:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c ",
                "{:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c ",
                "{:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c ",
                "{:.3} {:.3} {:.3} {:.3} {:.3} {:.3} c f"
            ),
            center_x + radius,
            center_y,
            center_x + radius,
            center_y + control,
            center_x + control,
            center_y + radius,
            center_x,
            center_y + radius,
            center_x - control,
            center_y + radius,
            center_x - radius,
            center_y + control,
            center_x - radius,
            center_y,
            center_x - radius,
            center_y - control,
            center_x - control,
            center_y - radius,
            center_x,
            center_y - radius,
            center_x + control,
            center_y - radius,
            center_x + radius,
            center_y - control,
            center_x + radius,
            center_y,
        );
    } else if selected {
        let _ = write!(
            content,
            " 1.5 w 3 {:.3} m {:.3} 3 l S 3 3 m {:.3} {:.3} l S",
            (height / 2.0).max(3.0),
            (width - 3.0).max(3.0),
            (width - 3.0).max(3.0),
            (height - 3.0).max(3.0)
        );
    }
    content.push_str(" Q");
    appearance_stream(
        document,
        width,
        height,
        Dictionary::new(),
        content.into_bytes(),
    )
}

fn create_caption_appearance(
    document: &mut Document,
    widget: &Dictionary,
    caption: &str,
) -> Result<Object, lopdf::Error> {
    let (width, height) = widget_dimensions(document, widget)?;
    let font_size = (height - 4.0).clamp(4.0, 12.0);
    let baseline = ((height - font_size) / 2.0).max(1.0);
    let mut content = format!(
        "q 0.9 g 0 0 {width:.3} {height:.3} re f 0 G 1 w 0.5 0.5 {:.3} {:.3} re S BT /Helv {font_size:.3} Tf 0 g 2 {baseline:.3} Td ",
        (width - 1.0).max(0.0),
        (height - 1.0).max(0.0),
    )
    .into_bytes();
    append_pdf_literal(&mut content, caption);
    content.extend_from_slice(b" Tj ET Q");
    let resources = dictionary! {
        "Font" => dictionary! {
            "Helv" => helvetica_dictionary(),
        },
    };
    Ok(appearance_stream(
        document, width, height, resources, content,
    ))
}

fn create_empty_appearance(
    document: &mut Document,
    widget: &Dictionary,
) -> Result<Object, lopdf::Error> {
    let (width, height) = widget_dimensions(document, widget)?;
    let content = format!(
        "q 1 g 0 0 {width:.3} {height:.3} re f 0.5 G 1 w 0.5 0.5 {:.3} {:.3} re S Q",
        (width - 1.0).max(0.0),
        (height - 1.0).max(0.0)
    );
    Ok(appearance_stream(
        document,
        width,
        height,
        Dictionary::new(),
        content.into_bytes(),
    ))
}

fn appearance_stream(
    document: &mut Document,
    width: f32,
    height: f32,
    resources: Dictionary,
    content: Vec<u8>,
) -> Object {
    Object::Reference(document.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), width.into(), height.into()],
            "Resources" => resources,
        },
        content,
    )))
}

fn widget_dimensions(document: &Document, widget: &Dictionary) -> Result<(f32, f32), lopdf::Error> {
    let rectangle = resolved_array(document, widget.get(b"Rect")?)?;
    Ok((
        (rectangle[2].as_float()? - rectangle[0].as_float()?)
            .abs()
            .max(1.0),
        (rectangle[3].as_float()? - rectangle[1].as_float()?)
            .abs()
            .max(1.0),
    ))
}

fn read_or_create_acroform(
    document: &mut Document,
) -> Result<(ObjectId, Dictionary, Vec<Object>), lopdf::Error> {
    let existing = document.catalog()?.get(b"AcroForm").ok().cloned();
    if let Some(existing) = existing {
        let (object_id, resolved) = document.dereference(&existing)?;
        let acroform = resolved.as_dict()?.clone();
        let fields = acroform
            .get(b"Fields")
            .ok()
            .map(|fields| resolved_array(document, fields))
            .transpose()?
            .unwrap_or_default();
        let id = object_id.unwrap_or_else(|| document.new_object_id());
        Ok((id, acroform, fields))
    } else {
        Ok((document.new_object_id(), Dictionary::new(), Vec::new()))
    }
}

fn ensure_default_resources(
    document: &Document,
    acroform: &mut Dictionary,
) -> Result<(), lopdf::Error> {
    let mut resources = acroform
        .get(b"DR")
        .ok()
        .map(|resources| resolved_dictionary(document, resources))
        .transpose()?
        .unwrap_or_default();
    let mut fonts = resources
        .get(b"Font")
        .ok()
        .map(|fonts| resolved_dictionary(document, fonts))
        .transpose()?
        .unwrap_or_default();
    if !fonts.has(b"Helv") {
        fonts.set("Helv", helvetica_dictionary());
    }
    resources.set("Font", fonts);
    acroform.set("DR", resources);
    if !acroform.has(b"DA") {
        acroform.set("DA", Object::string_literal("/Helv 12 Tf 0 g"));
    }
    Ok(())
}

fn helvetica_dictionary() -> Dictionary {
    dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    }
}

fn collect_existing_field_names(document: &Document) -> Result<HashSet<String>, lopdf::Error> {
    let Ok(acroform) = document.catalog()?.get(b"AcroForm") else {
        return Ok(HashSet::new());
    };
    let (_, acroform) = document.dereference(acroform)?;
    let fields = acroform
        .as_dict()?
        .get(b"Fields")
        .ok()
        .map(|fields| resolved_array(document, fields))
        .transpose()?
        .unwrap_or_default();
    let mut names = HashSet::new();
    let mut visited = HashSet::new();
    for field in fields {
        collect_field_name(document, &field, None, &mut visited, &mut names)?;
    }
    Ok(names)
}

fn collect_field_name(
    document: &Document,
    field: &Object,
    parent: Option<&str>,
    visited: &mut HashSet<ObjectId>,
    names: &mut HashSet<String>,
) -> Result<(), lopdf::Error> {
    let (object_id, resolved) = document.dereference(field)?;
    if object_id.is_some_and(|id| !visited.insert(id)) {
        return Ok(());
    }
    let dictionary = resolved.as_dict()?;
    let partial = dictionary
        .get(b"T")
        .ok()
        .and_then(|value| document.dereference(value).ok())
        .and_then(|(_, value)| lopdf::decode_text_string(value).ok());
    let full = match (parent, partial.as_deref()) {
        (Some(parent), Some(partial)) if !parent.is_empty() => Some(format!("{parent}.{partial}")),
        (_, Some(partial)) => Some(partial.to_owned()),
        (Some(parent), None) => Some(parent.to_owned()),
        (None, None) => None,
    };
    if let Some(full) = &full {
        names.insert(full.clone());
    }
    if let Ok(kids) = dictionary.get(b"Kids") {
        for kid in resolved_array(document, kids)? {
            let (_, child) = document.dereference(&kid)?;
            let child = child.as_dict()?;
            if child.has(b"T") || child.has(b"FT") {
                collect_field_name(document, &kid, full.as_deref(), visited, names)?;
            }
        }
    }
    Ok(())
}

fn resolved_array(document: &Document, object: &Object) -> Result<Vec<Object>, lopdf::Error> {
    let (_, resolved) = document.dereference(object)?;
    Ok(resolved.as_array()?.clone())
}

fn resolved_dictionary(document: &Document, object: &Object) -> Result<Dictionary, lopdf::Error> {
    let (_, resolved) = document.dereference(object)?;
    Ok(resolved.as_dict()?.clone())
}

fn parse_field_type(value: &str) -> Option<FieldType> {
    match value.trim().to_ascii_lowercase().as_str() {
        "text" => Some(FieldType::Text),
        "checkbox" => Some(FieldType::Checkbox),
        "radio" => Some(FieldType::Radio),
        "combobox" => Some(FieldType::ComboBox),
        "listbox" => Some(FieldType::ListBox),
        "button" => Some(FieldType::Button),
        "signature" => Some(FieldType::Signature),
        _ => None,
    }
}

fn unique_name(requested: &str, existing: &HashSet<String>) -> String {
    if !existing.contains(requested) {
        return requested.to_owned();
    }
    for index in 1_u64.. {
        let candidate = format!("{requested}_{index}");
        if !existing.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!()
}

fn radio_states(options: &[String], widgets: &[ValidatedWidget]) -> Vec<String> {
    if widgets.iter().all(|widget| widget.export_value.is_some()) {
        widgets
            .iter()
            .filter_map(|widget| widget.export_value.clone())
            .collect()
    } else {
        options.to_vec()
    }
}

fn checkbox_state(options: &[String], widgets: &[ValidatedWidget]) -> String {
    widgets
        .first()
        .and_then(|widget| widget.export_value.clone())
        .or_else(|| options.first().cloned())
        .unwrap_or_else(|| "Yes".to_owned())
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn is_true_value(value: &str) -> bool {
    ["true", "1", "yes", "on", "checked"]
        .iter()
        .any(|candidate| value.eq_ignore_ascii_case(candidate))
}

fn is_false_value(value: &str) -> bool {
    ["false", "0", "off", "unchecked"]
        .iter()
        .any(|candidate| value.eq_ignore_ascii_case(candidate))
}

fn invalid(index: usize, details: impl Into<String>) -> FormCreationError {
    FormCreationError::InvalidDefinition {
        index,
        details: details.into(),
    }
}

fn append_pdf_literal(output: &mut Vec<u8>, value: &str) {
    output.push(b'(');
    for character in value.chars() {
        match character {
            '\\' | '(' | ')' => {
                output.push(b'\\');
                output.push(character as u8);
            }
            '\r' | '\n' => output.push(b' '),
            _ => output.push(u8::try_from(u32::from(character)).unwrap_or(b'?')),
        }
    }
    output.push(b')');
}

#[cfg(test)]
mod tests {
    use std::fs;

    use lopdf::{Document, Object, dictionary};
    use tempfile::tempdir;

    use super::{FormFieldCreation, FormWidgetCreation, create_fields_to_file};

    #[test]
    fn creates_all_field_types_with_flags_names_and_cropbox_coordinates()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?;
        let input = temp.path().join("input.pdf");
        let output = temp.path().join("output.pdf");
        fs::write(&input, source_pdf()?)?;
        let widget = |x| FormWidgetCreation {
            page_index: 0,
            x,
            y: 20.0,
            width: 40.0,
            height: 20.0,
            export_value: None,
        };
        let definition = |name: &str, field_type: &str, tab_order: i64, x| FormFieldCreation {
            name: name.to_owned(),
            field_type: field_type.to_owned(),
            label: Some(format!("{name} label")),
            tooltip: None,
            required: true,
            read_only: true,
            multiline: field_type == "text",
            multi_select: field_type == "listbox",
            options: if matches!(field_type, "combobox" | "listbox") {
                vec!["One".to_owned(), "Two".to_owned()]
            } else if field_type == "radio" {
                vec!["A".to_owned()]
            } else {
                Vec::new()
            },
            default_value: match field_type {
                "text" => Some("hello".to_owned()),
                "checkbox" => Some("true".to_owned()),
                "radio" => Some("A".to_owned()),
                "combobox" | "listbox" => Some("One".to_owned()),
                _ => None,
            },
            font_size: Some(10.0),
            tab_order: Some(tab_order),
            widgets: vec![widget(x)],
        };
        let definitions = [
            definition("existing", "text", 1, 1.0),
            definition("check", "checkbox", 42, 42.0),
            definition("radio", "radio", 83, 83.0),
            definition("combo", "combobox", 124, 124.0),
            definition("list", "listbox", 165, 165.0),
            definition("push", "button", 206, 206.0),
            definition("sig", "signature", 247, 247.0),
        ];
        create_fields_to_file(&input, "input.pdf", &definitions, &output)?;

        let document = Document::load(&output)?;
        let acroform_id = document.catalog()?.get(b"AcroForm")?.as_reference()?;
        let acroform = document.get_dictionary(acroform_id)?;
        assert!(!acroform.get(b"NeedAppearances")?.as_bool()?);
        let fields = acroform.get(b"Fields")?.as_array()?;
        assert_eq!(fields.len(), 8);
        let created = fields
            .iter()
            .skip(1)
            .map(|field| document.get_dictionary(field.as_reference()?))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            lopdf::decode_text_string(created[0].get(b"T")?)?,
            "existing_1"
        );
        assert_eq!(created[0].get(b"Ff")?.as_i64()? & (1 << 12), 1 << 12);
        assert_eq!(created[4].get(b"Ff")?.as_i64()? & (1 << 21), 1 << 21);
        assert_eq!(created[5].get(b"Ff")?.as_i64()? & (1 << 16), 1 << 16);
        for field in created {
            let widget_id = field.get(b"Kids")?.as_array()?[0].as_reference()?;
            assert!(document.get_dictionary(widget_id)?.has(b"AP"));
        }
        let page = document.get_dictionary(document.get_pages()[&1])?;
        assert_eq!(page.get(b"Tabs")?.as_name()?, b"A");
        let first_widget = document.get_dictionary(
            document
                .get_dictionary(fields[1].as_reference()?)?
                .get(b"Kids")?
                .as_array()?[0]
                .as_reference()?,
        )?;
        let rect = first_widget.get(b"Rect")?.as_array()?;
        for (actual, expected) in rect
            .iter()
            .map(Object::as_float)
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .zip([11.0, 750.0, 51.0, 770.0])
        {
            assert!((actual - expected).abs() < f32::EPSILON);
        }
        Ok(())
    }

    #[test]
    fn rejects_an_out_of_bounds_widget_without_writing_output()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?;
        let input = temp.path().join("input.pdf");
        let output = temp.path().join("output.pdf");
        fs::write(&input, source_pdf()?)?;
        let definition = FormFieldCreation {
            name: "bad".to_owned(),
            field_type: "text".to_owned(),
            label: None,
            tooltip: None,
            required: false,
            read_only: false,
            multiline: false,
            multi_select: false,
            options: Vec::new(),
            default_value: None,
            font_size: None,
            tab_order: None,
            widgets: vec![FormWidgetCreation {
                page_index: 0,
                x: 590.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
                export_value: None,
            }],
        };
        assert!(create_fields_to_file(&input, "input.pdf", &[definition], &output).is_err());
        assert!(!output.exists());
        Ok(())
    }

    fn source_pdf() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut document = Document::with_version("1.7");
        let page_tree_id = document.new_object_id();
        let leaf_page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => page_tree_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "CropBox" => vec![10.into(), 20.into(), 610.into(), 790.into()],
        });
        document.objects.insert(
            page_tree_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(leaf_page_id)],
                "Count" => 1,
            }),
        );
        let existing_id = document.add_object(dictionary! {
            "FT" => "Tx",
            "T" => Object::string_literal("existing"),
        });
        let acroform_id = document.add_object(dictionary! {
            "Fields" => vec![Object::Reference(existing_id)],
        });
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => page_tree_id,
            "AcroForm" => acroform_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes)?;
        Ok(bytes)
    }
}
