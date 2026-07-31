use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use lopdf::{Dictionary, Document, Object, ObjectId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{pdf_form_mutation::pdf_text_string, pdf_page_geometry::inherited_value};

const MAX_TREE_DEPTH: usize = 256;
const MAX_TREE_ITEMS: usize = 100_000;
const MAX_STRUCTURE_PREVIEW: usize = 1_000;
const ANNOTATION_INVISIBLE: i64 = 1;
const ANNOTATION_HIDDEN: i64 = 1 << 1;

#[derive(Debug, Error)]
pub enum AccessibilityError {
    #[error("could not read PDF '{filename}': {source}")]
    ReadPdf {
        filename: String,
        #[source]
        source: lopdf::Error,
    },
    #[error("encrypted PDFs must be unlocked before accessibility inspection")]
    Encrypted,
    #[error("invalid accessibility repair: {0}")]
    InvalidRepair(String),
    #[error("PDF structure error: {0}")]
    Pdf(#[from] lopdf::Error),
    #[error("could not write remediated PDF: {0}")]
    Write(#[source] std::io::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingStatus {
    Pass,
    Fail,
    Manual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemediationKind {
    Automatic,
    UserInput,
    Manual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessibilityFinding {
    pub rule_id: String,
    pub status: FindingStatus,
    pub severity: FindingSeverity,
    pub scope: String,
    pub title: String,
    pub message: String,
    pub remediation: RemediationKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructurePreview {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<u16>,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alternative_text: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessibilityDocumentSummary {
    pub page_count: usize,
    pub language: Option<String>,
    pub has_structure_tree: bool,
    pub marked: bool,
    pub figure_count: usize,
    pub form_field_count: usize,
    pub structure_preview_truncated: bool,
    pub structure_order: Vec<StructurePreview>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessibilitySummary {
    pub passed: usize,
    pub failed: usize,
    pub manual_review: usize,
    pub total: usize,
    pub remediable: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessibilityReport {
    pub schema_version: u8,
    pub summary: AccessibilitySummary,
    pub document: AccessibilityDocumentSummary,
    pub findings: Vec<AccessibilityFinding>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessibilityRepairs {
    pub document_language: Option<String>,
    pub mark_as_tagged: Option<bool>,
    #[serde(default)]
    pub structure_tab_order_pages: Vec<usize>,
    #[serde(default)]
    pub alternative_texts: Vec<AlternativeTextRepair>,
    #[serde(default)]
    pub form_field_tooltips: Vec<FormTooltipRepair>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlternativeTextRepair {
    pub object_number: u32,
    pub generation: u16,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormTooltipRepair {
    pub field_name: String,
    pub text: String,
}

#[derive(Debug)]
struct Inspection {
    report: AccessibilityReport,
    pages: Vec<ObjectId>,
    figures: HashSet<ObjectId>,
    fields: HashMap<String, Vec<ObjectId>>,
}

#[derive(Debug, Default)]
struct StructureInspection {
    order: Vec<StructurePreview>,
    figures: HashSet<ObjectId>,
    figure_count: usize,
    missing_alternatives: Vec<(Option<ObjectId>, Option<usize>)>,
    visited: HashSet<ObjectId>,
    item_count: usize,
    preview_truncated: bool,
}

#[derive(Debug, Default)]
struct FormInspection {
    count: usize,
    missing_labels: Vec<(Option<String>, Option<ObjectId>)>,
    fields: HashMap<String, Vec<ObjectId>>,
    visited: HashSet<ObjectId>,
    item_count: usize,
}

/// Inspects the native, machine-checkable accessibility rules.
///
/// A successful result is a rule report, not a PDF/UA conformance claim.
///
/// # Errors
///
/// Returns [`AccessibilityError`] when the PDF cannot be read or traversed.
pub fn check_accessibility(
    input_path: &Path,
    filename: &str,
) -> Result<AccessibilityReport, AccessibilityError> {
    let document = load_document(input_path, filename)?;
    Ok(inspect_document(&document)?.report)
}

/// Applies only the bounded repairs authorized by [`AccessibilityRepairs`].
///
/// Every target is validated before the document is mutated.
///
/// # Errors
///
/// Returns [`AccessibilityError`] for malformed input, an invalid repair
/// target/value, or a write failure.
pub fn remediate_accessibility_to_file(
    input_path: &Path,
    filename: &str,
    repairs: &AccessibilityRepairs,
    output_path: &Path,
) -> Result<(), AccessibilityError> {
    let mut document = load_document(input_path, filename)?;
    let inspection = inspect_document(&document)?;
    let validated = validate_repairs(repairs, &inspection)?;

    if let Some(language) = validated.document_language {
        document
            .catalog_mut()?
            .set("Lang", pdf_text_string(&language));
    }
    if validated.mark_as_tagged {
        set_marked(&mut document)?;
    }
    for page_index in validated.structure_tab_order_pages {
        document
            .get_dictionary_mut(inspection.pages[page_index])?
            .set("Tabs", Object::Name(b"S".to_vec()));
    }
    for (object_id, text) in validated.alternative_texts {
        document
            .get_dictionary_mut(object_id)?
            .set("Alt", pdf_text_string(&text));
    }
    for (object_id, text) in validated.form_field_tooltips {
        document
            .get_dictionary_mut(object_id)?
            .set("TU", pdf_text_string(&text));
    }

    document
        .save(output_path)
        .map_err(AccessibilityError::Write)?;
    Ok(())
}

fn load_document(input_path: &Path, filename: &str) -> Result<Document, AccessibilityError> {
    let document = Document::load(input_path).map_err(|source| AccessibilityError::ReadPdf {
        filename: filename.to_owned(),
        source,
    })?;
    if document.is_encrypted() {
        return Err(AccessibilityError::Encrypted);
    }
    Ok(document)
}

#[allow(clippy::too_many_lines)]
fn inspect_document(document: &Document) -> Result<Inspection, AccessibilityError> {
    let page_entries: Vec<(u32, ObjectId)> = document.get_pages().into_iter().collect();
    let pages: Vec<ObjectId> = page_entries.iter().map(|(_, id)| *id).collect();
    let page_indexes: HashMap<ObjectId, usize> = page_entries
        .iter()
        .enumerate()
        .map(|(index, (_, id))| (*id, index))
        .collect();
    let catalog = document.catalog()?;
    let language = dictionary_text(document, catalog, b"Lang");
    let language_valid = language.as_deref().is_some_and(is_language_tag);
    let structure_root = catalog
        .get(b"StructTreeRoot")
        .ok()
        .and_then(|value| resolved_dictionary(document, value));
    let has_structure_tree = structure_root.is_some();
    let marked = catalog
        .get(b"MarkInfo")
        .ok()
        .and_then(|value| resolved_dictionary(document, value))
        .and_then(|dictionary| dictionary.get(b"Marked").ok())
        .and_then(|value| resolved_bool(document, value))
        .unwrap_or(false);

    let mut structure = StructureInspection::default();
    if let Some(root) = structure_root {
        let role_map = root
            .get(b"RoleMap")
            .ok()
            .and_then(|value| resolved_dictionary(document, value))
            .cloned()
            .unwrap_or_default();
        if let Ok(kids) = root.get(b"K") {
            walk_structure(
                document,
                kids,
                &role_map,
                &page_indexes,
                None,
                0,
                &mut structure,
            )?;
        }
    }

    let mut forms = FormInspection::default();
    inspect_forms(document, &mut forms)?;

    let mut findings = Vec::new();
    findings.push(simple_finding(
        "structure.tree",
        if has_structure_tree {
            FindingStatus::Pass
        } else {
            FindingStatus::Fail
        },
        if has_structure_tree {
            FindingSeverity::Info
        } else {
            FindingSeverity::Error
        },
        "document",
        "Tagged structure tree",
        if has_structure_tree {
            "The catalog contains a structure tree."
        } else {
            "The catalog has no structure tree; use a full tagging workflow before manual reading-order review."
        },
        RemediationKind::Manual,
    ));
    findings.push(simple_finding(
        "structure.marked",
        if marked {
            FindingStatus::Pass
        } else {
            FindingStatus::Fail
        },
        if marked {
            FindingSeverity::Info
        } else {
            FindingSeverity::Error
        },
        "document",
        "Document marked as tagged",
        if marked {
            "The catalog MarkInfo dictionary declares Marked true."
        } else if has_structure_tree {
            "A structure tree exists, but the catalog is not marked as tagged."
        } else {
            "The catalog is not marked as tagged, and no structure tree exists."
        },
        if has_structure_tree {
            RemediationKind::Automatic
        } else {
            RemediationKind::Manual
        },
    ));
    findings.push(simple_finding(
        "reading-order.logical",
        FindingStatus::Manual,
        FindingSeverity::Warning,
        "document",
        "Logical reading order",
        if has_structure_tree {
            "Review the ordered structure preview against the visible document; semantic reading order cannot be proven automatically."
        } else {
            "Reading order cannot be reviewed until the document has a structure tree."
        },
        RemediationKind::Manual,
    ));

    add_tab_order_findings(document, &page_entries, &mut findings)?;
    findings.push(simple_finding(
        "document.language",
        if language_valid {
            FindingStatus::Pass
        } else {
            FindingStatus::Fail
        },
        if language_valid {
            FindingSeverity::Info
        } else {
            FindingSeverity::Error
        },
        "document",
        "Default document language",
        if language_valid {
            "The catalog contains a nonblank language tag."
        } else if language.is_some() {
            "The catalog Lang value is not a supported language-tag shape."
        } else {
            "The catalog has no default Lang value."
        },
        RemediationKind::Automatic,
    ));

    if structure.missing_alternatives.is_empty() {
        findings.push(simple_finding(
            "figure.alternative-text",
            FindingStatus::Pass,
            FindingSeverity::Info,
            "structure",
            "Figure alternative text",
            if structure.figure_count == 0 {
                "No Figure structure elements were found."
            } else {
                "Every Figure structure element has Alt or ActualText."
            },
            RemediationKind::UserInput,
        ));
    } else {
        for (object_id, page_index) in &structure.missing_alternatives {
            findings.push(targeted_finding(
                "figure.alternative-text",
                "structure",
                "Figure alternative text",
                "A Figure structure element is missing nonblank Alt or ActualText.",
                *object_id,
                *page_index,
                None,
                if object_id.is_some() {
                    RemediationKind::UserInput
                } else {
                    RemediationKind::Manual
                },
            ));
        }
    }

    if forms.missing_labels.is_empty() {
        findings.push(simple_finding(
            "form-field.accessible-name",
            FindingStatus::Pass,
            FindingSeverity::Info,
            "formField",
            "Accessible form-field names",
            if forms.count == 0 {
                "No terminal AcroForm fields were found."
            } else {
                "Every terminal AcroForm field has a nonblank TU accessible name."
            },
            RemediationKind::UserInput,
        ));
    } else {
        for (field_name, object_id) in &forms.missing_labels {
            let message = field_name.as_ref().map_or(
                "An unnamed terminal form field has no TU accessible name.".to_owned(),
                |name| format!("Form field '{name}' has no nonblank TU accessible name."),
            );
            findings.push(targeted_finding(
                "form-field.accessible-name",
                "formField",
                "Accessible form-field name",
                &message,
                *object_id,
                None,
                field_name.clone(),
                if field_name.is_some() && object_id.is_some() {
                    RemediationKind::UserInput
                } else {
                    RemediationKind::Manual
                },
            ));
        }
    }

    if structure.preview_truncated {
        findings.push(simple_finding(
            "structure.preview-limit",
            FindingStatus::Manual,
            FindingSeverity::Warning,
            "structure",
            "Structure preview truncated",
            "The ordered structure preview reached 1,000 rows; use a dedicated tagging tool to review the complete tree.",
            RemediationKind::Manual,
        ));
    }

    let passed = findings
        .iter()
        .filter(|finding| finding.status == FindingStatus::Pass)
        .count();
    let failed = findings
        .iter()
        .filter(|finding| finding.status == FindingStatus::Fail)
        .count();
    let manual_review = findings
        .iter()
        .filter(|finding| finding.status == FindingStatus::Manual)
        .count();
    let remediable = findings
        .iter()
        .filter(|finding| {
            finding.status == FindingStatus::Fail && finding.remediation != RemediationKind::Manual
        })
        .count();
    let report = AccessibilityReport {
        schema_version: 1,
        summary: AccessibilitySummary {
            passed,
            failed,
            manual_review,
            total: findings.len(),
            remediable,
        },
        document: AccessibilityDocumentSummary {
            page_count: pages.len(),
            language,
            has_structure_tree,
            marked,
            figure_count: structure.figure_count,
            form_field_count: forms.count,
            structure_preview_truncated: structure.preview_truncated,
            structure_order: structure.order,
        },
        findings,
    };
    Ok(Inspection {
        report,
        pages,
        figures: structure.figures,
        fields: forms.fields,
    })
}

#[allow(clippy::too_many_arguments)]
fn walk_structure(
    document: &Document,
    object: &Object,
    role_map: &Dictionary,
    page_indexes: &HashMap<ObjectId, usize>,
    inherited_page: Option<ObjectId>,
    depth: usize,
    inspection: &mut StructureInspection,
) -> Result<(), AccessibilityError> {
    if depth > MAX_TREE_DEPTH || inspection.item_count >= MAX_TREE_ITEMS {
        inspection.preview_truncated = true;
        return Ok(());
    }
    let (object_id, resolved) = document.dereference(object)?;
    if object_id.is_some_and(|id| !inspection.visited.insert(id)) {
        return Ok(());
    }
    if let Ok(array) = resolved.as_array() {
        for child in array {
            walk_structure(
                document,
                child,
                role_map,
                page_indexes,
                inherited_page,
                depth + 1,
                inspection,
            )?;
        }
        return Ok(());
    }
    let Ok(dictionary) = resolved.as_dict() else {
        return Ok(());
    };
    let Some(role) = dictionary_name(document, dictionary, b"S") else {
        return Ok(());
    };
    inspection.item_count += 1;
    let effective_role = resolve_role(document, role_map, &role);
    let page_id = dictionary
        .get(b"Pg")
        .ok()
        .and_then(|value| document.dereference(value).ok())
        .and_then(|(id, _)| id)
        .or(inherited_page);
    let page_index = page_id.and_then(|id| page_indexes.get(&id).copied());
    let alternative_text = dictionary_text(document, dictionary, b"Alt")
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            dictionary_text(document, dictionary, b"ActualText")
                .filter(|text| !text.trim().is_empty())
        });
    if inspection.order.len() < MAX_STRUCTURE_PREVIEW {
        inspection.order.push(StructurePreview {
            object_number: object_id.map(|id| id.0),
            generation: object_id.map(|id| id.1),
            role: effective_role.clone(),
            page_index,
            alternative_text: alternative_text.clone(),
        });
    } else {
        inspection.preview_truncated = true;
    }
    if effective_role == "Figure" {
        inspection.figure_count += 1;
        if let Some(id) = object_id {
            inspection.figures.insert(id);
        }
        if alternative_text.is_none() {
            inspection
                .missing_alternatives
                .push((object_id, page_index));
        }
    }
    if let Ok(kids) = dictionary.get(b"K") {
        walk_structure(
            document,
            kids,
            role_map,
            page_indexes,
            page_id,
            depth + 1,
            inspection,
        )?;
    }
    Ok(())
}

fn resolve_role(document: &Document, role_map: &Dictionary, role: &str) -> String {
    let mut current = role.to_owned();
    let mut visited = HashSet::new();
    for _ in 0..32 {
        if !visited.insert(current.clone()) {
            break;
        }
        let Some(next) = role_map
            .get(current.as_bytes())
            .ok()
            .and_then(|value| object_name(document, value))
        else {
            break;
        };
        current = next;
    }
    current
}

fn inspect_forms(
    document: &Document,
    inspection: &mut FormInspection,
) -> Result<(), AccessibilityError> {
    let Ok(acroform) = document.catalog()?.get(b"AcroForm") else {
        return Ok(());
    };
    let Some(acroform) = resolved_dictionary(document, acroform) else {
        return Ok(());
    };
    let Some(fields) = acroform
        .get(b"Fields")
        .ok()
        .and_then(|value| resolved_array(document, value))
    else {
        return Ok(());
    };
    for field in fields {
        walk_form_field(document, field, None, None, 0, inspection)?;
    }
    Ok(())
}

fn walk_form_field(
    document: &Document,
    object: &Object,
    parent_name: Option<&str>,
    inherited_tooltip: Option<&str>,
    depth: usize,
    inspection: &mut FormInspection,
) -> Result<(), AccessibilityError> {
    if depth > MAX_TREE_DEPTH || inspection.item_count >= MAX_TREE_ITEMS {
        return Ok(());
    }
    let (object_id, resolved) = document.dereference(object)?;
    if object_id.is_some_and(|id| !inspection.visited.insert(id)) {
        return Ok(());
    }
    let dictionary = resolved.as_dict()?;
    inspection.item_count += 1;
    let partial_name = dictionary_text(document, dictionary, b"T");
    let full_name = qualified_name(parent_name, partial_name.as_deref());
    let tooltip = dictionary_text(document, dictionary, b"TU")
        .filter(|value| !value.trim().is_empty())
        .or_else(|| inherited_tooltip.map(ToOwned::to_owned));
    let kids = dictionary
        .get(b"Kids")
        .ok()
        .and_then(|value| resolved_array(document, value))
        .map_or(&[] as &[Object], Vec::as_slice);
    let field_kids: Vec<&Object> = kids
        .iter()
        .filter(|kid| is_field_child(document, kid))
        .collect();
    if !field_kids.is_empty() {
        for child in field_kids {
            walk_form_field(
                document,
                child,
                full_name.as_deref(),
                tooltip.as_deref(),
                depth + 1,
                inspection,
            )?;
        }
        return Ok(());
    }

    inspection.count += 1;
    if let (Some(name), Some(id)) = (full_name.as_ref(), object_id) {
        inspection.fields.entry(name.clone()).or_default().push(id);
    }
    if tooltip.is_none() {
        inspection.missing_labels.push((full_name, object_id));
    }
    Ok(())
}

fn is_field_child(document: &Document, object: &Object) -> bool {
    let Ok((_, resolved)) = document.dereference(object) else {
        return false;
    };
    let Ok(dictionary) = resolved.as_dict() else {
        return false;
    };
    let is_widget = dictionary_name(document, dictionary, b"Subtype").as_deref() == Some("Widget");
    !is_widget || dictionary.has(b"T") || dictionary.has(b"FT") || dictionary.has(b"Kids")
}

fn add_tab_order_findings(
    document: &Document,
    pages: &[(u32, ObjectId)],
    findings: &mut Vec<AccessibilityFinding>,
) -> Result<(), AccessibilityError> {
    let mut relevant_pages = 0usize;
    for (page_index, (_, page_id)) in pages.iter().enumerate() {
        if !page_has_visible_annotations(document, *page_id)? {
            continue;
        }
        relevant_pages += 1;
        let structured = document
            .get_dictionary(*page_id)?
            .get(b"Tabs")
            .ok()
            .and_then(|value| object_name(document, value))
            .as_deref()
            == Some("S");
        if structured {
            continue;
        }
        let mut finding = simple_finding(
            "reading-order.annotation-tabs",
            FindingStatus::Fail,
            FindingSeverity::Error,
            "page",
            "Annotation tab order",
            "This page contains a visible annotation but does not use structure order (/Tabs /S).",
            RemediationKind::Automatic,
        );
        finding.page_index = Some(page_index);
        findings.push(finding);
    }
    if relevant_pages == 0
        || !findings
            .iter()
            .any(|finding| finding.rule_id == "reading-order.annotation-tabs")
    {
        findings.push(simple_finding(
            "reading-order.annotation-tabs",
            FindingStatus::Pass,
            FindingSeverity::Info,
            "page",
            "Annotation tab order",
            if relevant_pages == 0 {
                "No pages with visible annotations were found."
            } else {
                "Every page with visible annotations uses structure order (/Tabs /S)."
            },
            RemediationKind::Automatic,
        ));
    }
    Ok(())
}

fn page_has_visible_annotations(
    document: &Document,
    page_id: ObjectId,
) -> Result<bool, AccessibilityError> {
    let page = document.get_dictionary(page_id)?;
    let Some(annotations) = page
        .get(b"Annots")
        .ok()
        .and_then(|value| resolved_array(document, value))
    else {
        return Ok(false);
    };
    let crop = page_box(document, page_id);
    for annotation in annotations {
        let Some(annotation) = resolved_dictionary(document, annotation) else {
            continue;
        };
        if dictionary_name(document, annotation, b"Subtype").as_deref() == Some("Popup") {
            continue;
        }
        let flags = annotation
            .get(b"F")
            .ok()
            .and_then(|value| resolved_integer(document, value))
            .unwrap_or_default();
        if flags & (ANNOTATION_INVISIBLE | ANNOTATION_HIDDEN) != 0 {
            continue;
        }
        if let (Some(crop), Some(rect)) =
            (crop, dictionary_number_array(document, annotation, b"Rect"))
            && !rectangles_intersect(crop, rect)
        {
            continue;
        }
        return Ok(true);
    }
    Ok(false)
}

fn page_box(document: &Document, page_id: ObjectId) -> Option<[f32; 4]> {
    inherited_value(document, page_id, b"CropBox")
        .or_else(|_| inherited_value(document, page_id, b"MediaBox"))
        .ok()
        .and_then(|value| object_number_array(document, &value))
}

fn rectangles_intersect(left: [f32; 4], right: [f32; 4]) -> bool {
    let first = normalized_rectangle(left);
    let second = normalized_rectangle(right);
    second[2] > first[0] && second[0] < first[2] && second[3] > first[1] && second[1] < first[3]
}

fn normalized_rectangle(rectangle: [f32; 4]) -> [f32; 4] {
    [
        rectangle[0].min(rectangle[2]),
        rectangle[1].min(rectangle[3]),
        rectangle[0].max(rectangle[2]),
        rectangle[1].max(rectangle[3]),
    ]
}

struct ValidatedRepairs {
    document_language: Option<String>,
    mark_as_tagged: bool,
    structure_tab_order_pages: Vec<usize>,
    alternative_texts: Vec<(ObjectId, String)>,
    form_field_tooltips: Vec<(ObjectId, String)>,
}

#[allow(clippy::too_many_lines)]
fn validate_repairs(
    repairs: &AccessibilityRepairs,
    inspection: &Inspection,
) -> Result<ValidatedRepairs, AccessibilityError> {
    let document_language = repairs
        .document_language
        .as_deref()
        .map(str::trim)
        .map(ToOwned::to_owned);
    if document_language
        .as_deref()
        .is_some_and(|language| !is_language_tag(language))
    {
        return Err(invalid_repair(
            "documentLanguage must use a supported language-tag shape",
        ));
    }
    let mark_as_tagged = repairs.mark_as_tagged.unwrap_or(false);
    if repairs.mark_as_tagged == Some(false) {
        return Err(invalid_repair("markAsTagged may only be true"));
    }
    if mark_as_tagged && !inspection.report.document.has_structure_tree {
        return Err(invalid_repair(
            "markAsTagged requires an existing structure tree",
        ));
    }

    let mut page_indexes = HashSet::new();
    for &page_index in &repairs.structure_tab_order_pages {
        if page_index >= inspection.pages.len() {
            return Err(invalid_repair(format!(
                "structureTabOrderPages contains out-of-range page index {page_index}"
            )));
        }
        if !page_indexes.insert(page_index) {
            return Err(invalid_repair(format!(
                "structureTabOrderPages contains duplicate page index {page_index}"
            )));
        }
    }

    let mut alternative_ids = HashSet::new();
    let mut alternative_texts = Vec::new();
    for repair in &repairs.alternative_texts {
        let object_id = (repair.object_number, repair.generation);
        if !alternative_ids.insert(object_id) {
            return Err(invalid_repair(format!(
                "duplicate alternative-text target {} {}",
                object_id.0, object_id.1
            )));
        }
        if !inspection.figures.contains(&object_id) {
            return Err(invalid_repair(format!(
                "object {} {} is not an existing Figure structure element",
                object_id.0, object_id.1
            )));
        }
        let text = repair.text.trim();
        if text.is_empty() {
            return Err(invalid_repair("alternative text must not be blank"));
        }
        alternative_texts.push((object_id, text.to_owned()));
    }

    let mut field_names = HashSet::new();
    let mut form_field_tooltips = Vec::new();
    for repair in &repairs.form_field_tooltips {
        let field_name = repair.field_name.trim();
        if field_name.is_empty() {
            return Err(invalid_repair("form field name must not be blank"));
        }
        if !field_names.insert(field_name.to_owned()) {
            return Err(invalid_repair(format!(
                "duplicate form-field target '{field_name}'"
            )));
        }
        let Some(targets) = inspection.fields.get(field_name) else {
            return Err(invalid_repair(format!(
                "form field '{field_name}' does not exist"
            )));
        };
        if targets.len() != 1 {
            return Err(invalid_repair(format!(
                "form field '{field_name}' does not resolve to exactly one indirect field object"
            )));
        }
        let text = repair.text.trim();
        if text.is_empty() {
            return Err(invalid_repair("form field tooltip must not be blank"));
        }
        form_field_tooltips.push((targets[0], text.to_owned()));
    }

    if document_language.is_none()
        && !mark_as_tagged
        && page_indexes.is_empty()
        && alternative_texts.is_empty()
        && form_field_tooltips.is_empty()
    {
        return Err(invalid_repair("at least one repair is required"));
    }
    let mut structure_tab_order_pages: Vec<usize> = page_indexes.into_iter().collect();
    structure_tab_order_pages.sort_unstable();
    Ok(ValidatedRepairs {
        document_language,
        mark_as_tagged,
        structure_tab_order_pages,
        alternative_texts,
        form_field_tooltips,
    })
}

fn set_marked(document: &mut Document) -> Result<(), AccessibilityError> {
    let mut mark_info = document
        .catalog()?
        .get(b"MarkInfo")
        .ok()
        .and_then(|value| resolved_dictionary(document, value))
        .cloned()
        .unwrap_or_default();
    mark_info.set("Marked", true);
    document
        .catalog_mut()?
        .set("MarkInfo", Object::Dictionary(mark_info));
    Ok(())
}

fn invalid_repair(message: impl Into<String>) -> AccessibilityError {
    AccessibilityError::InvalidRepair(message.into())
}

fn is_language_tag(value: &str) -> bool {
    let mut subtags = value.split('-');
    let Some(primary) = subtags.next() else {
        return false;
    };
    if !(2..=8).contains(&primary.len()) || !primary.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return false;
    }
    subtags.all(|subtag| {
        !subtag.is_empty()
            && subtag.len() <= 8
            && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

fn simple_finding(
    rule_id: &str,
    status: FindingStatus,
    severity: FindingSeverity,
    scope: &str,
    title: &str,
    message: &str,
    remediation: RemediationKind,
) -> AccessibilityFinding {
    AccessibilityFinding {
        rule_id: rule_id.to_owned(),
        status,
        severity,
        scope: scope.to_owned(),
        title: title.to_owned(),
        message: message.to_owned(),
        remediation,
        page_index: None,
        object_number: None,
        generation: None,
        field_name: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn targeted_finding(
    rule_id: &str,
    scope: &str,
    title: &str,
    message: &str,
    object_id: Option<ObjectId>,
    page_index: Option<usize>,
    field_name: Option<String>,
    remediation: RemediationKind,
) -> AccessibilityFinding {
    AccessibilityFinding {
        rule_id: rule_id.to_owned(),
        status: FindingStatus::Fail,
        severity: FindingSeverity::Error,
        scope: scope.to_owned(),
        title: title.to_owned(),
        message: message.to_owned(),
        remediation,
        page_index,
        object_number: object_id.map(|id| id.0),
        generation: object_id.map(|id| id.1),
        field_name,
    }
}

fn qualified_name(parent: Option<&str>, partial: Option<&str>) -> Option<String> {
    match (
        parent.filter(|value| !value.is_empty()),
        partial.filter(|value| !value.is_empty()),
    ) {
        (Some(parent), Some(partial)) => Some(format!("{parent}.{partial}")),
        (Some(parent), None) => Some(parent.to_owned()),
        (None, Some(partial)) => Some(partial.to_owned()),
        (None, None) => None,
    }
}

fn dictionary_text(document: &Document, dictionary: &Dictionary, key: &[u8]) -> Option<String> {
    dictionary
        .get(key)
        .ok()
        .and_then(|value| object_text(document, value))
}

fn object_text(document: &Document, object: &Object) -> Option<String> {
    let (_, resolved) = document.dereference(object).ok()?;
    lopdf::decode_text_string(resolved).ok().or_else(|| {
        resolved
            .as_name()
            .ok()
            .map(|name| String::from_utf8_lossy(name).into_owned())
    })
}

fn dictionary_name(document: &Document, dictionary: &Dictionary, key: &[u8]) -> Option<String> {
    dictionary
        .get(key)
        .ok()
        .and_then(|value| object_name(document, value))
}

fn object_name(document: &Document, object: &Object) -> Option<String> {
    let (_, resolved) = document.dereference(object).ok()?;
    resolved
        .as_name()
        .ok()
        .map(|name| String::from_utf8_lossy(name).into_owned())
}

fn resolved_bool(document: &Document, object: &Object) -> Option<bool> {
    let (_, resolved) = document.dereference(object).ok()?;
    resolved.as_bool().ok()
}

fn resolved_integer(document: &Document, object: &Object) -> Option<i64> {
    let (_, resolved) = document.dereference(object).ok()?;
    resolved.as_i64().ok()
}

fn resolved_dictionary<'a>(document: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    let (_, resolved) = document.dereference(object).ok()?;
    resolved.as_dict().ok()
}

fn resolved_array<'a>(document: &'a Document, object: &'a Object) -> Option<&'a Vec<Object>> {
    let (_, resolved) = document.dereference(object).ok()?;
    resolved.as_array().ok()
}

fn dictionary_number_array(
    document: &Document,
    dictionary: &Dictionary,
    key: &[u8],
) -> Option<[f32; 4]> {
    dictionary
        .get(key)
        .ok()
        .and_then(|object| object_number_array(document, object))
}

fn object_number_array(document: &Document, object: &Object) -> Option<[f32; 4]> {
    let (_, resolved) = document.dereference(object).ok()?;
    let array = resolved.as_array().ok()?;
    let values: Vec<f32> = array
        .iter()
        .take(4)
        .filter_map(|value| {
            document
                .dereference(value)
                .ok()
                .and_then(|(_, value)| value.as_float().ok())
        })
        .collect();
    (values.len() == 4).then(|| [values[0], values[1], values[2], values[3]])
}

#[cfg(test)]
mod tests {
    use lopdf::{Document, Object, Stream, dictionary};
    use tempfile::tempdir;

    use super::{
        AccessibilityError, AccessibilityRepairs, AlternativeTextRepair, FindingStatus,
        FormTooltipRepair, check_accessibility, remediate_accessibility_to_file,
    };

    fn write_accessibility_fixture(
        path: &Path,
        tagged: bool,
    ) -> Result<(u32, u16), Box<dyn std::error::Error>> {
        let mut document = Document::with_version("1.7");
        let page_tree_id = document.new_object_id();
        let leaf_page_id = document.new_object_id();
        let content_id = document.add_object(Stream::new(dictionary! {}, Vec::new()));
        let field_id = document.new_object_id();
        let widget_id = document.new_object_id();
        let figure_id = document.new_object_id();
        let paragraph_id = document.new_object_id();

        document.objects.insert(
            widget_id,
            Object::Dictionary(dictionary! {
                "Type" => "Annot",
                "Subtype" => "Widget",
                "Parent" => field_id,
                "P" => leaf_page_id,
                "Rect" => vec![10.into(), 10.into(), 110.into(), 30.into()],
            }),
        );
        document.objects.insert(
            field_id,
            Object::Dictionary(dictionary! {
                "FT" => "Tx",
                "T" => Object::string_literal("customer.name"),
                "Kids" => vec![widget_id.into()],
            }),
        );
        document.objects.insert(
            leaf_page_id,
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Parent" => page_tree_id,
                "MediaBox" => vec![0.into(), 0.into(), 300.into(), 400.into()],
                "Contents" => content_id,
                "Resources" => dictionary! {},
                "Annots" => vec![widget_id.into()],
                "Tabs" => "A",
            }),
        );
        document.objects.insert(
            page_tree_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![leaf_page_id.into()],
                "Count" => 1,
            }),
        );
        document.objects.insert(
            figure_id,
            Object::Dictionary(dictionary! {
                "Type" => "StructElem",
                "S" => "Illustration",
                "Pg" => leaf_page_id,
                "K" => 0,
            }),
        );
        document.objects.insert(
            paragraph_id,
            Object::Dictionary(dictionary! {
                "Type" => "StructElem",
                "S" => "P",
                "Pg" => leaf_page_id,
                "K" => 1,
            }),
        );

        let mut catalog = dictionary! {
            "Type" => "Catalog",
            "Pages" => page_tree_id,
            "AcroForm" => dictionary! { "Fields" => vec![field_id.into()] },
        };
        if tagged {
            let structure_root_id = document.add_object(dictionary! {
                "Type" => "StructTreeRoot",
                "K" => vec![figure_id.into(), paragraph_id.into()],
                "RoleMap" => dictionary! { "Illustration" => "Figure" },
            });
            catalog.set("StructTreeRoot", structure_root_id);
        }
        let catalog_id = document.add_object(catalog);
        document.trailer.set("Root", catalog_id);
        document.compress();
        document.save(path)?;
        Ok(figure_id)
    }

    use std::path::Path;

    #[test]
    fn reports_machine_findings_and_ordered_structure_preview()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let input = directory.path().join("input.pdf");
        let figure_id = write_accessibility_fixture(&input, true)?;
        let report = check_accessibility(&input, "input.pdf")?;

        assert_eq!(report.schema_version, 1);
        assert_eq!(report.document.page_count, 1);
        assert_eq!(report.document.figure_count, 1);
        assert_eq!(report.document.form_field_count, 1);
        assert_eq!(
            report
                .document
                .structure_order
                .iter()
                .map(|entry| entry.role.as_str())
                .collect::<Vec<_>>(),
            ["Figure", "P"]
        );
        let figure = report
            .findings
            .iter()
            .find(|finding| finding.rule_id == "figure.alternative-text")
            .ok_or("missing figure finding")?;
        assert_eq!(figure.status, FindingStatus::Fail);
        assert_eq!(figure.object_number, Some(figure_id.0));
        assert!(report.summary.failed >= 5);
        assert_eq!(report.summary.manual_review, 1);
        Ok(())
    }

    #[test]
    fn applies_atomic_repairs_and_checker_proves_the_result()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let input = directory.path().join("input.pdf");
        let output = directory.path().join("output.pdf");
        let figure_id = write_accessibility_fixture(&input, true)?;
        let repairs = AccessibilityRepairs {
            document_language: Some("en-US".to_owned()),
            mark_as_tagged: Some(true),
            structure_tab_order_pages: vec![0],
            alternative_texts: vec![AlternativeTextRepair {
                object_number: figure_id.0,
                generation: figure_id.1,
                text: "Quarterly sales chart".to_owned(),
            }],
            form_field_tooltips: vec![FormTooltipRepair {
                field_name: "customer.name".to_owned(),
                text: "Customer name".to_owned(),
            }],
        };
        remediate_accessibility_to_file(&input, "input.pdf", &repairs, &output)?;
        let report = check_accessibility(&output, "output.pdf")?;

        assert_eq!(report.summary.failed, 0);
        assert_eq!(report.summary.manual_review, 1);
        assert_eq!(report.document.language.as_deref(), Some("en-US"));
        assert!(report.document.marked);
        assert_eq!(
            report.document.structure_order[0]
                .alternative_text
                .as_deref(),
            Some("Quarterly sales chart")
        );
        Ok(())
    }

    #[test]
    fn rejects_all_repairs_before_writing_when_any_target_is_invalid()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let input = directory.path().join("input.pdf");
        let output = directory.path().join("output.pdf");
        write_accessibility_fixture(&input, true)?;
        let repairs = AccessibilityRepairs {
            document_language: Some("en-US".to_owned()),
            mark_as_tagged: None,
            structure_tab_order_pages: vec![],
            alternative_texts: vec![AlternativeTextRepair {
                object_number: 999_999,
                generation: 0,
                text: "Description".to_owned(),
            }],
            form_field_tooltips: vec![],
        };

        let Err(error) = remediate_accessibility_to_file(&input, "input.pdf", &repairs, &output)
        else {
            return Err("invalid repair unexpectedly succeeded".into());
        };
        assert!(matches!(error, AccessibilityError::InvalidRepair(_)));
        assert!(!output.exists());
        Ok(())
    }

    #[test]
    fn refuses_to_mark_an_untagged_document_as_tagged() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempdir()?;
        let input = directory.path().join("input.pdf");
        let output = directory.path().join("output.pdf");
        write_accessibility_fixture(&input, false)?;
        let repairs = AccessibilityRepairs {
            document_language: None,
            mark_as_tagged: Some(true),
            structure_tab_order_pages: vec![],
            alternative_texts: vec![],
            form_field_tooltips: vec![],
        };

        let Err(error) = remediate_accessibility_to_file(&input, "input.pdf", &repairs, &output)
        else {
            return Err("untagged document was incorrectly marked".into());
        };
        assert!(matches!(error, AccessibilityError::InvalidRepair(_)));
        assert!(!output.exists());
        Ok(())
    }
}
