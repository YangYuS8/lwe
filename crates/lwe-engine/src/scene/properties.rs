//! Bounded declarative defaults and pure scalar SceneScript property bindings.
//!
//! Direct user bindings and exact combo equality conditions are admitted,
//! together with exact `{"script":module,"value":scalar}` bindings
//! accepted by the independent bounded ECMAScript runtime. Broader scripts,
//! expressions, and unknown wrapper fields stay gated.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use anyhow::Result;
use serde::Serialize;
use serde_json::{Map, Value};

use super::{script, unsupported};

const MAX_PROPERTIES: usize = 256;
const MAX_NODES: usize = 100_000;
const MAX_DEPTH: usize = 64;
const MAX_LABEL_BYTES: usize = 4096;
const MAX_COMBO_OPTIONS: usize = 256;
const MAX_PROPERTY_STRING_BYTES: usize = 256;
const MAX_DISPLAY_NAME_BYTES: usize = 512;
const MAX_SCRIPTS: usize = 64;
const MAX_SCRIPT_BYTES: usize = 256 * 1024;

/// The finite, declarative controls admitted by the Scene runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyKind {
    Bool,
    Slider,
    Color,
    Combo,
    TextInput,
    TextDisplay,
    Texture,
}

/// Values exposed to the property editor. Colors are normalized RGB triples.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PropertyValue {
    Bool(bool),
    Slider(f64),
    Color([f32; 3]),
    Combo(String),
    TextInput(String),
    TextDisplay,
    Texture(Option<String>),
}

/// An author-declared selection, retaining its display label and stored value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PropertyOption {
    pub label: String,
    pub value: String,
}

/// An author-declared control and its current literal value.
///
/// `label` preserves the project's `text` verbatim, falling back to its name
/// when absent. Optional bounds, step and order remain absent when undeclared.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PropertyDefinition {
    pub name: String,
    pub label: String,
    pub kind: PropertyKind,
    pub value: PropertyValue,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub order: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<PropertyOption>,
}

/// A non-value, author-declared section. Its following controls belong to it
/// until the next section; controls before the first group remain ungrouped.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PropertyGroup {
    pub name: String,
    pub label: String,
    pub order: Option<f64>,
    pub members: Vec<String>,
}

enum PropertyEntry {
    Control(PropertyDefinition),
    Group(PropertyGroup),
}

impl PropertyEntry {
    fn sort_key(&self) -> (f64, &str) {
        match self {
            Self::Control(control) => (control.order.unwrap_or(0.0), &control.name),
            Self::Group(group) => (group.order.unwrap_or(0.0), &group.name),
        }
    }
}

/// List validated author controls in order, with a stable name tie-breaker.
///
/// This CPU-only operation performs no resource I/O or code evaluation. Scene
/// preparation still owns a separate, shared project-and-resources traversal
/// budget through `ProjectProperties`.
pub fn property_definitions(project: &Value) -> Result<Vec<PropertyDefinition>> {
    Ok(property_layout(project)?.0)
}

/// Groups are presentation metadata, never property values or script inputs.
pub fn property_groups(project: &Value) -> Result<Vec<PropertyGroup>> {
    Ok(property_layout(project)?.1)
}

fn property_layout(project: &Value) -> Result<(Vec<PropertyDefinition>, Vec<PropertyGroup>)> {
    let properties = ProjectProperties::from_project(project)?;
    let Some(declarations) = properties.declarations else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut entries = Vec::with_capacity(declarations.len());
    for (name, declaration) in declarations {
        let display = declaration["type"] == "text";
        let name_limit = if display { MAX_DISPLAY_NAME_BYTES } else { 256 };
        if name.is_empty() || name.len() > name_limit || (display && name.contains('\0')) {
            return Err(invalid("invalid project property name"));
        }
        let control = if display || declaration["type"] == "group" {
            None
        } else {
            Some(validated_declaration(declaration)?)
        };
        let object = match &control {
            Some(control) => control.object,
            None => validated_metadata(declaration)?,
        };
        let label = object
            .get("text")
            .map(|text| {
                text.as_str()
                    .filter(|text| text.len() <= MAX_LABEL_BYTES)
                    .ok_or_else(|| invalid("invalid project property label"))
            })
            .transpose()?
            .unwrap_or(name);
        let order = object.get("order").map(scalar).transpose()?;
        if display {
            entries.push(PropertyEntry::Control(PropertyDefinition {
                name: name.clone(),
                label: label.to_owned(),
                kind: PropertyKind::TextDisplay,
                value: PropertyValue::TextDisplay,
                min: None,
                max: None,
                step: None,
                order,
                options: Vec::new(),
            }));
            continue;
        }
        let Some(declaration) = control else {
            entries.push(PropertyEntry::Group(PropertyGroup {
                name: name.clone(),
                label: label.to_owned(),
                order,
                members: Vec::new(),
            }));
            continue;
        };
        entries.push(PropertyEntry::Control(PropertyDefinition {
            name: name.clone(),
            label: label.to_owned(),
            kind: declaration.kind,
            value: declaration.value,
            min: declaration.min,
            max: declaration.max,
            step: declaration.step,
            order,
            options: declaration.options,
        }));
    }
    entries.sort_by(|left, right| {
        let (left_order, left_name) = left.sort_key();
        let (right_order, right_name) = right.sort_key();
        left_order
            .total_cmp(&right_order)
            .then_with(|| left_name.cmp(right_name))
    });
    let mut definitions = Vec::new();
    let mut groups: Vec<PropertyGroup> = Vec::new();
    for entry in entries {
        match entry {
            PropertyEntry::Group(group) => groups.push(group),
            PropertyEntry::Control(control) => {
                if let Some(group) = groups.last_mut() {
                    group.members.push(control.name.clone());
                }
                definitions.push(control);
            }
        }
    }
    Ok((definitions, groups))
}

fn validated_metadata(declaration: &Value) -> Result<&Map<String, Value>> {
    let object = declaration
        .as_object()
        .ok_or_else(|| invalid("invalid project display metadata"))?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "type" | "text" | "order" | "index" | "value"))
    {
        return Err(invalid("unsupported project property display metadata"));
    }
    // The shipped group and text metadata may carry an empty placeholder.
    if object.get("value").is_some_and(|value| value != "") {
        return Err(invalid(
            "project property display metadata cannot declare values",
        ));
    }
    if object
        .get("index")
        .is_some_and(|index| index.as_u64().is_none())
    {
        return Err(invalid("invalid project property group index"));
    }
    if object.get("text").is_some_and(|text| {
        text.as_str()
            .is_none_or(|label| label.len() > MAX_LABEL_BYTES)
    }) {
        return Err(invalid("invalid project property label"));
    }
    if let Some(order) = object.get("order") {
        scalar(order)?;
    }
    Ok(object)
}

/// Validate every override before replacing any author value.
///
/// Unknown names, types, non-finite values and out-of-range values are errors.
/// RGB overrides accept a normalized three-element array or the project's RGB
/// string notation, and are stored in that original notation. Omitted controls
/// and all other project metadata are retained exactly. Step is a positive
/// editor increment; it does not quantize otherwise valid slider values.
pub fn apply_property_overrides(
    project: &mut Value,
    overrides: &BTreeMap<String, Value>,
) -> Result<()> {
    // Validate the unmodified project first: an override cannot erase a script,
    // expression, unsupported type or invalid declared default.
    let definitions = property_definitions(project)?;
    let mut updates = Vec::with_capacity(overrides.len().min(MAX_PROPERTIES));
    for (name, candidate) in overrides {
        if project["general"]["properties"][name]["type"] == "group" {
            return Err(invalid("property group metadata cannot be overridden"));
        }
        let definition = definitions
            .iter()
            .find(|definition| definition.name == *name)
            .ok_or_else(|| invalid("property override has no project declaration"))?;
        if definition.kind == PropertyKind::TextDisplay {
            return Err(invalid("property text metadata cannot be overridden"));
        }
        if definition.kind == PropertyKind::Texture {
            return Err(invalid(
                "resource properties require a validated image capability",
            ));
        }
        let normalized_color = match definition.kind {
            PropertyKind::Color => {
                let rgb = override_color(candidate)?;
                Some(Value::String(format!("{} {} {}", rgb[0], rgb[1], rgb[2])))
            }
            _ => None,
        };
        let candidate = normalized_color.as_ref().unwrap_or(candidate);
        validated_value(
            definition.kind,
            candidate,
            definition.min,
            definition.max,
            true,
            &definition.options,
        )?;
        updates.push((name, candidate.clone()));
    }
    if updates.is_empty() {
        return Ok(());
    }
    let declarations = project
        .get_mut("general")
        .and_then(|general| general.get_mut("properties"))
        .and_then(Value::as_object_mut)
        .expect("property definitions validated the declaration map");
    for (name, value) in updates {
        declarations
            .get_mut(name)
            .and_then(Value::as_object_mut)
            .expect("property definitions validated each declaration")
            .insert("value".to_owned(), value);
    }
    Ok(())
}

pub(super) struct ProjectProperties<'a> {
    declarations: Option<&'a Map<String, Value>>,
    remaining_nodes: Cell<usize>,
    remaining_scripts: Cell<usize>,
    remaining_script_bytes: Cell<usize>,
    script_values: RefCell<BTreeMap<(String, u64), f64>>,
}

impl<'a> ProjectProperties<'a> {
    pub(super) fn from_project(project: &'a Value) -> Result<Self> {
        let declarations = project
            .get("general")
            .and_then(|general| general.get("properties"))
            .map(|properties| {
                properties.as_object().ok_or_else(|| {
                    unsupported("property_binding", "invalid project property declarations")
                })
            })
            .transpose()?;
        if declarations.is_some_and(|declarations| declarations.len() > MAX_PROPERTIES) {
            return Err(unsupported(
                "property_binding",
                "project property count exceeds the preparation limit",
            ));
        }
        let properties = Self {
            declarations,
            remaining_nodes: Cell::new(MAX_NODES),
            remaining_scripts: Cell::new(MAX_SCRIPTS),
            remaining_script_bytes: Cell::new(MAX_SCRIPT_BYTES),
            script_values: RefCell::new(BTreeMap::new()),
        };
        // Project declarations stay literal; a script cannot replace their
        // typed defaults or metadata. Resource bindings are evaluated later.
        validate_document(project, 0, &properties.remaining_nodes, None)?;
        Ok(properties)
    }

    pub(super) fn resolve_document(&self, document: &mut Value) -> Result<()> {
        validate_document(document, 0, &self.remaining_nodes, Some(self))?;
        self.resolve(document, false)
    }

    pub(super) fn validate_timeline(&self, timeline: &Value) -> Result<()> {
        // The modeled timeline is extracted before the shared resource walk.
        validate_document(timeline, 0, &self.remaining_nodes, None)
    }

    pub(super) fn resolve_scene_document(&self, document: &mut Value) -> Result<()> {
        validate_document(document, 0, &self.remaining_nodes, Some(self))?;
        // Resolve direct layer slots before the general resource traversal.
        if let Some(objects) = document.get_mut("objects").and_then(Value::as_array_mut) {
            for object in objects {
                if let Some(visible) = object.get_mut("visible")
                    && let Some(wrapper) = visible.as_object()
                    && wrapper.len() == 2
                    && let (Some(user @ Value::String(_)), Some(Value::Bool(authored))) =
                        (wrapper.get("user"), wrapper.get("value"))
                {
                    let (name, _) = user_binding(user)?;
                    // The native bool loader sets the authored visibility, and
                    // a missing property never reaches the user value setter.
                    if name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                        && self.declarations.is_none_or(|map| !map.contains_key(name))
                    {
                        *visible = Value::Bool(*authored);
                    }
                }
                // Only the direct content of a Scene text object can consume user text.
                if ["image", "particle", "sound", "model"]
                    .iter()
                    .any(|key| object.get(key).is_some())
                {
                    continue;
                }
                let Some(text) = object.get_mut("text") else {
                    continue;
                };
                let name = text.get("user").and_then(Value::as_str);
                if name
                    .and_then(|name| self.declarations?.get(name))
                    .is_some_and(|declaration| declaration["type"] == "textinput")
                {
                    self.resolve(text, true)?;
                }
            }
        }
        self.resolve(document, false)
    }

    pub(super) fn shader_value(&self, name: &str) -> Result<Value> {
        if name.is_empty() || name.len() > 256 {
            return Err(invalid("invalid shader property name"));
        }
        let declaration = self
            .declarations
            .and_then(|declarations| declarations.get(name))
            .ok_or_else(|| invalid("shader property has no project declaration"))?;
        let declaration = validated_declaration(declaration)?;
        Ok(match declaration.value {
            PropertyValue::Bool(value) => Value::from(u8::from(value)),
            PropertyValue::Slider(value) => Value::from(value),
            PropertyValue::Color(value) => serde_json::to_value(value)?,
            PropertyValue::Combo(_)
            | PropertyValue::TextInput(_)
            | PropertyValue::TextDisplay
            | PropertyValue::Texture(_) => {
                return Err(invalid("property cannot be used as a shader value"));
            }
        })
    }

    fn resolve(&self, value: &mut Value, text_content: bool) -> Result<()> {
        match value {
            Value::Object(object) => {
                if let Some(source) = object.get("script").and_then(Value::as_str) {
                    let input = object["value"]
                        .as_f64()
                        .expect("scalar script input validated before resolution");
                    let output = self.script_values.borrow()[&(source.to_owned(), input.to_bits())];
                    *value = Value::from(output);
                    return Ok(());
                }
                if object.contains_key("binding") {
                    return Err(unsupported(
                        "property_binding",
                        "property binding expressions are not supported",
                    ));
                }
                if let Some(user) = object.get("user") {
                    if object.len() != 2 || !object.contains_key("value") {
                        return Err(unsupported(
                            "property_binding",
                            "unsupported user property binding shape",
                        ));
                    }
                    let (name, condition) = user_binding(user)?;
                    let declaration = self
                        .declarations
                        .and_then(|declarations| declarations.get(name))
                        .ok_or_else(|| {
                            unsupported(
                                "property_binding",
                                "user binding has no project declaration",
                            )
                        })?;
                    let fallback = &object["value"];
                    *value = if let Some(condition) = condition {
                        let declaration = validated_declaration(declaration)?;
                        if declaration.kind != PropertyKind::Combo || !fallback.is_boolean() {
                            return Err(invalid("combo conditions require a boolean binding"));
                        }
                        validated_value(
                            PropertyKind::Combo,
                            condition,
                            None,
                            None,
                            true,
                            &declaration.options,
                        )?;
                        Value::Bool(declaration.default == condition)
                    } else {
                        default_value(declaration, fallback, text_content)?
                    };
                } else {
                    for child in object.values_mut() {
                        self.resolve(child, false)?;
                    }
                }
            }
            Value::Array(array) => {
                for child in array {
                    self.resolve(child, false)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn validate_script(&self, object: &Map<String, Value>) -> Result<()> {
        if object.len() != 2 || !object.contains_key("value") {
            return Err(unsupported(
                "scenescript",
                "unsupported script binding shape",
            ));
        }
        let source = object
            .get("script")
            .and_then(Value::as_str)
            .ok_or_else(|| unsupported("scenescript", "script binding requires a module source"))?;
        let input = object["value"]
            .as_f64()
            .filter(|value| value.is_finite() && (*value as f32).is_finite())
            .ok_or_else(|| {
                unsupported(
                    "scenescript",
                    "script binding requires a finite scalar input",
                )
            })?;
        let remaining = self.remaining_scripts.get();
        let bytes = self.remaining_script_bytes.get();
        if remaining == 0 || source.len() > bytes {
            return Err(unsupported(
                "scenescript",
                "project scripts exceed the preparation limit",
            ));
        }
        self.remaining_scripts.set(remaining - 1);
        self.remaining_script_bytes.set(bytes - source.len());
        let key = (source.to_owned(), input.to_bits());
        if self.script_values.borrow().contains_key(&key) {
            return Ok(());
        }
        let mut snapshot = script::PropertySnapshot::new();
        if let Some(declarations) = self.declarations {
            for (name, declaration) in declarations {
                if matches!(declaration["type"].as_str(), Some("group" | "text")) {
                    validated_metadata(declaration)?;
                    continue;
                }
                match validated_declaration(declaration)?.value {
                    PropertyValue::Bool(value) => {
                        snapshot.insert(name.clone(), script::ScalarProperty::Bool(value));
                    }
                    PropertyValue::Slider(value) => {
                        snapshot.insert(name.clone(), script::ScalarProperty::Number(value));
                    }
                    // Color/vector access requires SceneScript Vec3 semantics
                    // and remains outside this scalar subset.
                    PropertyValue::Color(_)
                    | PropertyValue::Combo(_)
                    | PropertyValue::TextInput(_)
                    | PropertyValue::TextDisplay
                    | PropertyValue::Texture(_) => {}
                }
            }
        }
        let output = script::evaluate(source, input, &snapshot)?;
        self.script_values.borrow_mut().insert(key, output);
        Ok(())
    }
}

// Scan the bounded document before replacing bindings so a JS declaration in
// another object cannot lose its existing SceneScript blocker to an earlier
// unsupported declarative binding. The second traversal can only shrink it.
fn validate_document(
    value: &Value,
    depth: usize,
    nodes: &Cell<usize>,
    properties: Option<&ProjectProperties<'_>>,
) -> Result<()> {
    let remaining = nodes.get();
    if depth > MAX_DEPTH || remaining == 0 {
        return Err(invalid("property traversal exceeds the preparation limit"));
    }
    nodes.set(remaining - 1);
    match value {
        Value::Object(object) => {
            if properties.is_some()
                && object.contains_key("value")
                && object.contains_key("animation")
            {
                return Err(unsupported(
                    "timeline",
                    "unsupported property timeline binding",
                ));
            }
            if object
                .keys()
                .any(|key| matches!(key.to_ascii_lowercase().as_str(), "script" | "scenescript"))
            {
                if let (true, Some(properties)) = (object.contains_key("script"), properties) {
                    properties.validate_script(object)?;
                } else {
                    return Err(unsupported(
                        "scenescript",
                        "resource contains unsupported script bindings",
                    ));
                }
            }
            for child in object.values() {
                validate_document(child, depth + 1, nodes, properties)?;
            }
        }
        Value::Array(array) => {
            for child in array {
                validate_document(child, depth + 1, nodes, properties)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn invalid(detail: &'static str) -> anyhow::Error {
    unsupported("property_binding", detail)
}

fn scalar(value: &Value) -> Result<f64> {
    value
        .as_f64()
        .filter(|number| number.is_finite() && (*number as f32).is_finite())
        .ok_or_else(|| invalid("invalid finite slider property value"))
}

fn color(value: &Value) -> Result<[f32; 3]> {
    let numbers = match value {
        Value::String(string) if string.len() <= 256 => string
            .split_whitespace()
            .take(4)
            .map(str::parse::<f64>)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| invalid("invalid color property value"))?,
        _ => return Err(invalid("invalid color property value")),
    };
    let values: [f64; 3] = numbers
        .try_into()
        .map_err(|_| invalid("color properties require three components"))?;
    if !values
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    {
        return Err(invalid(
            "color property components must be finite and within 0..1",
        ));
    }
    Ok(values.map(|value| value as f32))
}

fn override_color(value: &Value) -> Result<[f32; 3]> {
    let Some(components) = value.as_array() else {
        return color(value);
    };
    if components.len() != 3 {
        return Err(invalid("color properties require three components"));
    }
    let mut rgb = [0.0; 3];
    for (channel, component) in rgb.iter_mut().zip(components) {
        let number = scalar(component)?;
        if !(0.0..=1.0).contains(&number) {
            return Err(invalid(
                "color property components must be finite and within 0..1",
            ));
        }
        *channel = number as f32;
    }
    Ok(rgb)
}

struct ValidatedDeclaration<'a> {
    object: &'a Map<String, Value>,
    default: &'a Value,
    kind: PropertyKind,
    value: PropertyValue,
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
    options: Vec<PropertyOption>,
}

fn validated_declaration(declaration: &Value) -> Result<ValidatedDeclaration<'_>> {
    let object = declaration
        .as_object()
        .ok_or_else(|| invalid("invalid project property declaration"))?;
    if matches!(
        object.get("type").and_then(Value::as_str),
        Some("group" | "text")
    ) {
        return Err(invalid(
            "property display metadata cannot be used as a value",
        ));
    }
    if object.keys().any(|key| {
        matches!(
            key.to_ascii_lowercase().as_str(),
            "binding" | "expr" | "expression" | "user"
        )
    }) {
        return Err(invalid(
            "property declaration expressions are not supported",
        ));
    }
    let default = object
        .get("value")
        .ok_or_else(|| invalid("project property has no default value"))?;
    let kind = match object.get("type").and_then(Value::as_str) {
        Some("bool") => PropertyKind::Bool,
        Some("slider") => PropertyKind::Slider,
        Some("color") => PropertyKind::Color,
        Some("combo") => PropertyKind::Combo,
        Some("textinput") => PropertyKind::TextInput,
        Some("scenetexture") => PropertyKind::Texture,
        _ => return Err(invalid("user property type is not in the validated subset")),
    };
    let (min, max, step) = if kind == PropertyKind::Slider {
        let min = object.get("min").map(scalar).transpose()?;
        let max = object.get("max").map(scalar).transpose()?;
        let step = object.get("step").map(scalar).transpose()?;
        if min.zip(max).is_some_and(|(min, max)| min > max) {
            return Err(invalid("invalid slider property range"));
        }
        if step.is_some_and(|step| step <= 0.0) {
            return Err(invalid("slider property step must be positive and finite"));
        }
        (min, max, step)
    } else {
        (None, None, None)
    };
    let options = if kind == PropertyKind::Combo {
        combo_options(
            object
                .get("options")
                .ok_or_else(|| invalid("combo property has no options"))?,
        )?
    } else {
        Vec::new()
    };
    let value = validated_value(kind, default, min, max, true, &options)?;
    Ok(ValidatedDeclaration {
        object,
        default,
        kind,
        value,
        min,
        max,
        step,
        options,
    })
}

fn validated_value(
    kind: PropertyKind,
    value: &Value,
    min: Option<f64>,
    max: Option<f64>,
    enforce_range: bool,
    options: &[PropertyOption],
) -> Result<PropertyValue> {
    match kind {
        PropertyKind::Bool => value
            .as_bool()
            .map(PropertyValue::Bool)
            .ok_or_else(|| invalid("boolean property binding requires boolean values")),
        PropertyKind::Slider => {
            let number = scalar(value)?;
            if enforce_range
                && (min.is_some_and(|min| number < min) || max.is_some_and(|max| number > max))
            {
                return Err(invalid(
                    "slider property value is outside its declared range",
                ));
            }
            Ok(PropertyValue::Slider(number))
        }
        PropertyKind::Color => color(value).map(PropertyValue::Color),
        PropertyKind::Combo => {
            let value = combo_value(value)?;
            if enforce_range && !options.iter().any(|option| option.value == value) {
                return Err(invalid(
                    "combo property value is outside its declared options",
                ));
            }
            Ok(PropertyValue::Combo(value.to_owned()))
        }
        PropertyKind::TextInput => value
            .as_str()
            .filter(|text| {
                text.len() <= MAX_PROPERTY_STRING_BYTES
                    && text.split('\n').count() <= 8
                    && text.chars().all(|c| c == '\n' || !c.is_control())
            })
            .map(|text| PropertyValue::TextInput(text.to_owned()))
            .ok_or_else(|| invalid("text property requires bounded plain text")),
        PropertyKind::TextDisplay => Err(invalid("property text metadata has no value")),
        PropertyKind::Texture => {
            if value != "" {
                return Err(invalid(
                    "only the authored empty image selection is supported",
                ));
            }
            Ok(PropertyValue::Texture(None))
        }
    }
}

fn default_value(declaration: &Value, fallback: &Value, text_content: bool) -> Result<Value> {
    // Project and document were already scanned once under the shared budget.
    // Do not rescan declaration metadata for every use of the same property.
    let declaration = validated_declaration(declaration)?;
    if declaration.kind == PropertyKind::Texture {
        return Err(invalid(
            "resource properties cannot be used as scalar values",
        ));
    }
    if declaration.kind == PropertyKind::TextInput && !text_content {
        return Err(invalid(
            "text properties require a Scene text content binding",
        ));
    }
    // A binding's fallback is not used when a declaration exists. Validate its
    // literal type, while the declared or overridden value enforces its bounds.
    validated_value(
        declaration.kind,
        fallback,
        declaration.min,
        declaration.max,
        false,
        &declaration.options,
    )?;
    Ok(declaration.default.clone())
}

fn combo_value(value: &Value) -> Result<&str> {
    value
        .as_str()
        .filter(|value| {
            !value.is_empty() && value.len() <= MAX_PROPERTY_STRING_BYTES && !value.contains('\0')
        })
        .ok_or_else(|| invalid("combo property requires a bounded nonempty string"))
}

fn combo_options(value: &Value) -> Result<Vec<PropertyOption>> {
    let options = value
        .as_array()
        .filter(|options| !options.is_empty() && options.len() <= MAX_COMBO_OPTIONS)
        .ok_or_else(|| invalid("combo property requires a bounded nonempty option list"))?;
    let mut validated: Vec<PropertyOption> = Vec::with_capacity(options.len());
    for option in options {
        let object = option
            .as_object()
            .filter(|object| object.len() == 2)
            .ok_or_else(|| invalid("invalid combo property option"))?;
        let label = object
            .get("label")
            .and_then(Value::as_str)
            .filter(|label| label.len() <= MAX_LABEL_BYTES)
            .ok_or_else(|| invalid("invalid combo property option label"))?;
        let value = combo_value(
            object
                .get("value")
                .ok_or_else(|| invalid("combo option has no value"))?,
        )?;
        if validated.iter().any(|option| option.value == value) {
            return Err(invalid("combo property options contain duplicate values"));
        }
        validated.push(PropertyOption {
            label: label.to_owned(),
            value: value.to_owned(),
        });
    }
    Ok(validated)
}

fn user_binding(user: &Value) -> Result<(&str, Option<&Value>)> {
    let (name, condition) = if let Some(name) = user.as_str() {
        (name, None)
    } else {
        let object = user
            .as_object()
            .filter(|object| object.len() == 2)
            .ok_or_else(|| invalid("unsupported conditional user binding shape"))?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("conditional user binding requires a property name"))?;
        let condition = object
            .get("condition")
            .ok_or_else(|| invalid("conditional user binding requires an equality condition"))?;
        (name, Some(condition))
    };
    if name.is_empty() || name.len() > 256 {
        return Err(invalid("user binding must name one property"));
    }
    Ok((name, condition))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::unsupported_feature_code;
    use serde_json::json;

    fn resolve(project: Value, mut document: Value) -> Result<Value> {
        ProjectProperties::from_project(&project)?.resolve_document(&mut document)?;
        Ok(document)
    }

    #[test]
    fn project_defaults_replace_binding_fallbacks_with_matching_types() {
        let project = json!({"general":{"properties":{
            "enabled":{"type":"bool","value":false},
            "opacity":{"type":"slider","value":0.25,"min":0,"max":1},
            "tint":{"type":"color","value":"0.1 0.2 0.3"}
        }}});
        let resolved = resolve(
            project,
            json!({
                "visible":{"user":"enabled","value":true},
                "alpha":{"user":"opacity","value":1},
                "color":{"user":"tint","value":"1 1 1"}
            }),
        )
        .unwrap();
        assert_eq!(
            resolved,
            json!({"visible":false,"alpha":0.25,"color":"0.1 0.2 0.3"})
        );
    }

    fn editable_project() -> Value {
        json!({"general":{"properties":{
            "enabled":{"type":"bool","value":false,"text":"  作者开关  ","order":20},
            "gain":{"type":"slider","value":0.4,"min":0.1,"max":0.9,
                "step":0.03,"text":"Author gain","order":10,"precision":3,"fraction":true},
            "accent":{"type":"color","value":"0.125 0.5 0.75","text":"Author tint","order":10},
            "plain":{"type":"bool","value":true}
        }},"title":"Untouched metadata"})
    }

    fn combo_project() -> Value {
        json!({"general":{"properties":{
            "selection":{"type":"combo","text":"Author selection","value":"first",
                "options":[{"label":"First choice","value":"first"},{"label":"Second choice","value":"second"}]}
        }}})
    }

    fn grouped_project() -> Value {
        json!({"general":{"properties":{
            "before":{"type":"bool","value":true,"order":0},
            "elements":{"type":"group","value":"","text":"ELEMENTS","index":1,"order":101},
            "enabled":{"type":"bool","value":false,"order":102},
            "gain":{"type":"slider","value":4,"order":103},
            "crt":{"type":"group","value":"","text":"CRT","index":7,"order":107},
            "noise":{"type":"slider","value":0.25,"order":108}
        }}})
    }

    #[test]
    fn groups_partition_following_controls_without_creating_values() {
        let project = grouped_project();
        let definitions = property_definitions(&project).unwrap();
        assert_eq!(
            definitions
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["before", "enabled", "gain", "noise"]
        );
        assert_eq!(
            serde_json::to_value(property_groups(&project).unwrap()).unwrap(),
            json!([
                {"name":"elements","label":"ELEMENTS","order":101.0,"members":["enabled","gain"]},
                {"name":"crt","label":"CRT","order":107.0,"members":["noise"]}
            ])
        );
        assert!(property_groups(&editable_project()).unwrap().is_empty());
        assert!(property_groups(&json!({})).unwrap().is_empty());
        // Group labels come from text; the native empty placeholder is never
        // exposed as the title or a typed value.
        assert_eq!(project["general"]["properties"]["elements"]["value"], "");
        assert_eq!(
            project["general"]["properties"]["elements"]["text"],
            "ELEMENTS"
        );
    }

    #[test]
    fn group_order_ties_empty_sections_and_duplicate_labels_are_stable() {
        let project = json!({"general":{"properties":{
            "a":{"type":"bool","value":true},
            "b":{"type":"group","text":""},
            "c":{"type":"bool","value":false,"order":0},
            "d":{"type":"group","value":"","text":"same","order":0},
            "e":{"type":"group","value":"","text":"same","order":0},
            "f":{"type":"slider","value":1,"order":0}
        }}});
        let groups = property_groups(&project).unwrap();
        assert_eq!(
            groups
                .iter()
                .map(|group| group.name.as_str())
                .collect::<Vec<_>>(),
            ["b", "d", "e"]
        );
        assert_eq!(groups[0].label, "");
        assert_eq!(groups[0].order, None);
        assert_eq!(groups[0].members, ["c"]);
        assert!(groups[1].members.is_empty());
        assert_eq!(groups[1].label, groups[2].label);
        assert_eq!(groups[2].members, ["f"]);
        assert_eq!(
            property_definitions(&project)
                .unwrap()
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "c", "f"]
        );
    }

    #[test]
    fn group_metadata_is_bounded_and_cannot_hide_dynamic_or_value_fields() {
        for declaration in [
            json!({"type":"group","value":false}),
            json!({"type":"group","value":1}),
            json!({"type":"group","value":"other"}),
            json!({"type":"group","value":null}),
            json!({"type":"group","value":[]}),
            json!({"type":"group","value":{"user":"enabled","value":""}}),
            json!({"type":"group","value":{"script":"export function update(v){return v;}","value":0}}),
            json!({"type":"group","text":{"user":"enabled","value":"Title"}}),
            json!({"type":"group","text":"x".repeat(MAX_LABEL_BYTES + 1)}),
            json!({"type":"group","order":null}),
            json!({"type":"group","order":"101"}),
            json!({"type":"group","index":-1}),
            json!({"type":"group","index":0.5}),
            json!({"type":"group","user":"enabled"}),
            json!({"type":"group","condition":"enabled.value == true"}),
        ] {
            let project = json!({"general":{"properties":{"section":declaration}}});
            assert!(
                property_definitions(&project).is_err(),
                "accepted {project}"
            );
            assert!(property_groups(&project).is_err());
        }
        for name in [String::new(), "x".repeat(257)] {
            let project = json!({"general":{"properties":{name:{"type":"group"}}}});
            assert!(property_groups(&project).is_err());
        }
        let mut project = grouped_project();
        let declarations = project["general"]["properties"].as_object_mut().unwrap();
        for index in declarations.len()..MAX_PROPERTIES {
            declarations.insert(format!("section{index}"), json!({"type":"group"}));
        }
        assert!(property_groups(&project).is_ok());
        project["general"]["properties"]["oneTooMany"] = json!({"type":"group"});
        assert!(property_groups(&project).is_err());
        // Use a retained project so the shared traversal budget includes the
        // headers and their metadata before any resource is visited.
        let project = grouped_project();
        let properties = ProjectProperties::from_project(&project).unwrap();
        let mut ungrouped = project.clone();
        let declarations = ungrouped["general"]["properties"].as_object_mut().unwrap();
        declarations.remove("elements");
        declarations.remove("crt");
        let without_headers = ProjectProperties::from_project(&ungrouped).unwrap();
        assert!(properties.remaining_nodes.get() < without_headers.remaining_nodes.get());
        let mut resource = Value::Array(vec![Value::Null; MAX_NODES]);
        assert!(properties.resolve_document(&mut resource).is_err());
    }

    #[test]
    fn groups_reject_value_bindings_and_all_overrides_before_mutation() {
        let original = grouped_project();
        for binding in [
            json!({"user":"elements","value":true}),
            json!({"user":"elements","value":""}),
            json!({"user":{"name":"elements","condition":""},"value":false}),
        ] {
            assert_eq!(
                unsupported_feature_code(&resolve(original.clone(), binding).unwrap_err()),
                Some("property_binding")
            );
        }
        assert!(
            ProjectProperties::from_project(&original)
                .unwrap()
                .shader_value("elements")
                .is_err()
        );
        for value in [json!(""), json!(true), json!(null)] {
            let mut project = original.clone();
            assert!(
                apply_property_overrides(
                    &mut project,
                    &BTreeMap::from([("before".into(), json!(false)), ("elements".into(), value),])
                )
                .is_err()
            );
            assert_eq!(project, original);
        }
        let mut project = original.clone();
        apply_property_overrides(&mut project, &BTreeMap::from([("gain".into(), json!(6))]))
            .unwrap();
        assert_eq!(project["general"]["properties"]["gain"]["value"], 6);
        assert_eq!(
            property_groups(&project).unwrap(),
            property_groups(&original).unwrap()
        );
    }

    #[test]
    fn scalar_scripts_with_groups_receive_only_value_controls() {
        let mut project = grouped_project();
        let source = "let gain=0; export function applyUserProperties(p){gain=p.gain;} export function update(v){return gain/10;}";
        let document = json!({"alpha":{"script":source,"value":1}});
        assert_eq!(
            resolve(project.clone(), document.clone()).unwrap()["alpha"],
            0.4
        );
        apply_property_overrides(&mut project, &BTreeMap::from([("gain".into(), json!(7))]))
            .unwrap();
        assert_eq!(resolve(project.clone(), document).unwrap()["alpha"], 0.7);
        let source = "let gain=0; export function applyUserProperties(p){gain=p.elements;} export function update(v){return gain;}";
        assert_eq!(
            unsupported_feature_code(
                &resolve(project, json!({"script":source,"value":1})).unwrap_err()
            ),
            Some("scenescript")
        );
    }

    #[test]
    fn group_support_does_not_admit_other_native_control_kinds() {
        let mut project = grouped_project();
        for kind in ["usershortcut", "unknown"] {
            project["general"]["properties"]["custombackground"] =
                json!({"type":kind,"value":"","order":106});
            assert_eq!(
                unsupported_feature_code(&property_definitions(&project).unwrap_err()),
                Some("property_binding")
            );
            assert!(property_groups(&project).is_err());
        }
    }

    #[test]
    fn image_controls_keep_empty_defaults_separate_from_scalar_values() {
        let mut project = grouped_project();
        project["general"]["properties"]["background"] =
            json!({"type":"scenetexture","value":"","text":"Background"});
        let definitions = property_definitions(&project).unwrap();
        let image = definitions
            .iter()
            .find(|entry| entry.name == "background")
            .unwrap();
        assert_eq!(image.kind, PropertyKind::Texture);
        assert_eq!(image.value, PropertyValue::Texture(None));
        let before = project.clone();
        for value in [
            json!(""),
            json!("sha256:abc"),
            json!("../image.png"),
            json!(null),
        ] {
            assert!(
                apply_property_overrides(
                    &mut project,
                    &BTreeMap::from([("background".into(), value)])
                )
                .is_err()
            );
            assert_eq!(project, before);
        }
        assert!(resolve(project.clone(), json!({"user":"background","value":""})).is_err());
        assert!(
            ProjectProperties::from_project(&project)
                .unwrap()
                .shader_value("background")
                .is_err()
        );
        project["general"]["properties"]["background"]["value"] = json!("file:///image.png");
        assert!(property_definitions(&project).is_err());
    }

    #[test]
    fn combo_options_preserve_labels_and_values_and_resolve_boolean_conditions() {
        let mut project = combo_project();
        let definitions = property_definitions(&project).unwrap();
        assert_eq!(definitions[0].kind, PropertyKind::Combo);
        assert_eq!(definitions[0].value, PropertyValue::Combo("first".into()));
        assert_eq!(
            definitions[0].options[1],
            PropertyOption {
                label: "Second choice".into(),
                value: "second".into()
            }
        );
        assert_eq!(
            serde_json::to_value(&definitions).unwrap()[0]["options"][0],
            json!({"label":"First choice","value":"first"})
        );
        let bindings = json!({
            "first":{"user":{"name":"selection","condition":"first"},"value":false},
            "second":{"user":{"name":"selection","condition":"second"},"value":true},
            "direct":{"user":"selection","value":"second"}
        });
        assert_eq!(
            resolve(project.clone(), bindings.clone()).unwrap(),
            json!({"first":true,"second":false,"direct":"first"})
        );
        apply_property_overrides(
            &mut project,
            &BTreeMap::from([("selection".into(), json!("second"))]),
        )
        .unwrap();
        assert_eq!(
            resolve(project, bindings).unwrap(),
            json!({"first":false,"second":true,"direct":"second"})
        );
    }

    #[test]
    fn invalid_combo_declarations_and_overrides_fail_without_modifying_project() {
        let original = combo_project();
        for candidate in [
            json!("missing"),
            json!(0),
            json!(""),
            json!("first\0"),
            json!("x".repeat(MAX_PROPERTY_STRING_BYTES + 1)),
        ] {
            let mut project = original.clone();
            assert!(
                apply_property_overrides(
                    &mut project,
                    &BTreeMap::from([("selection".into(), candidate)])
                )
                .is_err()
            );
            assert_eq!(project, original);
        }
        for options in [
            json!([]),
            json!(null),
            json!([{"label":"First","value":0}]),
            json!([{"label":"First","value":"first"},{"label":"Duplicate","value":"first"}]),
            json!([{"label":"First","value":"first","script":"code"}]),
            json!([{"label":0,"value":"first"}]),
            json!([{"label":"x".repeat(MAX_LABEL_BYTES + 1),"value":"first"}]),
            json!([{"label":"Other","value":"other"}]),
            json!(
                (0..=MAX_COMBO_OPTIONS)
                    .map(|index| json!({"label":"Choice","value":index.to_string()}))
                    .collect::<Vec<_>>()
            ),
        ] {
            let mut project = original.clone();
            project["general"]["properties"]["selection"]["options"] = options;
            assert!(property_definitions(&project).is_err());
        }
        for binding in [
            json!({"user":{"name":"selection","condition":"missing"},"value":true}),
            json!({"user":{"name":"selection","condition":"first"},"value":1}),
            json!({"user":{"name":"selection","condition":1},"value":true}),
            json!({"user":{"name":"selection","condition":"first","invert":true},"value":true}),
            json!({"user":{"name":"selection"},"value":true}),
        ] {
            assert!(resolve(original.clone(), binding).is_err());
        }
        assert!(
            ProjectProperties::from_project(&original)
                .unwrap()
                .shader_value("selection")
                .is_err()
        );
    }

    #[test]
    fn definitions_expose_typed_values_and_preserve_author_labels_and_order() {
        let definitions = property_definitions(&editable_project()).unwrap();
        assert_eq!(
            definitions
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["plain", "accent", "gain", "enabled"]
        );
        assert_eq!(definitions[0].label, "plain");
        assert_eq!(definitions[0].value, PropertyValue::Bool(true));
        assert_eq!(definitions[0].order, None);
        assert_eq!(
            definitions[1].value,
            PropertyValue::Color([0.125, 0.5, 0.75])
        );
        assert_eq!(definitions[2].value, PropertyValue::Slider(0.4));
        assert_eq!(definitions[2].min, Some(0.1));
        assert_eq!(definitions[2].max, Some(0.9));
        assert_eq!(definitions[2].step, Some(0.03));
        assert_eq!(definitions[3].label, "  作者开关  ");
        let encoded = serde_json::to_value(&definitions).unwrap();
        assert_eq!(encoded[0]["kind"], "bool");
        assert_eq!(encoded[0]["value"], true);
        assert_eq!(encoded[0]["min"], Value::Null);
        assert_eq!(encoded[1]["kind"], "color");
        assert_eq!(encoded[1]["value"], json!([0.125, 0.5, 0.75]));
        assert_eq!(encoded[2]["kind"], "slider");
        assert_eq!(encoded[2]["value"], 0.4);
        assert_eq!(encoded[3]["label"], "  作者开关  ");
    }

    #[test]
    fn overrides_change_only_selected_literals_and_resolve_with_the_same_rules() {
        let mut project = editable_project();
        let original = project.clone();
        apply_property_overrides(
            &mut project,
            &BTreeMap::from([
                ("enabled".to_owned(), json!(true)),
                ("gain".to_owned(), json!(0.43)),
                ("accent".to_owned(), json!([0.25, 0.5, 0.875])),
            ]),
        )
        .unwrap();
        let mut expected = original.clone();
        expected["general"]["properties"]["enabled"]["value"] = json!(true);
        expected["general"]["properties"]["gain"]["value"] = json!(0.43);
        expected["general"]["properties"]["accent"]["value"] = json!("0.25 0.5 0.875");
        assert_eq!(project, expected);
        assert_eq!(
            resolve(
                project,
                json!({
                    "enabled":{"user":"enabled","value":false},
                    "gain":{"user":"gain","value":99},
                    "accent":{"user":"accent","value":"0 0 0"},
                    "plain":{"user":"plain","value":false}
                })
            )
            .unwrap(),
            json!({"enabled":true,"gain":0.43,"accent":"0.25 0.5 0.875","plain":true})
        );
        let mut untouched = original.clone();
        apply_property_overrides(&mut untouched, &BTreeMap::new()).unwrap();
        assert_eq!(untouched, original);
    }

    #[test]
    fn invalid_overrides_retain_the_entire_original_project() {
        let original = editable_project();
        for (name, candidate) in [
            ("zz_unknown", json!(false)),
            ("enabled", json!(1)),
            ("enabled", json!("true")),
            ("gain", json!("0.5")),
            ("gain", json!(0.099)),
            ("gain", json!(0.901)),
            ("gain", json!(1e100)),
            ("accent", json!([0, 0])),
            ("accent", json!([0, 0, 0, 0])),
            ("accent", json!([0, "NaN", 0])),
            ("accent", json!([0, 1e100, 0])),
            ("accent", json!([0, -0.01, 0])),
            ("accent", json!([0, 1.00000000001, 0])),
            ("accent", json!("NaN 0 0")),
            ("accent", json!("Infinity 0 0")),
            ("accent", json!("0 1.00000000001 0")),
            ("accent", json!({"value":"0 0 0","binding":"expression"})),
        ] {
            let mut project = original.clone();
            let mut overrides = BTreeMap::from([("plain".to_owned(), json!(false))]);
            overrides.insert(name.to_owned(), candidate);
            let error = apply_property_overrides(&mut project, &overrides).unwrap_err();
            assert_eq!(unsupported_feature_code(&error), Some("property_binding"));
            assert_eq!(project, original);
        }
        // A valid, lexically earlier change must not escape when a later one
        // fails. This covers transactional validation independent of key order.
        let mut project = original.clone();
        assert!(
            apply_property_overrides(
                &mut project,
                &BTreeMap::from([
                    ("accent".to_owned(), json!([0.25, 0.5, 0.75])),
                    ("enabled".to_owned(), json!(1))
                ])
            )
            .is_err()
        );
        assert_eq!(project, original);
    }

    #[test]
    fn invalid_declarations_cannot_be_hidden_by_valid_overrides() {
        for declaration in [
            json!({"type":"bool","value":{"script":"code","value":true}}),
            json!({"type":"bool","value":{"binding":"expression","value":true}}),
            json!({"type":"bool","value":true,"binding":"expression"}),
            json!({"type":"bool","value":true,"Expression":"expression"}),
            json!({"type":"bool","value":true,"expr":"expression"}),
            json!({"type":"textinput","value":false}),
            json!({"type":"slider","value":2,"min":0,"max":1}),
            json!({"type":"bool","value":1}),
            json!({"type":"color","value":"NaN 0 0"}),
            json!({"type":"bool"}),
        ] {
            let original = json!({"general":{"properties":{"flag":declaration}}});
            assert!(property_definitions(&original).is_err());
            let mut project = original.clone();
            assert!(
                apply_property_overrides(
                    &mut project,
                    &BTreeMap::from([("flag".to_owned(), json!(true))])
                )
                .is_err()
            );
            assert_eq!(project, original);
            assert!(apply_property_overrides(&mut project, &BTreeMap::new()).is_err());
            assert_eq!(project, original);
        }
    }

    #[test]
    fn invalid_metadata_and_declarations_are_rejected_even_when_unused() {
        for declaration in [
            json!({"type":"bool","value":true,"text":1}),
            json!({"type":"bool","value":true,"text":"x".repeat(MAX_LABEL_BYTES + 1)}),
            json!({"type":"bool","value":true,"order":"first"}),
            json!({"type":"bool","value":true,"order":1e100}),
            json!({"type":"slider","value":0.5,"min":1,"max":0}),
            json!({"type":"slider","value":0.5,"min":"0"}),
            json!({"type":"slider","value":0.5,"max":1e100}),
            json!({"type":"slider","value":0.5,"step":0}),
            json!({"type":"slider","value":0.5,"step":-0.1}),
            json!({"type":"slider","value":0.5,"step":"0.1"}),
            json!({"type":"slider","value":0.5,"step":1e100}),
            json!({"type":"unknown","value":true}),
            json!({"value":true}),
            json!(false),
        ] {
            let mut project = json!({"general":{"properties":{"unused":declaration}}});
            assert!(property_definitions(&project).is_err());
            assert!(apply_property_overrides(&mut project, &BTreeMap::new()).is_err());
        }
        for name in [String::new(), "x".repeat(257)] {
            let project = json!({"general":{"properties":{name:{"type":"bool","value":true}}}});
            assert!(property_definitions(&project).is_err());
        }
    }

    #[test]
    fn color_string_compatibility_and_declared_slider_boundaries_are_retained() {
        let mut project = editable_project();
        apply_property_overrides(
            &mut project,
            &BTreeMap::from([
                ("accent".to_owned(), json!(" 0.25  0.5\t0.875 ")),
                ("gain".to_owned(), json!(0.1)),
            ]),
        )
        .unwrap();
        assert_eq!(
            project["general"]["properties"]["accent"]["value"],
            "0.25 0.5 0.875"
        );
        apply_property_overrides(
            &mut project,
            &BTreeMap::from([("gain".to_owned(), json!(0.9))]),
        )
        .unwrap();
        assert_eq!(project["general"]["properties"]["gain"]["value"], 0.9);
        let mut absent = json!({});
        assert!(property_definitions(&absent).unwrap().is_empty());
        apply_property_overrides(&mut absent, &BTreeMap::new()).unwrap();
        assert_eq!(absent, json!({}));
        assert!(
            apply_property_overrides(
                &mut absent,
                &BTreeMap::from([("flag".to_owned(), json!(true))])
            )
            .is_err()
        );
        assert_eq!(absent, json!({}));
    }

    #[test]
    fn workshop_365_declarations_resolve_all_six_observed_direct_bindings() {
        // Reduced, authored metadata reproduces the types/defaults and binding
        // shapes read from local 3659880761. No original media is included.
        let project = json!({"general":{"properties":{
            "hdr":{"type":"bool","value":true},
            "newproperty":{"type":"bool","value":true},
            "_1":{"type":"bool","value":true},
            "_2":{"type":"bool","value":true},
            "newproperty3":{"type":"slider","value":1,"min":0,"max":1},
            "newproperty2":{"type":"slider","value":0,"min":0,"max":1}
        }}});
        let scene = json!({
            "general":{"hdr":false,"bloom":{"user":"hdr","value":true}},
            "objects":[
                {"visible":{"user":"_2","value":true},"effects":[
                    {"visible":{"user":"newproperty","value":true}},
                    {"visible":{"user":"_1","value":true}}
                ]},
                {"volume":{"user":"newproperty3","value":1.0}},
                {"volume":{"user":"newproperty2","value":0.0}}
            ]
        });
        let resolved = resolve(project, scene).unwrap();
        assert_eq!(resolved["general"]["hdr"], false);
        assert_eq!(resolved["general"]["bloom"], true);
        assert_eq!(resolved["objects"][0]["visible"], true);
        assert_eq!(resolved["objects"][0]["effects"][0]["visible"], true);
        assert_eq!(resolved["objects"][0]["effects"][1]["visible"], true);
        assert_eq!(resolved["objects"][1]["volume"], 1);
        assert_eq!(resolved["objects"][2]["volume"], 0);
    }

    #[test]
    fn undeclared_visibility_fallback_is_limited_to_plain_layer_bool_bindings() {
        let project = json!({"general":{"properties":{"present":{"type":"bool","value":false}}}});
        let properties = ProjectProperties::from_project(&project).unwrap();
        let mut scene = json!({"objects":[
            {"visible":{"user":"missing","value":true}},
            {"visible":{"user":"present","value":true}}
        ]});
        properties.resolve_scene_document(&mut scene).unwrap();
        assert_eq!(
            scene,
            json!({"objects":[{"visible":true},{"visible":false}]})
        );
        for object in [
            json!({"visible":{"user":"missing","value":1}}),
            json!({"visible":{"user":{"name":"missing","condition":"x"},"value":true}}),
            json!({"visible":{"user":"","value":true}}),
            json!({"visible":{"user":"$mediaThumbnail","value":true}}),
            json!({"visible":{"user":"missing","value":true,"other":true}}),
            json!({"alpha":{"user":"missing","value":0.5}}),
            json!({"effects":[{"visible":{"user":"missing","value":true}}]}),
        ] {
            let mut scene = json!({"objects":[object]});
            assert!(properties.resolve_scene_document(&mut scene).is_err());
        }
    }

    #[test]
    fn missing_malformed_or_unknown_resource_bindings_are_never_defaulted() {
        let project = json!({"general":{"properties":{
            "flag":{"type":"bool","value":true},
            "slider":{"type":"slider","value":0.5,"min":0,"max":1},
            "text":{"type":"textinput","value":"anything"},
            "missing":{"type":"bool"},
            "wrong":{"type":"bool","value":1},
            "range":{"type":"slider","value":2,"min":0,"max":1}
        }}});
        for binding in [
            json!({"user":"unknown","value":true}),
            json!({"user":{"name":"flag"},"value":true}),
            json!({"user":"flag"}),
            json!({"user":"flag","value":true,"multiply":2}),
            json!({"binding":"flag","value":true}),
            json!({"user":"text","value":"anything"}),
            json!({"user":"missing","value":true}),
            json!({"user":"wrong","value":true}),
            json!({"user":"flag","value":1}),
            json!({"user":"slider","value":"0.5"}),
            json!({"user":"range","value":0.5}),
        ] {
            let error = resolve(project.clone(), binding).unwrap_err();
            assert_eq!(unsupported_feature_code(&error), Some("property_binding"));
        }
    }

    #[test]
    fn scripts_and_non_finite_colors_remain_blocked() {
        let error = resolve(
            json!({}),
            json!({"a":{"user":"missing","value":true},"z":{"script":"code"}}),
        )
        .unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("scenescript"));
        for key in ["script", "scenescript", "SceneScript"] {
            let error =
                resolve(json!({}), json!({"unused":{(key):"code","value":true}})).unwrap_err();
            assert_eq!(unsupported_feature_code(&error), Some("scenescript"));
            let project = json!({"general":{"properties":{"flag":{"type":"bool","value":true}}}});
            let error = resolve(
                project,
                json!({"user":"flag","value":{(key):"code","value":true}}),
            )
            .unwrap_err();
            assert_eq!(unsupported_feature_code(&error), Some("scenescript"));
        }
        for value in [
            json!("NaN 0 0"),
            json!("0 0"),
            json!("0 2 0"),
            json!([0, 0, 1e100]),
            json!([0.1, 0.2, 0.3]),
            json!(format!("{}0 0 0", " ".repeat(256))),
        ] {
            let project = json!({"general":{"properties":{"tint":{"type":"color","value":value}}}});
            let error = resolve(project, json!({"user":"tint","value":"1 1 1"})).unwrap_err();
            assert_eq!(unsupported_feature_code(&error), Some("property_binding"));
        }
    }

    #[test]
    fn pure_script_bindings_follow_overrides_and_prescan_hidden_scripts() {
        let source = "let gain=0; export function applyUserProperties(p) { if (p.hasOwnProperty('gain')) { gain=p.gain; } } export function update(v) { return gain / 10; }";
        let mut project =
            json!({"general":{"properties":{"gain":{"type":"slider","value":4,"min":0,"max":10}}}});
        let document = json!({"objects":[{"visible":false,"alpha":{"script":source,"value":1}}]});
        assert_eq!(
            resolve(project.clone(), document.clone()).unwrap()["objects"][0]["alpha"],
            0.4
        );
        apply_property_overrides(&mut project, &BTreeMap::from([("gain".into(), json!(9))]))
            .unwrap();
        assert_eq!(
            resolve(project.clone(), document.clone()).unwrap()["objects"][0]["alpha"],
            0.9
        );
        let properties = ProjectProperties::from_project(&project).unwrap();
        let mut invalid = json!({"a":{"user":"missing","value":true},"z":{"visible":false,"alpha":{"script":"export function update(v){return engine.runtime;}","value":0}}});
        let before = invalid.clone();
        let error = properties.resolve_document(&mut invalid).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("scenescript"));
        assert_eq!(invalid, before);
        for wrapper in [
            json!({"script":source,"value":1,"extra":true}),
            json!({"script":source,"value":true}),
            json!({"scenescript":source,"value":1}),
        ] {
            assert_eq!(
                unsupported_feature_code(&resolve(project.clone(), wrapper).unwrap_err()),
                Some("scenescript")
            );
        }
    }

    #[test]
    fn scripts_share_a_bounded_project_budget_and_cached_values() {
        let source = "let gain=0; export function applyUserProperties(p){gain=p.gain;} export function update(v){return gain/2;}";
        let project = json!({"general":{"properties":{"gain":{"type":"slider","value":6}}}});
        let properties = ProjectProperties::from_project(&project).unwrap();
        let wrapper = json!({"script":source,"value":1});
        for _ in 0..MAX_SCRIPTS {
            let mut document = wrapper.clone();
            properties.resolve_document(&mut document).unwrap();
            assert_eq!(document, 3.0);
        }
        assert_eq!(properties.script_values.borrow().len(), 1);
        let mut document = wrapper;
        let error = properties.resolve_document(&mut document).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("scenescript"));
    }

    #[test]
    fn traversal_and_declaration_limits_fail_closed() {
        let mut document = json!(0);
        for _ in 0..MAX_DEPTH + 1 {
            document = json!([document]);
        }
        assert!(resolve(json!({}), document).is_err());
        let declarations: Map<String, Value> = (0..=MAX_PROPERTIES)
            .map(|index| {
                (
                    format!("property{index}"),
                    json!({"type":"bool","value":true}),
                )
            })
            .collect();
        assert!(
            ProjectProperties::from_project(&json!({"general":{"properties":declarations}}))
                .is_err()
        );
        let error = resolve(json!({}), Value::Array(vec![Value::Null; MAX_NODES])).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("property_binding"));
    }

    #[test]
    fn all_project_resources_share_one_budget_and_failures_consume_it() {
        let project = json!({"general":{"properties":{"flag":{"type":"bool","value":true}}}});
        let properties = ProjectProperties::from_project(&project).unwrap();
        assert!(properties.remaining_nodes.get() < MAX_NODES);
        let mut invalid = json!({"user":"missing","value":true});
        let before = properties.remaining_nodes.get();
        assert!(properties.resolve_document(&mut invalid).is_err());
        assert_eq!(properties.remaining_nodes.get(), before - 3);
        let mut resource = Value::Array(vec![Value::Null; MAX_NODES / 2]);
        properties.resolve_document(&mut resource).unwrap();
        let error = properties.resolve_document(&mut resource).unwrap_err();
        assert_eq!(unsupported_feature_code(&error), Some("property_binding"));
        assert_eq!(properties.remaining_nodes.get(), 0);
        assert!(properties.resolve_document(&mut Value::Null).is_err());
    }
}

#[cfg(test)]
mod text_controls_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn display_text_keeps_order_and_has_no_property_value() {
        let display = "a".repeat(314);
        let label = "<img src='https://example.invalid/text'>".to_owned();
        let mut project = json!({"general":{"properties":{
            "panel":{"type":"group","text":"Text","order":1},
            (display.clone()):{"type":"text","text": label,"value":"","index":3,"order":2},
            "caption":{"type":"textinput","value":"Default","order":3}
        }}});
        let definitions = property_definitions(&project).unwrap();
        let groups = property_groups(&project).unwrap();
        assert_eq!(definitions[0].name, display);
        assert_eq!(definitions[0].kind, PropertyKind::TextDisplay);
        assert_eq!(definitions[0].label, label);
        assert_eq!(
            serde_json::to_value(&definitions[0]).unwrap()["value"],
            Value::Null
        );
        assert_eq!(groups[0].members, [display.as_str(), "caption"]);

        let original = project.clone();
        assert!(
            apply_property_overrides(
                &mut project,
                &BTreeMap::from([
                    ("caption".into(), json!("Changed")),
                    (display.clone(), json!(""))
                ])
            )
            .is_err()
        );
        assert_eq!(project, original);
        project["general"]["properties"][&display]["value"] = json!("not metadata");
        assert!(property_definitions(&project).is_err());
        project["general"]["properties"][&display] = json!({"type":"bool","value":true});
        assert!(property_definitions(&project).is_err());
    }

    #[test]
    fn text_input_resolves_only_to_direct_scene_text_and_preserves_empty_overrides() {
        let mut project = json!({"general":{"properties":{
            "caption":{"type":"textinput","value":"初音\nMiku"}
        }}});
        let bindings = json!({"objects":[{"text":{"user":"caption","value":"Fallback"}}]});
        let mut scene = bindings.clone();
        let properties = ProjectProperties::from_project(&project).unwrap();
        properties.resolve_scene_document(&mut scene).unwrap();
        assert_eq!(scene["objects"][0]["text"], "初音\nMiku");
        assert!(properties.shader_value("caption").is_err());
        assert!(properties.resolve_document(&mut bindings.clone()).is_err());
        for mut invalid in [
            json!({"objects":[{"image":{"user":"caption","value":"models/item.json"}}]}),
            json!({"objects":[{"text":"Literal","font":{"user":"caption","value":"fonts/Test.otf"}}]}),
            json!({"objects":[{"text":{"user":"caption","value":5}}]}),
            json!({"objects":[{"text":{"user":{"name":"caption","condition":"x"},"value":true}}]}),
        ] {
            assert!(
                ProjectProperties::from_project(&project)
                    .unwrap()
                    .resolve_scene_document(&mut invalid)
                    .is_err()
            );
        }
        drop(properties);
        let original = project.clone();
        for invalid in [
            json!(false),
            json!("\u{0000}"),
            json!("\ttext"),
            json!("A\n".repeat(8)),
            json!("我".repeat(86)),
        ] {
            assert!(
                apply_property_overrides(
                    &mut project,
                    &BTreeMap::from([("caption".into(), invalid)])
                )
                .is_err()
            );
            assert_eq!(project, original);
        }
        apply_property_overrides(
            &mut project,
            &BTreeMap::from([("caption".into(), json!(""))]),
        )
        .unwrap();
        let mut scene = bindings;
        ProjectProperties::from_project(&project)
            .unwrap()
            .resolve_scene_document(&mut scene)
            .unwrap();
        assert_eq!(scene["objects"][0]["text"], "");
    }
}
