//! Bounded SceneScript layer bindings. Only preparation and a per-renderer
//! worker execute ECMAScript; frame submission and snapshot reads never wait.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Result, ensure};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    AssignmentTarget, BindingPatternKind, Declaration, Expression, ForStatementInit,
    SimpleAssignmentTarget, Statement, VariableDeclaration,
};
use oxc_parser::Parser;
use oxc_span::SourceType;
use rquickjs::context::intrinsic;
use rquickjs::{Context, Function, Module, Object, Persistent, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    media_input::{MediaFeed, MediaSnapshot, Playback},
    property_timeline::{PropertyTimeline, Sample},
    unsupported,
};

pub(super) const MAX_DYNAMIC_LAYERS: usize = 128;
const MAX_BINDINGS: usize = 32;
const MAX_SOURCE_BYTES: usize = 64 * 1024;
const MAX_TOTAL_SOURCE_BYTES: usize = 256 * 1024;
const MAX_AST_DEPTH: usize = 48;
const MEMORY_BYTES: usize = 8 * 1024 * 1024;
const STACK_BYTES: usize = 256 * 1024;
const INITIALIZATION_LIMIT: Duration = Duration::from_millis(100);
const FRAME_LIMIT: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct LayerState {
    pub handle: usize,
    pub origin: [f32; 3],
    pub scale: [f32; 3],
    pub angles: [f32; 3],
    pub color: [f32; 3],
    pub alpha: f32,
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct Binding {
    pub layer: usize,
    pub property: String,
    pub source: String,
    pub input: Value,
    pub script_properties: Value,
    pub timeline: Option<PropertyTimeline>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(super) struct CreatedLayer {
    pub path: String,
    pub state: LayerState,
}

#[derive(Debug, Clone)]
pub(super) struct PreparedScripts {
    bindings: Vec<Binding>,
    timeline_owners: Vec<Option<usize>>,
    layers: Vec<LayerState>,
    properties: Value,
    created: Vec<CreatedLayer>,
    initial: Vec<LayerState>,
    audio: bool,
    media: bool,
}

fn blocked(detail: impl Into<String>) -> anyhow::Error {
    unsupported("scenescript", detail)
}

fn timeline_owners(bindings: &[Binding]) -> Result<Vec<Option<usize>>> {
    let invalid = |detail| unsupported("timeline", detail);
    bindings
        .iter()
        .enumerate()
        .map(|(index, binding)| {
            let Some(track) = &binding.timeline else {
                return Ok(None);
            };
            for child in &track.children {
                ensure!(
                    bindings.iter().any(|peer| {
                        peer.layer == binding.layer
                            && peer.property == *child
                            && peer
                                .timeline
                                .as_ref()
                                .and_then(|track| track.parent.as_deref())
                                == Some(binding.property.as_str())
                    }),
                    invalid("timeline child does not link back to its owner")
                );
            }
            let Some(parent) = &track.parent else {
                return Ok(Some(index));
            };
            let owner = bindings.iter().position(|peer| {
                peer.layer == binding.layer
                    && peer.property == *parent
                    && peer
                        .timeline
                        .as_ref()
                        .is_some_and(|track| track.parent.is_none())
            });
            Ok(Some(owner.ok_or_else(|| {
                invalid("linked timeline requires a root track on the same layer")
            })?))
        })
        .collect()
}

impl PreparedScripts {
    pub(super) fn prepare(
        bindings: Vec<Binding>,
        layers: Vec<LayerState>,
        properties: Value,
    ) -> Result<Self> {
        if bindings.is_empty() || bindings.len() > MAX_BINDINGS {
            return Err(blocked("dynamic script binding count exceeds its budget"));
        }
        if layers.is_empty() || layers.len() > MAX_DYNAMIC_LAYERS {
            return Err(blocked("dynamic script layer count exceeds its budget"));
        }
        let mut handles = BTreeSet::new();
        for layer in &layers {
            validate_layer(layer)?;
            if !handles.insert(layer.handle) || layer.handle > 1_000_000 {
                return Err(blocked("invalid or duplicate dynamic layer handle"));
            }
        }
        let properties_map = properties
            .as_object()
            .ok_or_else(|| blocked("dynamic script properties require an object"))?;
        if properties_map.len() > 256 {
            return Err(blocked("dynamic script property count exceeds its budget"));
        }
        for (name, value) in properties_map {
            if !safe_name(name) {
                return Err(blocked("invalid dynamic script property name"));
            }
            match value {
                Value::Bool(_) => {}
                Value::Number(number) if number.as_f64().is_some_and(finite) => {}
                Value::String(text) if text.len() <= 4096 => {}
                Value::Array(values)
                    if values.len() == 3
                        && values
                            .iter()
                            .all(|value| value.as_f64().is_some_and(finite)) => {}
                _ => return Err(blocked("unsupported dynamic script property value")),
            }
        }
        let mut total_bytes = 0_usize;
        let mut media = false;
        for binding in &bindings {
            if !handles.contains(&binding.layer)
                || !matches!(
                    binding.property.as_str(),
                    "visible" | "alpha" | "origin" | "scale" | "angles" | "color" | "text"
                )
            {
                return Err(blocked("unsupported dynamic script layer binding"));
            }
            if binding.property == "text"
                && !layers
                    .iter()
                    .any(|layer| layer.handle == binding.layer && layer.text.is_some())
            {
                return Err(blocked("text binding requires a prepared text layer"));
            }
            validate_binding_input(binding)?;
            let overrides = binding
                .script_properties
                .as_object()
                .ok_or_else(|| blocked("script properties must be a bounded object"))?;
            if overrides.len() > 32
                || overrides.iter().any(|(name, value)| {
                    !safe_name(name)
                        || !(value.is_boolean()
                            || value.as_f64().is_some_and(finite)
                            || value.as_str().is_some_and(|text| text.len() <= 4096))
                })
            {
                return Err(blocked("invalid script property overrides"));
            }
            total_bytes = total_bytes
                .checked_add(binding.source.len())
                .ok_or_else(|| blocked("dynamic script source budget overflow"))?;
            if total_bytes > MAX_TOTAL_SOURCE_BYTES {
                return Err(blocked(
                    "dynamic script sources exceed their combined budget",
                ));
            }
            media |= validate_source(
                &binding.source,
                properties_map.keys().cloned().collect(),
                binding.timeline.is_some(),
            )?;
        }
        let mut prepared = Self {
            timeline_owners: timeline_owners(&bindings)?,
            bindings,
            layers,
            properties,
            created: Vec::new(),
            initial: Vec::new(),
            audio: false,
            media,
        };
        let stop = Arc::new(AtomicBool::new(false));
        let (mut vm, initialization) = Vm::new(&prepared, stop)?;
        prepared.created = initialization.created;
        prepared.audio = initialization.audio;
        prepared.initial = vm.frame(0.0, [0.0; 64])?;
        Ok(prepared)
    }

    pub(super) fn created_layers(&self) -> &[CreatedLayer] {
        &self.created
    }

    pub(super) fn initial_states(&self) -> &[LayerState] {
        &self.initial
    }

    pub(super) fn uses_audio(&self) -> bool {
        self.audio
    }
}

fn finite(value: f64) -> bool {
    value.is_finite() && (value as f32).is_finite()
}

fn validate_layer(layer: &LayerState) -> Result<()> {
    if !layer
        .origin
        .iter()
        .chain(&layer.scale)
        .chain(&layer.angles)
        .chain(&layer.color)
        .chain(std::iter::once(&layer.alpha))
        .all(|value| value.is_finite())
        || !(0.0..=1.0).contains(&layer.alpha)
        || layer
            .text
            .as_ref()
            .is_some_and(|text| text.len() > 4096 || text.chars().count() > 256)
        || (layer.text.is_none() && (layer.angles[0] != 0.0 || layer.angles[1] != 0.0))
    {
        return Err(blocked("invalid or unsupported dynamic layer transform"));
    }
    Ok(())
}

fn validate_binding_input(binding: &Binding) -> Result<()> {
    let valid = match binding.property.as_str() {
        "visible" => binding.input.is_boolean(),
        "text" => binding
            .input
            .as_str()
            .is_some_and(|text| text.len() <= 4096 && text.chars().count() <= 256),
        "alpha" => binding
            .input
            .as_f64()
            .is_some_and(|number| finite(number) && (0.0..=1.0).contains(&number)),
        _ => binding.input.as_array().is_some_and(|values| {
            values.len() == 3
                && values
                    .iter()
                    .all(|value| value.as_f64().is_some_and(finite))
        }),
    };
    if !valid {
        return Err(blocked("invalid dynamic script binding input"));
    }
    Ok(())
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && !matches!(name, "constructor" | "prototype" | "__proto__")
        && !name.starts_with("__lwe")
}

// This is a source-language gate, not a JavaScript translator. QuickJS executes
// the original module. New syntax or host capabilities must be admitted here
// deliberately, rather than surviving preparation as an ignored property.
const MEDIA_PLAYBACK: &str = "mediaPlaybackChanged";
const MEDIA_PROPERTIES: &str = "mediaPropertiesChanged";

fn validate_source(source: &str, properties: BTreeSet<String>, has_timeline: bool) -> Result<bool> {
    if source.is_empty()
        || source.len() > MAX_SOURCE_BYTES
        || source
            .bytes()
            .filter(|byte| b"{}()[]".contains(byte))
            .count()
            > 4096
    {
        return Err(blocked("dynamic script source exceeds its parsing budget"));
    }
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::mjs()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(blocked("invalid dynamic ECMAScript module"));
    }
    let mut validation = Validation {
        names: BTreeSet::new(),
        numeric_names: BTreeSet::new(),
        non_numeric_names: BTreeSet::new(),
        properties,
        script_fields: BTreeSet::new(),
        numeric_script_fields: BTreeSet::new(),
        audio_names: BTreeSet::new(),
        non_audio_names: BTreeSet::new(),
        declaring_builder: false,
        playback_callback: false,
        media_parameter: None,
    };
    let mut callbacks = BTreeSet::new();
    for statement in &parsed.program.body {
        match statement {
            Statement::VariableDeclaration(declaration) => {
                validation.declarations(declaration)?;
            }
            Statement::ExportNamedDeclaration(export)
                if export.source.is_none() && export.specifiers.is_empty() =>
            {
                if let Some(Declaration::VariableDeclaration(declaration)) = &export.declaration {
                    if declaration.declarations.len() != 1 {
                        return Err(blocked("unsupported script export"));
                    }
                    let item = &declaration.declarations[0];
                    let BindingPatternKind::BindingIdentifier(id) = &item.id.kind else {
                        return Err(blocked("unsupported script export"));
                    };
                    if id.name == "scriptProperties" {
                        if validation.names.contains("scriptProperties")
                            || !matches!(&item.init,Some(Expression::CallExpression(call)) if matches!(&call.callee,Expression::StaticMemberExpression(member) if member.property.name=="finish") && call.arguments.is_empty())
                        {
                            return Err(blocked("script builder must finish exactly once"));
                        }
                        validation.builder_fields(
                            item.init
                                .as_ref()
                                .ok_or_else(|| blocked("missing script builder"))?,
                        )?;
                    } else if id.name != "__workshopId"
                        || !matches!(&item.init, Some(Expression::StringLiteral(value)) if value.value.len() <= 256)
                    {
                        return Err(blocked(
                            "only builder and inert workshop metadata may be exported",
                        ));
                    }
                    validation.declaring_builder = id.name == "scriptProperties";
                    validation.declarations(declaration)?;
                    validation.declaring_builder = false;
                    continue;
                }
                let Some(Declaration::FunctionDeclaration(function)) = &export.declaration else {
                    return Err(blocked("only SceneScript callbacks may be exported"));
                };
                let name = function
                    .id
                    .as_ref()
                    .map(|name| name.name.as_str())
                    .ok_or_else(|| blocked("dynamic callback requires a name"))?;
                if !matches!(
                    name,
                    "init" | "update" | "applyUserProperties" | MEDIA_PLAYBACK | MEDIA_PROPERTIES
                ) || !callbacks.insert(name)
                    || function.r#async
                    || function.generator
                    || function.params.rest.is_some()
                    || function.body.is_none()
                    || function.params.items.len() > 1
                {
                    return Err(blocked("unsupported dynamic script callback"));
                }
                for parameter in &function.params.items {
                    validation.binding(&parameter.pattern.kind)?;
                    if let BindingPatternKind::BindingIdentifier(identifier) =
                        &parameter.pattern.kind
                    {
                        validation
                            .non_numeric_names
                            .insert(identifier.name.to_string());
                        validation.audio_names.remove(identifier.name.as_str());
                        validation
                            .non_audio_names
                            .insert(identifier.name.to_string());
                        validation.numeric_names.remove(identifier.name.as_str());
                    }
                }
                for statement in &function
                    .body
                    .as_ref()
                    .expect("checked callback body")
                    .statements
                {
                    validation.collect(statement, 0)?;
                }
            }
            _ => return Err(blocked("unsupported dynamic module statement or import")),
        }
    }
    let media = callbacks.contains(MEDIA_PLAYBACK) || callbacks.contains(MEDIA_PROPERTIES);
    if !callbacks.contains("update") && !media {
        return Err(blocked("dynamic script requires an update callback"));
    }
    for statement in &parsed.program.body {
        match statement {
            Statement::VariableDeclaration(declaration) => {
                validation.variable_expressions(declaration, 0)?;
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(Declaration::VariableDeclaration(declaration)) = &export.declaration {
                    validation.declaring_builder = matches!(
                        &declaration.declarations[0].id.kind,
                        BindingPatternKind::BindingIdentifier(id) if id.name == "scriptProperties"
                    );
                    let result = validation.variable_expressions(declaration, 0);
                    validation.declaring_builder = false;
                    result?;
                    continue;
                }
                let Some(Declaration::FunctionDeclaration(function)) = &export.declaration else {
                    unreachable!("declarations validated above")
                };
                validation.playback_callback = has_timeline;
                let callback_name = function
                    .id
                    .as_ref()
                    .expect("validated callback name")
                    .name
                    .as_str();
                if matches!(callback_name, MEDIA_PLAYBACK | MEDIA_PROPERTIES)
                    && let Some(parameter) = function.params.items.first()
                    && let BindingPatternKind::BindingIdentifier(identifier) =
                        &parameter.pattern.kind
                {
                    validation.media_parameter =
                        Some((identifier.name.to_string(), callback_name == MEDIA_PLAYBACK));
                }
                for statement in &function.body.as_ref().expect("checked body").statements {
                    validation.statement(statement, 0)?;
                }
                validation.playback_callback = false;
                validation.media_parameter = None;
            }
            _ => unreachable!("module statements validated above"),
        }
    }
    Ok(media)
}

struct Validation {
    names: BTreeSet<String>,
    numeric_names: BTreeSet<String>,
    non_numeric_names: BTreeSet<String>,
    properties: BTreeSet<String>,
    script_fields: BTreeSet<String>,
    numeric_script_fields: BTreeSet<String>,
    audio_names: BTreeSet<String>,
    non_audio_names: BTreeSet<String>,
    declaring_builder: bool,
    playback_callback: bool,
    media_parameter: Option<(String, bool)>,
}

fn literal_script_metadata(expression: &Expression<'_>, depth: usize) -> bool {
    match expression {
        Expression::StringLiteral(text) => text.value.len() <= 4096,
        Expression::ObjectExpression(object) if depth < 3 => {
            let mut fields = BTreeSet::new();
            object.properties.iter().all(|property| {
                let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property) = property else {
                    return false;
                };
                property
                    .key
                    .static_name()
                    .is_some_and(|name| fields.insert(name))
                    && literal_script_metadata(&property.value, depth + 1)
            })
        }
        Expression::ArrayExpression(array) if depth == 1 && array.elements.len() <= 32 => {
            array.elements.iter().all(|element| {
                element
                    .as_expression()
                    .is_some_and(|expression| literal_script_metadata(expression, depth + 1))
            })
        }
        _ => false,
    }
}

impl Validation {
    fn builder_fields(&mut self, expression: &Expression<'_>) -> Result<()> {
        let Expression::CallExpression(call) = expression else {
            return Err(blocked("invalid script builder chain"));
        };
        if let Expression::Identifier(id) = &call.callee {
            return if id.name == "createScriptProperties" && call.arguments.is_empty() {
                Ok(())
            } else {
                Err(blocked("invalid script builder root"))
            };
        }
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Err(blocked("invalid script builder method"));
        };
        self.builder_fields(&member.object)?;
        let method = member.property.name.as_str();
        if method == "finish" && call.arguments.is_empty() {
            return Ok(());
        }
        if !matches!(method, "addSlider" | "addCheckbox" | "addText" | "addCombo")
            || call.arguments.len() != 1
        {
            return Err(blocked("unsupported script builder method"));
        }
        let Some(Expression::ObjectExpression(object)) = call.arguments[0].as_expression() else {
            return Err(blocked("script builder requires literal metadata"));
        };
        let mut name = None;
        for property in &object.properties {
            let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property) = property else {
                return Err(blocked("script builder spread unsupported"));
            };
            if property.key.static_name().as_deref() == Some("name")
                && let Expression::StringLiteral(value) = &property.value
            {
                name = Some(value.value.to_string());
            }
        }
        let name = name
            .filter(|name| safe_name(name))
            .ok_or_else(|| blocked("script builder needs a literal safe name"))?;
        if !self.script_fields.insert(name.clone()) || self.script_fields.len() > 32 {
            return Err(blocked("duplicate or excessive script properties"));
        }
        if method == "addSlider" {
            self.numeric_script_fields.insert(name);
        }
        Ok(())
    }

    fn reduce_callback(&self, expression: &Expression<'_>) -> Result<()> {
        let Expression::ArrowFunctionExpression(arrow) = expression else {
            return Err(blocked("reduce requires a numeric sum callback"));
        };
        if arrow.r#async
            || !arrow.expression
            || arrow.params.rest.is_some()
            || arrow.params.items.len() != 2
            || arrow.body.statements.len() != 1
        {
            return Err(blocked("unsupported reduce callback"));
        }
        let mut names = Vec::new();
        for parameter in &arrow.params.items {
            let BindingPatternKind::BindingIdentifier(id) = &parameter.pattern.kind else {
                return Err(blocked("unsupported reduce parameter"));
            };
            if !safe_name(id.name.as_str())
                || matches!(
                    id.name.as_str(),
                    "engine"
                        | "thisLayer"
                        | "thisScene"
                        | "Vec3"
                        | "Math"
                        | "createScriptProperties"
                )
            {
                return Err(blocked("reduce parameter shadows capability"));
            }
            names.push(id.name.as_str());
        }
        let Statement::ExpressionStatement(body) = &arrow.body.statements[0] else {
            return Err(blocked("unsupported reduce body"));
        };
        let Expression::BinaryExpression(sum) = &body.expression else {
            return Err(blocked("reduce must sum two parameters"));
        };
        if sum.operator.as_str() != "+"
            || !matches!(&sum.left,Expression::Identifier(id) if id.name==names[0])
            || !matches!(&sum.right,Expression::Identifier(id) if id.name==names[1])
        {
            return Err(blocked("reduce must sum two parameters"));
        }
        Ok(())
    }

    fn depth(&self, depth: usize) -> Result<()> {
        if depth > MAX_AST_DEPTH {
            return Err(blocked("dynamic script AST exceeds its nesting budget"));
        }
        Ok(())
    }

    fn binding(&mut self, pattern: &BindingPatternKind<'_>) -> Result<()> {
        let BindingPatternKind::BindingIdentifier(identifier) = pattern else {
            return Err(blocked("dynamic destructuring bindings are not supported"));
        };
        let name = identifier.name.as_str();
        if !safe_name(name)
            || (name == "scriptProperties" && !self.declaring_builder)
            || matches!(
                name,
                "engine"
                    | "thisScene"
                    | "thisLayer"
                    | "thisObject"
                    | "MediaPlaybackEvent"
                    | "Vec3"
                    | "Math"
                    | "undefined"
                    | "createScriptProperties"
            )
        {
            return Err(blocked("dynamic binding shadows a host capability"));
        }
        self.names.insert(name.to_owned());
        if self.names.len() > 256 {
            return Err(blocked("dynamic binding count exceeds its budget"));
        }
        Ok(())
    }

    fn declarations(&mut self, declaration: &VariableDeclaration<'_>) -> Result<()> {
        for declarator in &declaration.declarations {
            self.binding(&declarator.id.kind)?;
            if let BindingPatternKind::BindingIdentifier(identifier) = &declarator.id.kind {
                let name = identifier.name.as_str();
                let is_audio = declaration.kind == oxc_ast::ast::VariableDeclarationKind::Const
                    && matches!(&declarator.init,Some(Expression::CallExpression(call)) if matches!(&call.callee,Expression::StaticMemberExpression(member) if matches!(&member.object,Expression::Identifier(id) if id.name=="engine") && member.property.name=="registerAudioBuffers"));
                if is_audio && !self.non_audio_names.contains(name) {
                    self.audio_names.insert(name.to_owned());
                } else {
                    self.audio_names.remove(name);
                    self.non_audio_names.insert(name.to_owned());
                }
                if declarator
                    .init
                    .as_ref()
                    .is_some_and(|value| self.numeric_expression(value))
                    && !self.non_numeric_names.contains(name)
                {
                    self.numeric_names.insert(name.to_owned());
                } else {
                    self.numeric_names.remove(name);
                    self.non_numeric_names.insert(name.to_owned());
                }
            }
        }
        Ok(())
    }

    fn collect(&mut self, statement: &Statement<'_>, depth: usize) -> Result<()> {
        self.depth(depth)?;
        match statement {
            Statement::VariableDeclaration(declaration) => self.declarations(declaration),
            Statement::BlockStatement(block) => {
                for statement in &block.body {
                    self.collect(statement, depth + 1)?;
                }
                Ok(())
            }
            Statement::ForStatement(statement) => {
                if let Some(ForStatementInit::VariableDeclaration(declaration)) = &statement.init {
                    self.declarations(declaration)?;
                }
                self.collect(&statement.body, depth + 1)
            }
            Statement::WhileStatement(statement) => self.collect(&statement.body, depth + 1),
            Statement::IfStatement(statement) => {
                self.collect(&statement.consequent, depth + 1)?;
                if let Some(alternate) = &statement.alternate {
                    self.collect(alternate, depth + 1)?;
                }
                Ok(())
            }
            Statement::ExpressionStatement(_)
            | Statement::ReturnStatement(_)
            | Statement::EmptyStatement(_)
            | Statement::BreakStatement(_)
            | Statement::ContinueStatement(_) => Ok(()),
            _ => Err(blocked("unsupported dynamic script statement")),
        }
    }

    fn variable_expressions(
        &self,
        declaration: &VariableDeclaration<'_>,
        depth: usize,
    ) -> Result<()> {
        for declarator in &declaration.declarations {
            if let Some(expression) = &declarator.init {
                self.expression(expression, depth + 1)?;
            }
        }
        Ok(())
    }

    fn statement(&self, statement: &Statement<'_>, depth: usize) -> Result<()> {
        self.depth(depth)?;
        match statement {
            Statement::VariableDeclaration(declaration) => {
                self.variable_expressions(declaration, depth)
            }
            Statement::BlockStatement(block) => {
                for statement in &block.body {
                    self.statement(statement, depth + 1)?;
                }
                Ok(())
            }
            Statement::ExpressionStatement(statement) => {
                self.expression(&statement.expression, depth + 1)
            }
            Statement::ReturnStatement(statement) => {
                if let Some(expression) = &statement.argument {
                    self.expression(expression, depth + 1)?;
                }
                Ok(())
            }
            Statement::ForStatement(statement) => {
                if let Some(init) = &statement.init {
                    match init {
                        ForStatementInit::VariableDeclaration(declaration) => {
                            self.variable_expressions(declaration, depth + 1)?;
                        }
                        _ => self.expression(
                            init.as_expression().expect("expression for initializer"),
                            depth + 1,
                        )?,
                    }
                }
                if let Some(test) = &statement.test {
                    self.expression(test, depth + 1)?;
                }
                if let Some(update) = &statement.update {
                    self.expression(update, depth + 1)?;
                }
                self.statement(&statement.body, depth + 1)
            }
            Statement::WhileStatement(statement) => {
                self.expression(&statement.test, depth + 1)?;
                self.statement(&statement.body, depth + 1)
            }
            Statement::IfStatement(statement) => {
                self.expression(&statement.test, depth + 1)?;
                self.statement(&statement.consequent, depth + 1)?;
                if let Some(alternate) = &statement.alternate {
                    self.statement(alternate, depth + 1)?;
                }
                Ok(())
            }
            Statement::EmptyStatement(_) => Ok(()),
            Statement::BreakStatement(statement) if statement.label.is_none() => Ok(()),
            Statement::ContinueStatement(statement) if statement.label.is_none() => Ok(()),
            _ => Err(blocked("unsupported dynamic script statement")),
        }
    }

    fn identifier(&self, name: &str) -> Result<()> {
        if self.names.contains(name) || matches!(name, "undefined" | "thisLayer") {
            Ok(())
        } else {
            Err(blocked(format!("unsupported dynamic script global {name}")))
        }
    }

    fn member(&self, object: &Expression<'_>, property: &str, depth: usize) -> Result<()> {
        if !safe_name(property) {
            return Err(blocked("dynamic prototype access is not supported"));
        }
        if let Expression::Identifier(identifier) = object {
            if let Some((parameter, playback)) = &self.media_parameter {
                let allowed = match identifier.name.as_str() {
                    "MediaPlaybackEvent" if *playback => Some(matches!(
                        property,
                        "PLAYBACK_STOPPED" | "PLAYBACK_PLAYING" | "PLAYBACK_PAUSED"
                    )),
                    name if name == parameter => Some(if *playback {
                        property == "state"
                    } else {
                        matches!(
                            property,
                            "title" | "artist" | "albumTitle" | "albumArtist" | "genres"
                        )
                    }),
                    _ => None,
                };
                if let Some(allowed) = allowed {
                    ensure!(allowed, blocked("unsupported media event field"));
                    return Ok(());
                }
            }
            let allowed = match identifier.name.as_str() {
                "engine" => Some(matches!(
                    property,
                    "registerAudioBuffers"
                        | "frametime"
                        | "runtime"
                        | "AUDIO_RESOLUTION_16"
                        | "AUDIO_RESOLUTION_64"
                )),
                "scriptProperties" => Some(self.script_fields.contains(property)),
                "thisScene" => Some(property == "createLayer"),
                "thisLayer" => Some(matches!(
                    property,
                    "origin" | "scale" | "angles" | "color" | "alpha" | "visible" | "text"
                )),
                "Math" => Some(matches!(
                    property,
                    "abs"
                        | "sign"
                        | "sqrt"
                        | "floor"
                        | "ceil"
                        | "round"
                        | "trunc"
                        | "sin"
                        | "cos"
                        | "tan"
                        | "exp"
                        | "log"
                        | "pow"
                        | "min"
                        | "max"
                        | "PI"
                        | "E"
                )),
                _ => None,
            };
            if let Some(allowed) = allowed {
                return if allowed {
                    Ok(())
                } else {
                    Err(blocked(format!(
                        "unsupported SceneScript host member {property}"
                    )))
                };
            }
        }
        let known_member = matches!(
            property,
            "origin"
                | "scale"
                | "angles"
                | "color"
                | "alpha"
                | "visible"
                | "copy"
                | "x"
                | "y"
                | "z"
                | "push"
                | "unshift"
                | "reduce"
                | "addSlider"
                | "addCheckbox"
                | "finish"
                | "text"
                | "left"
                | "right"
                | "length"
                | "average"
                | "hasOwnProperty"
        ) || (self.declaring_builder
            && matches!(property, "addText" | "addCombo"));
        if !known_member && !self.properties.contains(property) {
            return Err(blocked(format!(
                "unsupported SceneScript member {property}"
            )));
        }
        self.expression(object, depth + 1)
    }

    fn computed(
        &self,
        object: &Expression<'_>,
        index: &Expression<'_>,
        depth: usize,
    ) -> Result<()> {
        // Numeric indexing admits arrays and audio buffers, but cannot become
        // a computed route to a host function or JavaScript prototype.
        let numeric = match index {
            Expression::NumericLiteral(number) => {
                number.value.fract() == 0.0
                    && (0.0..MAX_DYNAMIC_LAYERS as f64).contains(&number.value)
            }
            Expression::Identifier(identifier) => {
                self.numeric_names.contains(identifier.name.as_str())
            }
            Expression::StaticMemberExpression(member) => {
                matches!(&member.object,Expression::Identifier(id) if id.name=="scriptProperties")
                    && self
                        .numeric_script_fields
                        .contains(member.property.name.as_str())
            }
            _ => false,
        };
        if !numeric {
            return Err(blocked("dynamic member names are not supported"));
        }
        self.expression(object, depth + 1)?;
        self.expression(index, depth + 1)
    }

    fn target(&self, target: &AssignmentTarget<'_>, depth: usize) -> Result<()> {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                if identifier.name == "scriptProperties" {
                    return Err(blocked("script property snapshots are immutable"));
                }
                if self.names.contains(identifier.name.as_str()) {
                    Ok(())
                } else {
                    Err(blocked(
                        "dynamic assignments cannot replace a host capability",
                    ))
                }
            }
            AssignmentTarget::StaticMemberExpression(member) if !member.optional => {
                if let Expression::Identifier(identifier) = &member.object {
                    ensure!(
                        identifier.name != "MediaPlaybackEvent"
                            && self
                                .media_parameter
                                .as_ref()
                                .is_none_or(|(name, _)| name != identifier.name.as_str()),
                        blocked("media event members are immutable")
                    );
                }
                self.member(&member.object, member.property.name.as_str(), depth)
            }
            AssignmentTarget::ComputedMemberExpression(_) => Err(blocked(
                "dynamic indexed writes and audio buffer mutations are unsupported",
            )),
            _ => Err(blocked("unsupported dynamic assignment target")),
        }
    }

    fn expression(&self, expression: &Expression<'_>, depth: usize) -> Result<()> {
        self.depth(depth)?;
        match expression {
            Expression::NumericLiteral(number) if finite(number.value) => Ok(()),
            Expression::BooleanLiteral(_) | Expression::NullLiteral(_) => Ok(()),
            Expression::StringLiteral(text) if text.value.len() <= 4096 => Ok(()),
            Expression::Identifier(identifier) => self.identifier(identifier.name.as_str()),
            Expression::ArrayExpression(array) => {
                if array.elements.len() > MAX_DYNAMIC_LAYERS {
                    return Err(blocked("dynamic array literal exceeds its budget"));
                }
                for element in &array.elements {
                    let expression = element
                        .as_expression()
                        .ok_or_else(|| blocked("array spread and holes are not supported"))?;
                    self.expression(expression, depth + 1)?;
                }
                Ok(())
            }
            Expression::ObjectExpression(object) => {
                if object.properties.len() > 8 {
                    return Err(blocked("object literal exceeds metadata budget"));
                }
                for property in &object.properties {
                    let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property) = property
                    else {
                        return Err(blocked("object spread unsupported"));
                    };
                    let name = property
                        .key
                        .static_name()
                        .ok_or_else(|| blocked("dynamic object key unsupported"))?;
                    if !safe_name(&name)
                        || property.computed
                        || property.method
                        || property.shorthand
                        || property.kind != oxc_ast::ast::PropertyKind::Init
                    {
                        return Err(blocked("unsafe object literal"));
                    }
                    self.expression(&property.value, depth + 1)?;
                }
                Ok(())
            }
            Expression::ParenthesizedExpression(expression) => {
                self.expression(&expression.expression, depth + 1)
            }
            Expression::UnaryExpression(unary)
                if matches!(unary.operator.as_str(), "+" | "-" | "!" | "typeof") =>
            {
                self.expression(&unary.argument, depth + 1)
            }
            Expression::BinaryExpression(binary)
                if matches!(
                    binary.operator.as_str(),
                    "+" | "-"
                        | "|"
                        | "*"
                        | "/"
                        | "%"
                        | "**"
                        | "=="
                        | "!="
                        | "==="
                        | "!=="
                        | "<"
                        | ">"
                        | "<="
                        | ">="
                ) =>
            {
                self.expression(&binary.left, depth + 1)?;
                self.expression(&binary.right, depth + 1)
            }
            Expression::LogicalExpression(logical) => {
                self.expression(&logical.left, depth + 1)?;
                self.expression(&logical.right, depth + 1)
            }
            Expression::ConditionalExpression(conditional) => {
                self.expression(&conditional.test, depth + 1)?;
                self.expression(&conditional.consequent, depth + 1)?;
                self.expression(&conditional.alternate, depth + 1)
            }
            Expression::StaticMemberExpression(member) if !member.optional => {
                self.member(&member.object, member.property.name.as_str(), depth)
            }
            Expression::ComputedMemberExpression(member) if !member.optional => {
                self.computed(&member.object, &member.expression, depth)
            }
            Expression::AssignmentExpression(assignment) => {
                if let AssignmentTarget::AssignmentTargetIdentifier(identifier) = &assignment.left
                    && self.numeric_names.contains(identifier.name.as_str())
                    && !self.numeric_expression(&assignment.right)
                {
                    return Err(blocked(
                        "numeric array indices cannot acquire dynamic member names",
                    ));
                }
                self.target(&assignment.left, depth + 1)?;
                self.expression(&assignment.right, depth + 1)
            }
            Expression::UpdateExpression(update) => match &update.argument {
                SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                    if identifier.name == "scriptProperties" {
                        return Err(blocked("script property snapshots are immutable"));
                    }
                    self.identifier(identifier.name.as_str())
                }
                _ => Err(blocked(
                    "only local scalar increment and decrement are supported",
                )),
            },
            Expression::CallExpression(call) if !call.optional => {
                if let Expression::Identifier(id) = &call.callee {
                    return if id.name == "createScriptProperties" && call.arguments.is_empty() {
                        Ok(())
                    } else {
                        Err(blocked("unsupported dynamic function call"))
                    };
                }
                let Expression::StaticMemberExpression(member) = &call.callee else {
                    return Err(blocked(
                        "only explicit bounded host and value methods may be called",
                    ));
                };
                let method = member.property.name.as_str();
                if self.playback_callback
                    && method == "play"
                    && !member.optional
                    && call.arguments.is_empty()
                    && matches!(&member.object, Expression::CallExpression(getter)
                        if !getter.optional && getter.arguments.is_empty()
                            && matches!(&getter.callee, Expression::StaticMemberExpression(member)
                                if !member.optional && member.property.name == "getAnimation"
                                    && matches!(&member.object, Expression::Identifier(id) if id.name == "thisObject")))
                {
                    return Ok(());
                }
                let string_builder =
                    self.declaring_builder && matches!(method, "addText" | "addCombo");
                if string_builder
                    && !matches!(call.arguments.as_slice(), [argument] if argument.as_expression().is_some_and(|expression| literal_script_metadata(expression, 0)))
                {
                    return Err(blocked("string builder requires bounded literal metadata"));
                }
                let allowed_method = string_builder
                    || matches!(
                        method,
                        "registerAudioBuffers"
                            | "createLayer"
                            | "copy"
                            | "push"
                            | "unshift"
                            | "reduce"
                            | "addSlider"
                            | "addCheckbox"
                            | "finish"
                            | "hasOwnProperty"
                            | "abs"
                            | "sign"
                            | "sqrt"
                            | "floor"
                            | "ceil"
                            | "round"
                            | "trunc"
                            | "sin"
                            | "cos"
                            | "tan"
                            | "exp"
                            | "log"
                            | "pow"
                            | "min"
                            | "max"
                    );
                if !allowed_method || call.arguments.len() > 8 {
                    return Err(blocked("unsupported dynamic method call"));
                }
                self.member(&member.object, method, depth + 1)?;
                for (index, argument) in call.arguments.iter().enumerate() {
                    if method == "reduce" && index == 0 {
                        self.reduce_callback(
                            argument
                                .as_expression()
                                .ok_or_else(|| blocked("reduce spread unsupported"))?,
                        )?;
                        continue;
                    }
                    self.expression(
                        argument
                            .as_expression()
                            .ok_or_else(|| blocked("argument spread is unsupported"))?,
                        depth + 1,
                    )?;
                }
                Ok(())
            }
            Expression::NewExpression(constructor)
                if matches!(&constructor.callee, Expression::Identifier(identifier) if identifier.name == "Vec3")
                    && matches!(constructor.arguments.len(), 1 | 3) =>
            {
                for argument in &constructor.arguments {
                    self.expression(
                        argument
                            .as_expression()
                            .ok_or_else(|| blocked("constructor spread is unsupported"))?,
                        depth + 1,
                    )?;
                }
                Ok(())
            }
            _ => Err(blocked(
                "expression is outside the bounded dynamic SceneScript subset",
            )),
        }
    }

    fn numeric_expression(&self, expression: &Expression<'_>) -> bool {
        match expression {
            Expression::NumericLiteral(_) => true,
            Expression::Identifier(identifier) => {
                self.numeric_names.contains(identifier.name.as_str())
            }
            Expression::StaticMemberExpression(member) => match &member.object {
                Expression::Identifier(id) if id.name == "engine" => matches!(
                    member.property.name.as_str(),
                    "frametime" | "runtime" | "AUDIO_RESOLUTION_16" | "AUDIO_RESOLUTION_64"
                ),
                Expression::Identifier(id) if id.name == "scriptProperties" => self
                    .numeric_script_fields
                    .contains(member.property.name.as_str()),
                _ => false,
            },
            Expression::ComputedMemberExpression(member) => {
                matches!(&member.object,Expression::StaticMemberExpression(buffer) if matches!(&buffer.object,Expression::Identifier(id) if self.audio_names.contains(id.name.as_str())) && matches!(buffer.property.name.as_str(),"average"|"left"|"right"))
            }
            Expression::CallExpression(call) => {
                matches!(&call.callee,Expression::StaticMemberExpression(member) if matches!(&member.object,Expression::Identifier(id) if id.name=="Math"))
                    && call.arguments.iter().all(|arg| {
                        arg.as_expression()
                            .is_some_and(|value| self.numeric_expression(value))
                    })
            }
            Expression::UnaryExpression(expression) => {
                matches!(expression.operator.as_str(), "+" | "-")
                    && self.numeric_expression(&expression.argument)
            }
            Expression::BinaryExpression(expression) => {
                matches!(
                    expression.operator.as_str(),
                    "+" | "-" | "*" | "/" | "%" | "**" | "|"
                ) && self.numeric_expression(&expression.left)
                    && self.numeric_expression(&expression.right)
            }
            Expression::ParenthesizedExpression(expression) => {
                self.numeric_expression(&expression.expression)
            }
            _ => false,
        }
    }
}

// The closure keeps mutable host state out of the author's globals. Its
// accessors copy vectors both ways, matching native value semantics when an
// author reuses one Vec3 while assigning many layers inside a loop.
const HOST: &str = r#"
(() => {
    'use strict';
    const functionPrototype = Function.prototype;
    const hasOwn = (object,key) => Object.prototype.hasOwnProperty.call(object,key);
    Object.defineProperty(functionPrototype, 'constructor', {value: undefined, writable: false, configurable: false});
    Object.defineProperty(globalThis, 'eval', {value: undefined, writable: false, configurable: false});
    Object.defineProperty(globalThis, 'Function', {value: undefined, writable: false, configurable: false});
    for (const method of ['push','unshift']) {
        const original = Array.prototype[method];
        Object.defineProperty(Array.prototype, method, {value:function(...values) {
            if (this.length + values.length > 128) throw new RangeError('bounded script array exceeded');
            return original.apply(this, values);
        }, writable:false, configurable:false});
    }
    for (const prototype of [Object.prototype, Array.prototype, Map.prototype, Number.prototype, String.prototype, functionPrototype,
        Float32Array.prototype, Object.getPrototypeOf(Float32Array.prototype)]) Object.freeze(prototype);
    Object.freeze(Math);
    const layers = new Map();
    const created = [];
    let nextHandle = 0;
    let initializing = false;
    let usesAudio = false;
    let registration = false;
    let runtimeTime=0, frameTime=0;
    let canPlay=false, playRequested=false;
    let scriptOverrides={}, builderFinished=false, builderCount=0;
    const audioBuffers = new Map();
    for (const count of [16,64]) {
        const item={left:new Float32Array(count),right:new Float32Array(count),average:new Float32Array(count)};
        for (const array of Object.values(item)) Object.preventExtensions(array);
        audioBuffers.set(count,Object.freeze(item));
    }
    const finite = v => typeof v === 'number' && Number.isFinite(v) && Math.abs(v) <= 3.4028234663852886e38;
    class Vec3 {
        constructor(x, y, z) {
            if (arguments.length === 1) { y = x; z = x; }
            if (!finite(x) || !finite(y) || !finite(z)) throw new TypeError('invalid Vec3');
            this.x = x; this.y = y; this.z = z;
            Object.preventExtensions(this);
        }
        copy() { return new Vec3(this.x, this.y, this.z); }
    }
    Object.freeze(Vec3.prototype);
    Object.freeze(Vec3);
    const vector = v => {
        if (!v || !finite(v.x) || !finite(v.y) || !finite(v.z)) throw new TypeError('invalid layer vector');
        return [v.x, v.y, v.z];
    };
    const copy = v => new Vec3(v[0], v[1], v[2]);
    const makeLayer = state => {
        const layer = {};
        const authoredXY=[state.angles[0],state.angles[1]];
        for (const key of ['origin', 'scale', 'angles', 'color']) {
            Object.defineProperty(layer, key, {
                enumerable: true,
                get: () => copy(state[key]),
                set: value => {
                    const parsed = vector(value);
                    if (key === 'angles' && (parsed[0] !== authoredXY[0] || parsed[1] !== authoredXY[1])) throw new TypeError('changing authored planar rotation unsupported');
                    state[key] = parsed;
                }
            });
        }
        Object.defineProperty(layer, 'visible', {
            enumerable: true, get: () => state.visible,
            set: value => { if (typeof value !== 'boolean') throw new TypeError('invalid layer visibility'); state.visible = value; }
        });
        Object.defineProperty(layer, 'alpha', {
            enumerable: true, get: () => state.alpha,
            set: value => { if (!finite(value) || value < 0 || value > 1) throw new TypeError('invalid layer alpha'); state.alpha = value; }
        });
        if (typeof state.text === 'string') Object.defineProperty(layer, 'text', {
            enumerable:true, get:()=>state.text,
            set:value=>{ if(typeof value!=='string' || value.length>4096 || [...value].length>256) throw new TypeError('invalid bounded layer text'); state.text=value; }
        });
        Object.preventExtensions(layer);
        layers.set(state.handle, {state, layer});
        nextHandle = Math.max(nextHandle, state.handle + 1);
        return layer;
    };
    const engine = Object.freeze({
        get frametime(){return frameTime;}, get runtime(){return runtimeTime;},
        AUDIO_RESOLUTION_16:16, AUDIO_RESOLUTION_64:64,
        registerAudioBuffers: count => {
            if (!registration) throw new TypeError('audio registration is only supported at module global scope');
            if (!audioBuffers.has(count)) throw new TypeError('only 16/64 audio bands are supported');
            usesAudio = true;
            return audioBuffers.get(count);
        }
    });
    const createScriptProperties=()=>{
        if (!registration || ++builderCount>32) throw new TypeError('bounded builders require module scope');
        const values={};
        let done=false;
        const add=(spec,kind)=>{
            if(done || !spec || typeof spec!=='object' || Array.isArray(spec) || Object.keys(values).length>=32) throw new TypeError('invalid builder state');
            const allowed=kind==='slider'?['name','label','value','min','max','integer']:
                ['name','label',kind==='combo'?'options':'value'];
            if(Object.keys(spec).some(k=>!allowed.includes(k)) || typeof spec.name!=='string' || !spec.name || spec.name.length>256 || ['constructor','prototype','__proto__'].includes(spec.name) || spec.name.startsWith('__lwe') || hasOwn(values,spec.name) || typeof spec.label!=='string' || spec.label.length>4096) throw new TypeError('invalid property metadata');
            let fallback=spec.value;
            let choices;
            if(kind==='combo'){
                if(!Array.isArray(spec.options) || spec.options.length<1 || spec.options.length>32) throw new TypeError('invalid combo options');
                choices=new Set();
                for(const option of spec.options){
                    if(!option || typeof option!=='object' || Array.isArray(option) || Object.keys(option).length!==2 || !hasOwn(option,'label') || !hasOwn(option,'value') || typeof option.label!=='string' || option.label.length>4096 || typeof option.value!=='string' || option.value.length>256 || choices.has(option.value)) throw new TypeError('invalid combo option');
                    choices.add(option.value);
                }
                fallback=spec.options[0].value;
            }
            const selected=hasOwn(scriptOverrides,spec.name)?scriptOverrides[spec.name]:fallback;
            if(kind==='slider'){
                if(!finite(spec.min)||!finite(spec.max)||spec.min>spec.max||typeof spec.integer!=='boolean'||!finite(spec.value)||spec.value<spec.min||spec.value>spec.max||!finite(selected)||selected<spec.min||selected>spec.max||(spec.integer && (!Number.isInteger(spec.value)||!Number.isInteger(selected)))) throw new TypeError('invalid slider value/range');
            }else if(kind==='checkbox'){
                if(typeof spec.value!=='boolean'||typeof selected!=='boolean') throw new TypeError('invalid checkbox value');
            }else if(kind==='text'){
                if(typeof fallback!=='string'||fallback.length>4096||typeof selected!=='string'||selected.length>4096) throw new TypeError('invalid text value');
            }else if(!choices.has(selected)) throw new TypeError('invalid combo value');
            values[spec.name]=selected;
            return builder;
        };
        const builder=Object.freeze({addSlider:spec=>add(spec,'slider'),addCheckbox:spec=>add(spec,'checkbox'),
          addText:spec=>add(spec,'text'),addCombo:spec=>add(spec,'combo'),finish:()=>{
            if(done || Object.keys(scriptOverrides).some(key=>!hasOwn(values,key))) throw new TypeError('invalid builder overrides');
            done=true; builderFinished=true; return Object.freeze(values);
        }});
        return builder;
    };
    Object.defineProperty(globalThis,'createScriptProperties',{value:createScriptProperties});
    const thisScene = Object.freeze({createLayer: path => {
        if (!initializing) throw new TypeError('layer creation is only supported during init');
        if (typeof path !== 'string' || path.length === 0 || path.length > 1024 || !path.startsWith('models/') || !path.endsWith('.json')
            || ![...path].every(char => 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_./ -'.includes(char))
            || path.split('/').some(part => part === '' || part === '.' || part === '..')) throw new TypeError('unsafe image model resource path');
        if (layers.size >= 128) throw new RangeError('dynamic layer budget exceeded');
        const state = {handle: nextHandle, origin: [0,0,0], scale: [1,1,1], angles: [0,0,0], color: [1,1,1], alpha: 1, visible: true};
        const layer = makeLayer(state);
        created.push({path, state});
        return layer;
    }});
    Object.defineProperty(globalThis, 'Vec3', {value: Vec3});
    Object.defineProperty(globalThis, 'engine', {value: engine});
    Object.defineProperty(globalThis, 'thisScene', {value: thisScene});
    const animation = Object.freeze({play: () => {
        if (!canPlay) throw new TypeError('no property timeline');
        playRequested = true;
    }});
    Object.defineProperty(globalThis, 'thisObject', {value: Object.freeze({getAnimation: () => animation})});
    Object.defineProperty(globalThis, 'MediaPlaybackEvent', {value: Object.freeze({
        PLAYBACK_STOPPED:0, PLAYBACK_PLAYING:1, PLAYBACK_PAUSED:2
    })});
    return Object.freeze({
        load: values => { for (const state of values) makeLayer(state); },
        owner: (handle, playable) => {
            const item = layers.get(handle);
            if (!item) throw new TypeError('unknown script owner layer');
            globalThis.thisLayer = item.layer;
            canPlay = playable;
            playRequested = false;
        },
        playRequested: () => playRequested,
        mediaEvent: value => Object.freeze(value),
        initialize: value => { initializing = value; },
        configure: values => { scriptOverrides=values; builderFinished=false; },
        register: value => { registration = value; },
        finishModule: () => { if(Object.keys(scriptOverrides).length && !builderFinished) throw new TypeError('overrides require a finished builder'); },
        frame: (time,delta,values) => {
            registration = false; runtimeTime=time; frameTime=delta;
            for (const count of [16,64]) {
                const item=audioBuffers.get(count);
                const left=count===16?values.left:values.script_left;
                const right=count===16?values.right:values.script_right;
                for(let i=0;i<count;++i){item.left[i]=left[i];item.right[i]=right[i];item.average[i]=count===16?(left[i]+right[i])*0.5:values.script_average[i];}
            }
        },
        input: (handle, property) => layers.get(handle).layer[property],
        timelineVector: (handle, property, x, y, z) => {
            layers.get(handle).layer[property] = new Vec3(x,y,z);
        },
        properties: values => {
            for (const key of Object.keys(values)) {
                if (Array.isArray(values[key])) values[key] = copy(values[key]);
            }
            return values;
        },
        output: (handle, property, value) => {
            if (value !== undefined) layers.get(handle).layer[property] = value;
        },
        snapshot: () => JSON.stringify({states: [...layers.values()].map(item => item.state), created, audio: usesAudio})
    });
})()
"#;

#[derive(Deserialize)]
struct Snapshot {
    states: Vec<LayerState>,
    created: Vec<CreatedLayer>,
    audio: bool,
}

struct Vm {
    // Persistent JS values must be destroyed before their context/runtime.
    modules: Vec<Persistent<Object<'static>>>,
    host: Persistent<Object<'static>>,
    context: Context,
    _runtime: Runtime,
    deadline: Rc<Cell<Instant>>,
    bindings: Vec<Binding>,
    timeline_owners: Vec<Option<usize>>,
    timeline_starts: Vec<Option<f64>>,
    base_handles: BTreeSet<usize>,
    last_time: f64,
    last_media: Option<MediaSnapshot>,
}

fn play(track: &PropertyTimeline, start: &mut Option<f64>, time: f64) {
    if start.is_none_or(|start| track.clock.finished(time - start)) {
        *start = Some(time);
    }
}

impl Vm {
    fn new(plan: &PreparedScripts, stop: Arc<AtomicBool>) -> Result<(Self, Snapshot)> {
        let base_handles = plan
            .layers
            .iter()
            .map(|layer| layer.handle)
            .collect::<BTreeSet<_>>();
        let runtime = Runtime::new().map_err(|_| blocked("cannot initialize dynamic script VM"))?;
        runtime.set_memory_limit(MEMORY_BYTES);
        runtime.set_max_stack_size(STACK_BYTES);
        let deadline = Rc::new(Cell::new(Instant::now() + INITIALIZATION_LIMIT));
        let callback_deadline = Rc::clone(&deadline);
        runtime.set_interrupt_handler(Some(Box::new(move || {
            stop.load(Ordering::Acquire) || Instant::now() >= callback_deadline.get()
        })));
        // QuickJS needs Eval internally to compile the host and modules. HOST
        // removes author-accessible eval/Function/constructor routes before any
        // author code runs. Date, Proxy, external I/O and loaders are absent.
        // Promise supports module evaluation; author async/await is rejected.
        let context = Context::custom::<(
            intrinsic::Eval,
            intrinsic::Promise,
            intrinsic::Json,
            intrinsic::MapSet,
            intrinsic::TypedArrays,
        )>(&runtime)
        .map_err(|_| blocked("cannot initialize dynamic script context"))?;
        let mut timeline_starts: Vec<_> = plan
            .bindings
            .iter()
            .map(|binding| {
                binding
                    .timeline
                    .as_ref()
                    .filter(|track| !track.clock.start_paused)
                    .map(|_| 0.0)
            })
            .collect();
        let (host, modules, initialization) = context.with(|ctx| -> Result<_> {
            let host: Object = ctx.eval(HOST).map_err(|_| {
                let exception = ctx.catch();
                let message = exception
                    .as_object()
                    .and_then(|object| object.get::<_, String>("message").ok())
                    .unwrap_or_default();
                blocked(format!(
                    "cannot initialize dynamic host capabilities: {message}"
                ))
            })?;
            let load: Function = host.get("load")?;
            load.call::<_, ()>((ctx.json_parse(serde_json::to_vec(&plan.layers)?)?,))?;
            let owner: Function = host.get("owner")?;
            let play_requested: Function = host.get("playRequested")?;
            let initialize: Function = host.get("initialize")?;
            let registration: Function = host.get("register")?;
            let mut modules = Vec::new();
            for (index, binding) in plan.bindings.iter().enumerate() {
                let clock_owner = plan.timeline_owners[index];
                owner.call::<_, ()>((binding.layer, clock_owner.is_some()))?;
                let configure: Function = host.get("configure")?;
                configure.call::<_, ()>((
                    ctx.json_parse(serde_json::to_vec(&binding.script_properties)?)?,
                ))?;
                let declared = Module::declare(
                    ctx.clone(),
                    format!("lwe-layer-binding-{index}"),
                    binding.source.as_str(),
                )
                .map_err(|_| blocked("cannot compile dynamic script module"))?;
                registration.call::<_, ()>((true,))?;
                let (module, completion) = declared.eval().map_err(|_| {
                    blocked("dynamic script initialization failed or exceeded its limits")
                })?;
                completion.finish::<()>().map_err(|_| {
                    blocked("dynamic script module initialization is not synchronous")
                })?;
                registration.call::<_, ()>((false,))?;
                let finish: Function = host.get("finishModule")?;
                finish
                    .call::<_, ()>(())
                    .map_err(|_| blocked("invalid script property builder overrides"))?;
                let init: rquickjs::Value = module.get("init")?;
                if let Some(init) = init.as_function() {
                    initialize.call::<_, ()>((true,))?;
                    let input: Function = host.get("input")?;
                    let current = input
                        .call::<_, rquickjs::Value>((binding.layer, binding.property.as_str()))?;
                    let initial: rquickjs::Value = init
                        .call((current,))
                        .map_err(|_| blocked("dynamic init failed or exceeded its limits"))?;
                    let output: Function = host.get("output")?;
                    output
                        .call::<_, ()>((binding.layer, binding.property.as_str(), initial))
                        .map_err(|_| blocked("dynamic init returned an invalid binding value"))?;
                    initialize.call::<_, ()>((false,))?;
                }
                let apply: rquickjs::Value = module.get("applyUserProperties")?;
                if let Some(apply) = apply.as_function() {
                    let property_values: Function = host.get("properties")?;
                    let snapshot = property_values.call::<_, rquickjs::Value>((
                        ctx.json_parse(serde_json::to_vec(&plan.properties)?)?,
                    ))?;
                    apply.call::<_, ()>((snapshot,)).map_err(|_| {
                        blocked("dynamic property callback failed or exceeded its limits")
                    })?;
                }
                if clock_owner == Some(index) && play_requested.call::<_, bool>(())? {
                    timeline_starts[index] = Some(0.0);
                }
                modules.push(Persistent::save(&ctx, module.namespace()?));
            }
            let snapshot = read_snapshot(&host, &base_handles)?;
            Ok((Persistent::save(&ctx, host), modules, snapshot))
        })?;
        Ok((
            Self {
                modules,
                host,
                context,
                _runtime: runtime,
                deadline,
                bindings: plan.bindings.clone(),
                timeline_owners: plan.timeline_owners.clone(),
                timeline_starts,
                base_handles,
                last_time: 0.0,
                last_media: None,
            },
            initialization,
        ))
    }

    fn frame(&mut self, time: f64, audio: [f32; 64]) -> Result<Vec<LayerState>> {
        self.frame_with_audio(
            time,
            super::spectrum::SpectrumSnapshot {
                script_average: audio,
                ..Default::default()
            },
        )
    }

    fn frame_with_audio(
        &mut self,
        time: f64,
        audio: super::spectrum::SpectrumSnapshot,
    ) -> Result<Vec<LayerState>> {
        self.frame_with_media(time, audio, None)
    }

    fn frame_with_media(
        &mut self,
        time: f64,
        audio: super::spectrum::SpectrumSnapshot,
        media: Option<MediaSnapshot>,
    ) -> Result<Vec<LayerState>> {
        ensure!(
            time.is_finite() && time >= self.last_time,
            "Invalid or decreasing dynamic frame time"
        );
        let previous_time = self.last_time;
        let delta = time - previous_time;
        self.last_time = time;
        self.deadline.set(Instant::now() + FRAME_LIMIT);
        self.context.with(|ctx| -> Result<_> {
            let host = self.host.clone().restore(&ctx)?;
            let frame: Function = host.get("frame")?;
            let values=serde_json::json!({"left":audio.left.to_vec(),"right":audio.right.to_vec(),"script_left":audio.script_left.to_vec(),"script_right":audio.script_right.to_vec(),"script_average":audio.script_average.to_vec()});
            let owner: Function = host.get("owner")?;
            let play_requested: Function = host.get("playRequested")?;
            let input: Function = host.get("input")?;
            let output: Function = host.get("output")?;
            if let Some(media) = media {
                let old = self.last_media.as_ref();
                let playback_changed = old.is_none_or(|old| {
                    old.playback != media.playback || old.player != media.player
                });
                let properties_changed = old.is_some_and(|old| {
                    old.player != media.player
                        || old.track_id != media.track_id
                        || old.title != media.title
                        || old.artist != media.artist
                        || old.album_title != media.album_title
                        || old.album_artist != media.album_artist
                        || old.genres != media.genres
                }) || (old.is_none() && media.player.is_some());
                let freeze: Function = host.get("mediaEvent")?;
                for (name, payload) in [
                    (
                        MEDIA_PLAYBACK,
                        playback_changed.then(|| serde_json::json!({"state": match media.playback {
                            Playback::Stopped => 0, Playback::Playing => 1, Playback::Paused => 2,
                        }})),
                    ),
                    (
                        MEDIA_PROPERTIES,
                        properties_changed.then(|| serde_json::json!({
                            "title":media.title,"artist":media.artist,"albumTitle":media.album_title,
                            "albumArtist":media.album_artist,"genres":media.genres,
                        })),
                    ),
                ] {
                    let Some(payload) = payload else { continue; };
                    let payload = freeze.call::<_, rquickjs::Value>((
                        ctx.json_parse(serde_json::to_vec(&payload)?)?,
                    ))?;
                    for (index, (module, binding)) in self.modules.iter().zip(&self.bindings).enumerate() {
                        let module = module.clone().restore(&ctx)?;
                        let callback: rquickjs::Value = module.get(name)?;
                        if let Some(callback) = callback.as_function() {
                            let clock_owner = self.timeline_owners[index];
                            owner.call::<_, ()>((binding.layer, clock_owner.is_some()))?;
                            callback.call::<_, ()>((payload.clone(),))
                                .map_err(|_| blocked("dynamic media callback failed or exceeded its limits"))?;
                            if clock_owner == Some(index)
                                && play_requested.call::<_, bool>(())?
                            {
                                let track = binding.timeline.as_ref().expect("validated owner");
                                // Native media dispatch precedes clock advance.
                                play(track, &mut self.timeline_starts[index], previous_time);
                            }
                        }
                    }
                }
                self.last_media = Some(media);
            }
            frame.call::<_, ()>((time,delta,ctx.json_parse(serde_json::to_vec(&values)?)?))?;
            // The authored timeline pass precedes every SceneScript update,
            // including callbacks on other properties of the same layer.
            let timeline_vector: Function = host.get("timelineVector")?;
            for (binding, owner) in self.bindings.iter().zip(&self.timeline_owners) {
                let (Some(track), Some(owner)) = (&binding.timeline, owner) else {
                    continue;
                };
                let clock = &self.bindings[*owner].timeline.as_ref().expect("validated owner").clock;
                let elapsed = self.timeline_starts[*owner].map_or(0.0, |start| time - start);
                let position = clock.position(elapsed);
                let property = binding.property.as_str();
                match track.sample(position) {
                    Sample::Scalar(value) => output.call::<_, ()>((binding.layer, property, value))?,
                    Sample::Vector([x, y, z]) => {
                        timeline_vector.call::<_, ()>((binding.layer, property, x, y, z))?;
                    }
                }
            }
            for (index, ((module, binding), clock_owner)) in self
                .modules
                .iter()
                .zip(&self.bindings)
                .zip(&self.timeline_owners)
                .enumerate()
            {
                owner.call::<_, ()>((binding.layer, clock_owner.is_some()))?;
                let module = module.clone().restore(&ctx)?;
                let update: rquickjs::Value = module.get("update")?;
                let Some(update) = update.as_function() else {
                    continue;
                };
                let current =
                    input.call::<_, rquickjs::Value>((binding.layer, binding.property.as_str()))?;
                let value: rquickjs::Value = update
                    .call((current,))
                    .map_err(|_| blocked("dynamic update failed or exceeded its limits"))?;
                output
                    .call::<_, ()>((binding.layer, binding.property.as_str(), value))
                    .map_err(|_| blocked("dynamic update returned an invalid binding value"))?;
                // The child handle controls its private track. Only the root
                // handle can change the clock used to sample the group.
                if *clock_owner == Some(index)
                    && play_requested.call::<_, bool>(())?
                {
                    let track = binding.timeline.as_ref().expect("validated owner");
                    play(track, &mut self.timeline_starts[index], time);
                }
            }
            let snapshot = read_snapshot(&host, &self.base_handles)?;
            Ok(snapshot.states)
        })
    }
}

fn read_snapshot(host: &Object<'_>, base_handles: &BTreeSet<usize>) -> Result<Snapshot> {
    let snapshot: Function = host.get("snapshot")?;
    let json: String = snapshot
        .call(())
        .map_err(|_| blocked("cannot read dynamic script snapshot"))?;
    if json.len() > 128 * 1024 {
        return Err(blocked("dynamic snapshot exceeds its byte budget"));
    }
    let snapshot: Snapshot =
        serde_json::from_str(&json).map_err(|_| blocked("invalid dynamic script snapshot"))?;
    if snapshot.states.len() > MAX_DYNAMIC_LAYERS {
        return Err(blocked("dynamic snapshot exceeds its layer budget"));
    }
    let mut state_handles = BTreeSet::new();
    for state in &snapshot.states {
        validate_layer(state)?;
        if !state_handles.insert(state.handle) {
            return Err(blocked("dynamic snapshot repeats a layer handle"));
        }
    }
    let mut created_handles = BTreeSet::new();
    for created in &snapshot.created {
        if !created.path.starts_with("models/")
            || !created.path.ends_with(".json")
            || created.path.len() > 1024
            || created
                .path
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            || !created
                .path
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_./ -".contains(&byte))
            || base_handles.contains(&created.state.handle)
            || !created_handles.insert(created.state.handle)
            || !snapshot.states.iter().any(|state| state == &created.state)
        {
            return Err(blocked(
                "dynamic snapshot has an invalid created-layer resource record",
            ));
        }
    }
    if !base_handles.is_subset(&state_handles)
        || state_handles.len() != base_handles.len() + created_handles.len()
        || !created_handles.is_subset(&state_handles)
    {
        return Err(blocked(
            "dynamic snapshot layers do not match their prepared resource records",
        ));
    }
    Ok(snapshot)
}

struct FrameInput {
    seq: u64,
    time: f64,
    audio: super::spectrum::SpectrumSnapshot,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct FrameOutput {
    pub seq: u64,
    pub time: f64,
    pub states: Vec<LayerState>,
}

#[derive(Default)]
struct Mailbox {
    initialized: bool,
    input: Option<FrameInput>,
    output: Option<FrameOutput>,
    error: Option<String>,
    last_submitted: Option<(u64, f64)>,
    in_flight: bool,
}

pub(super) struct ScriptWorker {
    stop: Arc<AtomicBool>,
    mailbox: Arc<Mutex<Mailbox>>,
    thread: Option<JoinHandle<()>>,
}

impl ScriptWorker {
    #[cfg(test)]
    pub(super) fn spawn(plan: &PreparedScripts) -> Result<Self> {
        Self::spawn_with_wake(plan, None)
    }

    pub(super) fn spawn_with_wake(
        plan: &PreparedScripts,
        wake: Option<super::worker_wake::WorkerWake>,
    ) -> Result<Self> {
        let plan = plan.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let mailbox = Arc::new(Mutex::new(Mailbox::default()));
        let worker_stop = Arc::clone(&stop);
        let worker_mailbox = Arc::clone(&mailbox);
        let thread = thread::Builder::new()
            .name("lwe-scene-script".to_owned())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker_run(
                        plan,
                        &worker_mailbox,
                        Arc::clone(&worker_stop),
                        wake.as_ref(),
                    )
                }))
                .unwrap_or_else(|_| Err(blocked("dynamic script worker stopped unexpectedly")));
                if !worker_stop.load(Ordering::Acquire)
                    && let Err(error) = result
                {
                    worker_mailbox
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .error = Some(format!("{error:#}"));
                    if let Some(wake) = &wake {
                        wake.notify();
                    }
                }
            })
            .map_err(|_| blocked("cannot start dynamic SceneScript worker"))?;
        Ok(Self {
            stop,
            mailbox,
            thread: Some(thread),
        })
    }

    /// Submit exactly one tagged tick. False means not ready/contention or an
    /// already-submitted identical tick; no input or output is overwritten.
    pub(super) fn submit_frame(
        &self,
        seq: u64,
        time: f64,
        audio: super::spectrum::SpectrumSnapshot,
    ) -> Result<bool> {
        ensure!(
            time.is_finite()
                && time >= 0.0
                && audio
                    .left
                    .iter()
                    .chain(&audio.right)
                    .chain(&audio.script_left)
                    .chain(&audio.script_right)
                    .chain(&audio.script_average)
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "Invalid dynamic SceneScript frame input"
        );
        let mut mailbox = match self.mailbox.try_lock() {
            Ok(value) => value,
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Poisoned(_)) => {
                return Err(blocked("dynamic script mailbox is unavailable"));
            }
        };
        if let Some(error) = &mailbox.error {
            return Err(blocked(error.clone()));
        }
        if !mailbox.initialized {
            return Ok(false);
        }
        if let Some((previous, previous_time)) = mailbox.last_submitted {
            if (seq, time) == (previous, previous_time) {
                return Ok(false);
            }
            ensure!(
                seq > previous && time >= previous_time,
                "Invalid or stale dynamic SceneScript tick"
            );
        }
        if mailbox.in_flight || mailbox.output.is_some() {
            return Ok(false);
        }
        mailbox.last_submitted = Some((seq, time));
        mailbox.in_flight = true;
        mailbox.input = Some(FrameInput { seq, time, audio });
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
        Ok(true)
    }

    /// Consume an output with its actual input tag; never relabel a late VM
    /// result with a newer renderer time. Author code never runs under this lock.
    pub(super) fn take_frame(&self) -> Result<Option<FrameOutput>> {
        let mut mailbox = match self.mailbox.try_lock() {
            Ok(value) => value,
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Poisoned(_)) => {
                return Err(blocked("dynamic script mailbox is unavailable"));
            }
        };
        if let Some(error) = &mailbox.error {
            return Err(blocked(error.clone()));
        }
        let output = mailbox.output.take();
        if output.is_some() {
            mailbox.in_flight = false;
        }
        Ok(output)
    }

    /// Compatibility adapter for the existing 64-average-only tests/callers.
    /// Timed composition must use tagged submit/take rather than this adapter.
    #[cfg(test)]
    pub(super) fn poll_frame(
        &self,
        time: f64,
        audio: [f32; 64],
    ) -> Result<Option<Vec<LayerState>>> {
        ensure!(
            time.is_finite()
                && time >= 0.0
                && audio
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "Invalid dynamic SceneScript frame input"
        );
        if let Some(output) = self.take_frame()? {
            return Ok(Some(output.states));
        }
        let seq = {
            let mailbox = match self.mailbox.try_lock() {
                Ok(value) => value,
                Err(TryLockError::WouldBlock) => return Ok(None),
                Err(TryLockError::Poisoned(_)) => {
                    return Err(blocked("dynamic script mailbox is unavailable"));
                }
            };
            match mailbox.last_submitted {
                Some((_, previous_time)) if previous_time == time => return Ok(None),
                Some((seq, _)) => seq
                    .checked_add(1)
                    .ok_or_else(|| blocked("dynamic tick sequence overflow"))?,
                None => return Ok(None),
            }
        };
        self.submit_frame(
            seq,
            time,
            super::spectrum::SpectrumSnapshot {
                script_average: audio,
                ..Default::default()
            },
        )?;
        Ok(None)
    }
}

fn worker_run(
    plan: PreparedScripts,
    mailbox: &Mutex<Mailbox>,
    stop: Arc<AtomicBool>,
    wake: Option<&super::worker_wake::WorkerWake>,
) -> Result<()> {
    let (mut vm, initialization) = Vm::new(&plan, Arc::clone(&stop))?;
    if initialization.created != plan.created || initialization.audio != plan.audio {
        return Err(blocked(
            "dynamic initialization differs from the prepared resource plan",
        ));
    }
    let media_feed = plan.media.then(MediaFeed::acquire).transpose()?;
    let initial = if let Some(feed) = &media_feed {
        let deadline = Instant::now() + Duration::from_secs(2);
        let snapshot = loop {
            if stop.load(Ordering::Acquire) {
                return Ok(());
            }
            if let Some(snapshot) = feed.latest()? {
                break snapshot;
            }
            ensure!(
                Instant::now() < deadline,
                blocked("media input did not become ready")
            );
            thread::park_timeout(Duration::from_millis(10));
        };
        vm.frame_with_media(0.0, Default::default(), Some(snapshot))?
    } else {
        let initial = vm.frame(0.0, [0.0; 64])?;
        ensure!(
            initial == plan.initial,
            blocked("dynamic initial frame differs from its prepared snapshot")
        );
        initial
    };
    let mut ready = mailbox
        .lock()
        .map_err(|_| blocked("dynamic script mailbox is unavailable"))?;
    ready.output = Some(FrameOutput {
        seq: 0,
        time: 0.0,
        states: initial,
    });
    ready.initialized = true;
    ready.last_submitted = Some((0, 0.0));
    drop(ready);
    if let Some(wake) = wake {
        wake.notify();
    }
    while !stop.load(Ordering::Acquire) {
        let input = mailbox
            .lock()
            .map_err(|_| blocked("dynamic script mailbox is unavailable"))?
            .input
            .take();
        if let Some(input) = input {
            let media = media_feed
                .as_ref()
                .map(MediaFeed::latest)
                .transpose()?
                .flatten();
            let states = vm.frame_with_media(input.time, input.audio, media)?;
            let output = FrameOutput {
                seq: input.seq,
                time: input.time,
                states,
            };
            mailbox
                .lock()
                .map_err(|_| blocked("dynamic script mailbox is unavailable"))?
                .output = Some(output);
            if let Some(wake) = wake {
                wake.notify();
            }
        } else {
            thread::park();
        }
    }
    Ok(())
}

impl Drop for ScriptWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            if thread.join().is_err() {
                tracing::warn!("SceneScript worker stopped unexpectedly");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    fn layer() -> LayerState {
        LayerState {
            handle: 7,
            origin: [100.0, 90.0, 0.0],
            scale: [0.0; 3],
            angles: [0.0; 3],
            color: [0.0; 3],
            alpha: 1.0,
            visible: true,
            text: None,
        }
    }

    fn prepare(source: &str) -> Result<PreparedScripts> {
        PreparedScripts::prepare(
            vec![Binding {
                layer: 7,
                property: "visible".to_owned(),
                source: source.to_owned(),
                input: Value::Bool(true),
                script_properties: serde_json::json!({}),
                timeline: None,
            }],
            vec![layer()],
            serde_json::json!({"mode":"meter"}),
        )
    }

    #[test]
    fn mixed_timeline_samples_before_any_binding_update() {
        let author = super::super::tests::timeline_alpha();
        let mut plan = prepare("export function update(){return thisLayer.alpha > 0.5;}").unwrap();
        plan.bindings.push(Binding {
            layer: 7,
            property: "alpha".into(),
            source: "let base; export function init(value){base=value;} export function update(value){return value*base*0.5;}".into(),
            input: author["value"].clone(),
            script_properties: serde_json::json!({}),
            timeline: Some(
                PropertyTimeline::parse(&author["animation"], "alpha", &author["value"]).unwrap(),
            ),
        });
        plan.timeline_owners = timeline_owners(&plan.bindings).unwrap();
        let (mut vm, _) = Vm::new(&plan, Arc::new(AtomicBool::new(false))).unwrap();
        let initial = vm.frame(0., [0.; 64]).unwrap();
        assert!(!initial[0].visible);
        assert_eq!(initial[0].alpha, 0.125);
        let next = vm.frame(0.25, [0.; 64]).unwrap();
        assert!(next[0].visible);
        assert_eq!(next[0].alpha, 0.375);
    }

    #[test]
    fn media_changes_dispatch_before_timeline_and_deduplicate() {
        let mut author = super::super::tests::timeline_alpha();
        author["animation"]["options"]["startpaused"] = Value::Bool(true);
        let source = r#"
            let changes=0;
            export function mediaPlaybackChanged(event){
                thisLayer.visible=event.state!==MediaPlaybackEvent.PLAYBACK_STOPPED;
            }
            export function mediaPropertiesChanged(event){
                changes+=1;
                thisLayer.origin=new Vec3(changes,0,0);
                if(event.title==='Example'){thisObject.getAnimation().play();}
            }
        "#;
        let plan = PreparedScripts::prepare(
            vec![Binding {
                layer: 7,
                property: "alpha".to_owned(),
                source: source.to_owned(),
                input: author["value"].clone(),
                script_properties: serde_json::json!({}),
                timeline: Some(
                    PropertyTimeline::parse(&author["animation"], "alpha", &author["value"])
                        .unwrap(),
                ),
            }],
            vec![layer()],
            serde_json::json!({}),
        )
        .unwrap();
        assert!(plan.media);
        let (mut vm, _) = Vm::new(&plan, Arc::new(AtomicBool::new(false))).unwrap();
        let stopped = MediaSnapshot::default();
        let playing = MediaSnapshot {
            player: Some("org.mpris.MediaPlayer2.lwe_test".to_owned()),
            playback: Playback::Playing,
            title: "Example".to_owned(),
            ..Default::default()
        };
        let mut tick = |t, snapshot| {
            vm.frame_with_media(t, Default::default(), Some(snapshot))
                .unwrap()
                .remove(0)
        };
        let initial = tick(0., stopped.clone());
        assert!(!initial.visible);
        assert_eq!(initial.alpha, 0.25);
        let active = tick(0.25, playing.clone());
        assert!(active.visible);
        assert_eq!(active.alpha, 0.75);
        assert_eq!(active.origin[0], 1.);
        let unchanged = tick(0.5, playing);
        assert_eq!(unchanged, active);
        let clear = tick(0.75, stopped);
        assert!(!clear.visible);
        assert_eq!(clear.origin[0], 2.);
    }

    #[test]
    fn media_callbacks_keep_unmodeled_fields_and_thumbnail_events_gated() {
        for source in [
            "export function mediaThumbnailChanged(event){}",
            "export function mediaPropertiesChanged(event){return event.artUrl;}",
            "export function mediaPlaybackChanged(event){event.state=0;}",
        ] {
            assert!(prepare(source).is_err());
        }
    }

    #[test]
    #[ignore = "requires the isolated dbus-run-session MPRIS fixture"]
    fn private_bus_media_callbacks_reach_accepted_worker_frames() -> Result<()> {
        ensure!(
            std::env::var("LWE_TEST_PRIVATE_MPRIS").as_deref() == Ok("1"),
            "Start the fixture on a private session bus"
        );
        let plan = prepare(
            r#"
            export function mediaPlaybackChanged(event){
                thisLayer.visible=event.state===MediaPlaybackEvent.PLAYBACK_PLAYING;
            }
            export function mediaPropertiesChanged(event){
                thisLayer.origin=new Vec3(event.title==='First fixture track'?1:event.title==='Second fixture track'?2:3,0,0);
            }
        "#,
        )?;
        let worker = ScriptWorker::spawn(&plan)?;
        let first = wait_tagged(&worker);
        assert_eq!(first.seq, 0);
        assert!(first.states[0].visible);
        assert_eq!(first.states[0].origin[0], 1.);
        let mut paused = false;
        for tick in 1..=180 {
            thread::sleep(Duration::from_millis(20));
            let states = wait_snapshot(&worker, f64::from(tick) * 0.02, [0.; 64]);
            if states[0].origin[0] == 2. {
                assert!(!states[0].visible);
                paused = true;
            }
            if states[0].origin[0] == 3. {
                assert!(!states[0].visible);
                assert!(paused);
                return Ok(());
            }
        }
        Err(blocked(
            "MPRIS fixture changes did not reach the script worker",
        ))
    }

    #[test]
    fn property_play_starts_paused_and_restarts_only_after_completion() {
        let mut author = super::super::tests::timeline_alpha();
        author["animation"]["options"]["startpaused"] = Value::Bool(true);
        let source = "export function update(value){if(engine.runtime>=0.25){thisObject.getAnimation().play();}return value;}";
        assert!(prepare(source).is_err());
        let plan = PreparedScripts::prepare(
            vec![Binding {
                layer: 7,
                property: "alpha".into(),
                source: source.into(),
                input: author["value"].clone(),
                script_properties: serde_json::json!({}),
                timeline: Some(
                    PropertyTimeline::parse(&author["animation"], "alpha", &author["value"])
                        .unwrap(),
                ),
            }],
            vec![layer()],
            serde_json::json!({}),
        )
        .unwrap();
        let (mut vm, _) = Vm::new(&plan, Arc::new(AtomicBool::new(false))).unwrap();
        for (time, alpha) in [
            (0., 0.25),
            (0.125, 0.25),
            (0.25, 0.25),
            (0.375, 0.5),
            (0.5, 0.75),
            (1.25, 0.75),
            (1.375, 0.5),
        ] {
            assert_eq!(vm.frame(time, [0.; 64]).unwrap()[0].alpha, alpha, "{time}");
        }
    }

    // Independent author fixture: do not copy a user-owned package's scripts
    // into the repository. The reused vectors exercise native value copies.
    const AUDIO_BARS: &str = r#"
        const samples = engine.registerAudioBuffers(64);
        let meters = [];
        let anchor;
        export function init() {
            for (let slot = 0; slot < 64; slot++) {
                meters.push(slot === 0 ? thisLayer : thisScene.createLayer('models/meter.json'));
            }
            anchor = thisLayer.origin;
        }
        export function applyUserProperties(settings) {
            for (let item = 0; item < 64; item++) {
                meters[item].visible = settings.mode === 'meter';
            }
        }
        export function update() {
            let position = anchor.copy();
            let stretch = new Vec3(5);
            for (let band = 0; band < 64; band++) {
                position.x += 30;
                stretch.y = samples.average[band] * 50;
                meters[band].origin = position;
                meters[band].scale = stretch;
            }
        }
    "#;

    #[test]
    fn audio_layers_preserve_value_copy_and_zero_scale() {
        let plan = prepare(AUDIO_BARS).unwrap();
        assert!(plan.uses_audio());
        assert_eq!(plan.created_layers().len(), 63);
        assert_eq!(plan.created_layers()[0].state.handle, 8);
        assert_eq!(plan.initial_states()[0].scale, [5.0, 0.0, 5.0]);
        assert_eq!(plan.initial_states()[0].origin, [130.0, 90.0, 0.0]);
        assert_eq!(plan.initial_states()[63].origin, [2020.0, 90.0, 0.0]);
        let (mut vm, _) = Vm::new(&plan, Arc::new(AtomicBool::new(false))).unwrap();
        let output = vm
            .frame(1.0, std::array::from_fn(|i| i as f32 / 64.0))
            .unwrap();
        assert_eq!(output[0].scale, [5.0, 0.0, 5.0]);
        assert_eq!(output[63].scale, [5.0, 49.21875, 5.0]);
        assert_eq!(output[0].origin[0], 130.0);
        assert_eq!(output[63].origin[0], 2020.0);
        assert!(output.iter().all(|state| state.visible));
    }

    #[test]
    fn unsafe_and_unimplemented_features_fail_closed() {
        for source in [
            "export function update(){ engine.launch('x'); }",
            "export function update(){ thisLayer.foo = 1; }",
            "export function update(){ return eval('1'); }",
            "export async function update(){}",
            "import x from 'x'; export function update(){}",
            "export function update(){ thisLayer.origin.constructor('return 1')(); }",
            "export function update(){ thisScene.createLayer('../x'); }",
            "let a=engine.registerAudioBuffers(64); export function update(){a.average[0]=1;}",
            "let a=engine.registerAudioBuffers(64); let k='left'; export function update(){return a[k];}",
            "let a=engine.registerAudioBuffers(64); let k=0; export function update(){k='left';return a[k];}",
            "let i=0; let list=[]; i='__proto__'; let proto=list[i]; proto.push=Math.sin; export function init(){thisScene.createLayer('models/bar.json');} export function update(){}",
            "let key=0; let list=[]; export function init(){let key='__proto__'; let proto=list[key]; proto.push=Math.sin; thisScene.createLayer('models/bar.json');} export function update(){}",
            "let key=0; let list=[]; export function applyUserProperties(key){let proto=list[key];proto.push=Math.sin;} export function update(){}",
            "export function update(){Math.max=Math.abs;}",
            "export function update(){thisLayer=1;}",
            "export function init(){let a=engine.registerAudioBuffers(64);} export function update(){}",
            "export function applyUserProperties(p){let a=engine.registerAudioBuffers(64);} export function update(){}",
        ] {
            let error = prepare(source).unwrap_err();
            assert_eq!(
                super::super::unsupported_feature_code(&error),
                Some("scenescript")
            );
        }
    }

    #[test]
    fn loops_and_creation_budgets_are_enforced() {
        let started = Instant::now();
        assert!(prepare("export function update(){while(true){}}").is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(prepare("export function init(){for(let i=0;i<128;i++){thisScene.createLayer('models/x.json');}} export function update(){}").is_err());
    }

    #[test]
    fn undefined_return_retains_binding_and_boolean_return_applies() {
        let unchanged = prepare("export function update(){}").unwrap();
        assert!(unchanged.initial_states()[0].visible);
        let changed = prepare("export function update(){return false;}").unwrap();
        assert!(!changed.initial_states()[0].visible);
        let initialized =
            prepare("export function init(){return false;} export function update(){}").unwrap();
        assert!(!initialized.initial_states()[0].visible);
    }

    fn wait_snapshot(worker: &ScriptWorker, time: f64, audio: [f32; 64]) -> Vec<LayerState> {
        let started = Instant::now();
        loop {
            if let Some(snapshot) = worker.poll_frame(time, audio).unwrap() {
                return snapshot;
            }
            assert!(started.elapsed() < Duration::from_secs(2));
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn workers_are_independent_and_cancellation_is_bounded() {
        let plan =
            prepare("let n=0; export function update(){n+=1; thisLayer.origin=new Vec3(n);}")
                .unwrap();
        let first = ScriptWorker::spawn(&plan).unwrap();
        let second = ScriptWorker::spawn(&plan).unwrap();
        assert_eq!(wait_snapshot(&first, 0.0, [0.0; 64])[0].origin, [1.0; 3]);
        assert_eq!(wait_snapshot(&second, 0.0, [0.0; 64])[0].origin, [1.0; 3]);
        assert!(first.poll_frame(0.0, [0.0; 64]).unwrap().is_none());
        assert!(first.mailbox.lock().unwrap().input.is_none());
        let started = Instant::now();
        drop(first);
        drop(second);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn contention_never_waits_and_later_faults_are_reported() {
        let plan = prepare("let n=0; export function update(){n+=1;thisLayer.origin=new Vec3(n);if(n>1){while(true){}}}").unwrap();
        let worker = ScriptWorker::spawn(&plan).unwrap();
        let lock = worker.mailbox.lock().unwrap();
        let started = Instant::now();
        assert!(worker.take_frame().unwrap().is_none());
        assert!(started.elapsed() < Duration::from_millis(100));
        drop(lock);
        let initial = wait_tagged(&worker);
        assert_eq!((initial.seq, initial.time), (0, 0.0));
        assert_eq!(initial.states[0].origin, [1.0; 3]);
        assert!(worker.submit_frame(1, 1.0, Default::default()).unwrap());
        let started = Instant::now();
        loop {
            if let Err(error) = worker.take_frame() {
                assert_eq!(
                    super::super::unsupported_feature_code(&error),
                    Some("scenescript")
                );
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(2));
            thread::sleep(Duration::from_millis(1));
        }
        // The failed tick mutated the private heap/layer before interruption.
        // Neither reading nor resubmitting can manufacture a recovered snapshot.
        for _ in 0..3 {
            assert!(worker.take_frame().is_err());
            assert!(worker.submit_frame(2, 2.0, Default::default()).is_err());
        }
        assert!(worker.mailbox.lock().unwrap().output.is_none());
        let started = Instant::now();
        drop(worker);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn pause_without_submission_keeps_vm_state_and_returns_use_current_value() {
        let plan = prepare("export function update(current){return !current;}").unwrap();
        let (mut first, _) = Vm::new(&plan, Arc::new(AtomicBool::new(false))).unwrap();
        let (mut second, _) = Vm::new(&plan, Arc::new(AtomicBool::new(false))).unwrap();
        assert!(!first.frame(0.0, [0.0; 64]).unwrap()[0].visible);
        assert!(first.frame(1.0, [0.0; 64]).unwrap()[0].visible);
        // An independent paused instance has no callback to advance its state.
        assert!(!second.frame(10.0, [0.0; 64]).unwrap()[0].visible);
    }

    fn prepare_bound(
        source: &str,
        property: &str,
        input: Value,
        overrides: Value,
    ) -> Result<PreparedScripts> {
        let mut owner = layer();
        if property == "text" {
            owner.text = Some(input.as_str().unwrap().to_owned());
            owner.angles[1] = -0.79215;
        }
        PreparedScripts::prepare(
            vec![Binding {
                layer: 7,
                property: property.to_owned(),
                source: source.to_owned(),
                input,
                script_properties: overrides,
                timeline: None,
            }],
            vec![owner],
            serde_json::json!({}),
        )
    }

    const BUILDER_ALPHA: &str = r#"
        export var scriptProperties=createScriptProperties()
          .addSlider({name:'band',label:'Band',value:2,min:0,max:15,integer:true})
          .addSlider({name:'response',label:'Response',value:1,min:0,max:10,integer:false})
          .addCheckbox({name:'enabled',label:'Enabled',value:true}).finish();
        const bands=engine.registerAudioBuffers(engine.AUDIO_RESOLUTION_16);
        let initial;
        let amount=0;
        export function init(value){initial=typeof value==='number'?value:value.x;}
        export function update(){
          const delta=bands.average[scriptProperties.band]-amount;
          amount+=delta*Math.min(1,engine.frametime*scriptProperties.response);
          return scriptProperties.enabled?initial*amount:0;
        }
    "#;

    #[test]
    fn builders_init_and_stereo_resolution_follow_frame_clock() {
        let plan = prepare_bound(
            BUILDER_ALPHA,
            "alpha",
            serde_json::json!(1),
            serde_json::json!({"band":3,"response":2}),
        )
        .unwrap();
        assert!(plan.uses_audio());
        assert_eq!(plan.initial_states()[0].alpha, 0.0);
        let (mut vm, _) = Vm::new(&plan, Arc::new(AtomicBool::new(false))).unwrap();
        vm.frame_with_audio(0.0, Default::default()).unwrap();
        let mut audio = super::super::spectrum::SpectrumSnapshot::default();
        audio.left[3] = 0.8;
        audio.right[3] = 0.2;
        // Independent 64-band values cannot substitute for the 16-band source.
        audio.script_average[3] = 1.0;
        assert!((vm.frame_with_audio(0.25, audio).unwrap()[0].alpha - 0.25).abs() < 1e-6);
        assert!((vm.frame_with_audio(0.5, audio).unwrap()[0].alpha - 0.375).abs() < 1e-6);
        assert!(vm.frame_with_audio(0.4, audio).is_err());
        for overrides in [
            serde_json::json!({"band":16}),
            serde_json::json!({"band":1.5}),
            serde_json::json!({"enabled":1}),
            serde_json::json!({"unknown":1}),
        ] {
            assert!(
                prepare_bound(BUILDER_ALPHA, "alpha", serde_json::json!(1), overrides).is_err()
            );
        }
    }

    const BUILDER_TEXT: &str = r#"
        export const scriptProperties=createScriptProperties()
          .addText({name:'separator',label:'Separator',value:'/'})
          .addCombo({name:'format',label:'Format',options:[
            {label:'Long',value:'2'}, {label:'Short',value:'1'}
          ]}).finish();
        export function update(){return scriptProperties.format+scriptProperties.separator;}
    "#;

    #[test]
    fn text_and_combo_builder_use_typed_overrides_and_first_option_default() {
        for (overrides, expected) in [
            (serde_json::json!({}), "2/"),
            (serde_json::json!({"separator":"","format":"1"}), "1"),
            (serde_json::json!({"separator":"年"}), "2年"),
        ] {
            let plan =
                prepare_bound(BUILDER_TEXT, "text", serde_json::json!("x"), overrides).unwrap();
            assert_eq!(plan.initial_states()[0].text.as_deref(), Some(expected));
        }
        for overrides in [
            serde_json::json!({"separator":1}),
            serde_json::json!({"separator":"x".repeat(4097)}),
            serde_json::json!({"format":2}),
            serde_json::json!({"format":"missing"}),
            serde_json::json!({"unknown":"1"}),
        ] {
            assert!(
                prepare_bound(BUILDER_TEXT, "text", serde_json::json!("x"), overrides).is_err()
            );
        }
    }

    #[test]
    fn string_builder_requires_literal_metadata_and_cannot_enable_string_indexing() {
        for (old, replacement) in [
            ("value:'/'", "value:'/'+'x'"),
            ("label:'Short',value:'1'", "label:'Short',value:'2'"),
            ("label:'Short',value:'1'", "label:'Short',value:1"),
            ("options:[", "value:'1',options:["),
            (
                "scriptProperties.format+scriptProperties.separator",
                "[1,2][scriptProperties.format]",
            ),
        ] {
            let source = BUILDER_TEXT.replace(old, replacement);
            assert!(
                prepare_bound(
                    &source,
                    "text",
                    serde_json::json!("x"),
                    serde_json::json!({})
                )
                .is_err()
            );
        }
    }

    #[test]
    fn text_array_sum_and_fixed_authored_rotation_are_bounded() {
        let source = r#"
          export let __workshopId='inert-metadata';
          export var scriptProperties=createScriptProperties()
            .addCheckbox({name:'enabled',label:'Enabled',value:true}).finish();
          let values=[];
          export function init(value){return value;}
          export function update(value){
            if(!scriptProperties.enabled){return '';}
            const rate=(1/engine.frametime)|0;
            if(engine.frametime==0){return value;}
            values.unshift(rate);
            if(values.length>3){values.length=3;}
            thisLayer.color=new Vec3(255,0,0);
            return 'Rate '+((values.reduce((left,right)=>left+right,0)/values.length)|0);
          }
        "#;
        let plan = prepare_bound(
            source,
            "text",
            serde_json::json!("initial"),
            serde_json::json!({}),
        )
        .unwrap();
        assert_eq!(plan.initial_states()[0].text.as_deref(), Some("initial"));
        let (mut vm, _) = Vm::new(&plan, Arc::new(AtomicBool::new(false))).unwrap();
        vm.frame(0.0, [0.0; 64]).unwrap();
        let state = vm.frame(0.25, [0.0; 64]).unwrap().remove(0);
        assert_eq!(state.text.as_deref(), Some("Rate 4"));
        assert_eq!(state.color, [255.0, 0.0, 0.0]);
        assert_eq!(state.angles[1], -0.79215);
        assert!(
            prepare_bound(
                "export function update(){thisLayer.angles=new Vec3(0);}",
                "text",
                serde_json::json!("x"),
                serde_json::json!({})
            )
            .is_err()
        );
        assert!(prepare("export function update(){thisLayer.text='x';}").is_err());
        assert!(
            prepare(
                "let list=[];export function update(){for(let i=0;i<129;i++){list.unshift(1);}}"
            )
            .is_err()
        );
        for source in [
            "export var scriptProperties=createScriptProperties().finish();export function update(){scriptProperties={band:'__proto__'};}",
            "export var scriptProperties=createScriptProperties().addSlider({name:'band',label:'',value:0,min:0,max:4,integer:true}).finish();export function update(scriptProperties){let list=[];return list[scriptProperties.band];}",
            "export var scriptProperties=createScriptProperties();export function update(){}",
            "let values=[];export function update(){values.reduce((a,b)=>thisLayer.alpha,0);}",
            "const samples=engine.registerAudioBuffers(64);let key=0;export function update(){let samples={average:['__proto__']};key=samples.average[0];let list=[];return list[key];}",
        ] {
            assert!(prepare(source).is_err(), "accepted unsafe source {source}");
        }
    }

    fn wait_tagged(worker: &ScriptWorker) -> FrameOutput {
        let start = Instant::now();
        loop {
            if let Some(output) = worker.take_frame().unwrap() {
                return output;
            }
            assert!(start.elapsed() < Duration::from_secs(2));
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn wait_notification(
        wake: &super::super::worker_wake::WorkerWake,
        event_loop: &mut calloop::EventLoop<'_, ()>,
    ) {
        let start = Instant::now();
        while wake.take().is_none() {
            assert!(start.elapsed() < Duration::from_secs(2));
            event_loop
                .dispatch(Duration::from_millis(20), &mut ())
                .unwrap();
        }
    }

    #[test]
    fn worker_notifications_publish_initial_tagged_output_and_fatal_errors_first() {
        use super::super::worker_wake::WorkerWake;
        let (ping, source) = calloop::ping::make_ping().unwrap();
        let wake = WorkerWake::new(ping.clone());
        let mut event_loop = calloop::EventLoop::<()>::try_new().unwrap();
        event_loop
            .handle()
            .insert_source(source, |(), _, ()| {})
            .unwrap();
        let plan = prepare("let count=0;export function update(){count+=1;thisLayer.origin=new Vec3(engine.runtime,engine.frametime,count);}").unwrap();
        let worker = ScriptWorker::spawn_with_wake(&plan, Some(wake.clone())).unwrap();
        wait_notification(&wake, &mut event_loop);
        let initial = worker.take_frame().unwrap().unwrap();
        assert_eq!((initial.seq, initial.time), (0, 0.0));
        assert_eq!(initial.states[0].origin, [0., 0., 1.]);
        assert!(worker.submit_frame(7, 0.25, Default::default()).unwrap());
        assert!(!worker.submit_frame(8, 0.5, Default::default()).unwrap());
        wait_notification(&wake, &mut event_loop);
        let output = worker.take_frame().unwrap().unwrap();
        assert_eq!((output.seq, output.time), (7, 0.25));
        assert_eq!(output.states[0].origin, [0.25, 0.25, 2.]);
        assert!(worker.take_frame().unwrap().is_none());
        drop(worker);
        let failed_wake = WorkerWake::new(ping);
        let mut inconsistent = plan;
        inconsistent.initial[0].origin = [999.; 3];
        let failed =
            ScriptWorker::spawn_with_wake(&inconsistent, Some(failed_wake.clone())).unwrap();
        wait_notification(&failed_wake, &mut event_loop);
        assert!(failed.take_frame().is_err());
        assert!(failed.mailbox.lock().unwrap().output.is_none());
    }

    #[test]
    fn tagged_ticks_never_overwrite_or_relabel_an_unconsumed_vm_result() {
        let plan=prepare("let count=0;export function update(){count+=1;thisLayer.origin=new Vec3(engine.runtime,engine.frametime,count);}").unwrap();
        let worker = ScriptWorker::spawn(&plan).unwrap();
        let first = wait_tagged(&worker);
        assert_eq!((first.seq, first.time), (0, 0.0));
        assert_eq!(first.states[0].origin, [0.0, 0.0, 1.0]);
        assert!(worker.submit_frame(7, 0.25, Default::default()).unwrap());
        assert!(!worker.submit_frame(8, 0.5, Default::default()).unwrap());
        let next = wait_tagged(&worker);
        assert_eq!((next.seq, next.time), (7, 0.25));
        assert_eq!(next.states[0].origin, [0.25, 0.25, 2.0]);
        assert!(!worker.submit_frame(7, 0.25, Default::default()).unwrap());
        assert!(worker.submit_frame(6, 0.5, Default::default()).is_err());
        assert!(worker.submit_frame(8, 0.2, Default::default()).is_err());
        // Forced paused draws do not submit a tick or mutate the VM.
        assert!(worker.take_frame().unwrap().is_none());
        assert!(worker.submit_frame(8, 0.25, Default::default()).unwrap());
        let same_time = wait_tagged(&worker);
        assert_eq!(same_time.states[0].origin, [0.25, 0.0, 3.0]);
        let mut invalid = super::super::spectrum::SpectrumSnapshot::default();
        invalid.script_left[63] = f32::NAN;
        assert!(worker.submit_frame(9, 0.3, invalid).is_err());
    }

    #[test]
    #[ignore = "requires private original script evidence and explicit CPU-only opt-in"]
    fn original_five_modules_run_with_wrappers_and_true_resolution_inputs() {
        assert_eq!(
            std::env::var("LWE_ORIGINAL_SCRIPT_ORACLE").as_deref(),
            Ok("1")
        );
        let path = std::path::PathBuf::from(
            std::env::var_os("LWE_ORIGINAL_SCRIPT_BINDINGS").expect("private binding evidence"),
        );
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() < 128 * 1024);
        let evidence: Value = serde_json::from_slice(&bytes).unwrap();
        let originals = evidence["scripts"].as_array().unwrap();
        assert_eq!(originals.len(), 5);
        let mut bindings = Vec::new();
        let mut states = Vec::new();
        let mut hashes = Vec::new();
        for original in originals {
            let path = original["path"].as_str().unwrap();
            let parts: Vec<_> = path.split('/').collect();
            let handle: usize = parts[2].parse().unwrap();
            let wrapper = &original["wrapper"];
            let property = parts[3];
            let mut state = layer();
            state.handle = handle;
            state.scale = [1.0; 3];
            state.color = [1.0; 3];
            if property == "text" {
                state.text = Some(wrapper["value"].as_str().unwrap().to_owned());
                state.angles[1] = -0.79215;
            } else {
                state.alpha = wrapper["value"].as_f64().unwrap() as f32;
            }
            let source = wrapper["script"].as_str().unwrap().to_owned();
            hashes.push(format!("{:x}", sha2::Sha256::digest(source.as_bytes())));
            bindings.push(Binding {
                layer: handle,
                property: property.to_owned(),
                source,
                input: wrapper["value"].clone(),
                script_properties: wrapper["scriptproperties"].clone(),
                timeline: None,
            });
            states.push(state);
        }
        let plan = PreparedScripts::prepare(bindings, states, serde_json::json!({})).unwrap();
        assert!(plan.uses_audio());
        assert!(plan.created_layers().is_empty());
        assert_eq!(plan.initial_states()[0].text.as_deref(), Some("FPS"));
        let (mut vm, _) = Vm::new(&plan, Arc::new(AtomicBool::new(false))).unwrap();
        vm.frame(0.0, [0.0; 64]).unwrap();
        let mut audio = super::super::spectrum::SpectrumSnapshot::default();
        audio.script_average[0] = 0.4;
        audio.script_average[1] = 1.0;
        audio.script_average[63] = 0.8;
        audio.left[7] = 0.8;
        audio.right[7] = 0.2;
        let first = vm.frame_with_audio(0.2, audio).unwrap();
        for (handle, expected) in [(5, 0.0), (6, 0.26), (7, 0.519999984), (9, 0.574999995)] {
            assert!(
                (f64::from(first.iter().find(|s| s.handle == handle).unwrap().alpha) - expected)
                    .abs()
                    < 1e-6
            );
        }
        let final_states = vm.frame_with_audio(0.4, audio).unwrap();
        let text = final_states.iter().find(|s| s.handle == 3).unwrap();
        assert_eq!(text.text.as_deref(), Some("FPS: 5\n Avg: 5"));
        assert_eq!(text.color, [255.0, 0.0, 0.0]);
        let report = serde_json::json!({"ok":true,"kind":"original_modules_cpu_vm_only","source_bindings_sha256":format!("{:x}",sha2::Sha256::digest(&bytes)),"module_sha256":hashes,"count":5,"times":[0.0,0.2,0.4],"initial":plan.initial_states(),"frame_0_2":first,"frame_0_4":final_states,"audio":"independent supplied 16 stereo / 64 average; not capture acceptance","limits":"no desktop, GPU, native Windows parity, or arbitrary QuickJS rollback"});
        if let Some(path) = std::env::var_os("LWE_ORIGINAL_SCRIPT_REPORT") {
            std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
        println!(
            "Original 5 module CPU VM oracle passed; authored input SHA {}",
            report["source_bindings_sha256"]
        );
    }

    #[test]
    fn snapshot_requires_a_resource_record_for_every_added_handle() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        let mut added = layer();
        added.handle = 8;
        context.with(|ctx| {
            for payload in [
                serde_json::json!({"states":[layer(),added],"created":[],"audio":false}),
                serde_json::json!({"states":[layer()],"created":[{"path":"models/bar.json","state":added}],"audio":false}),
                serde_json::json!({"states":[layer(),layer()],"created":[],"audio":false}),
                serde_json::json!({"states":[layer(),added],"created":[{"path":"models/../x.json","state":added}],"audio":false}),
            ] {
                let host = Object::new(ctx.clone()).unwrap();
                let json = payload.to_string();
                host.set("snapshot", Function::new(ctx.clone(), move || json.clone()).unwrap()).unwrap();
                assert!(read_snapshot(&host, &BTreeSet::from([7])).is_err());
            }
        });
    }
}
