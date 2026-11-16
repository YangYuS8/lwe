//! Prepare authored, planar layer placements before resource loading.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde_json::Value;

use super::{boolean, dynamic_script::LayerState, unsupported, vector};

#[derive(Debug, Clone, Copy)]
pub(super) struct Transform {
    pub(super) x: [f32; 2],
    pub(super) y: [f32; 2],
    pub(super) origin: [f32; 2],
}

impl Transform {
    pub(super) fn fits_projected_quad(self, size: [f32; 2], canvas: [f32; 2]) -> bool {
        // A cover viewport can magnify these values by at most the ratio of
        // its positive i32 dimensions. Reserve room for the vertex sums too.
        let limit = f32::MAX / (8. * i32::MAX as f32);
        self.x
            .map(|x| x * size[0])
            .into_iter()
            .chain(self.y.map(|y| y * size[1]))
            .chain(
                self.origin
                    .into_iter()
                    .zip(canvas)
                    .map(|(x, d)| 2. * (x - d / 2.)),
            )
            .all(|value| value.abs() <= limit)
    }

    fn apply_vector(self, v: [f32; 2]) -> [f32; 2] {
        std::array::from_fn(|i| self.x[i] * v[0] + self.y[i] * v[1])
    }

    fn compose(self, local: Self) -> Self {
        let origin = self.apply_vector(local.origin);
        Self {
            x: self.apply_vector(local.x),
            y: self.apply_vector(local.y),
            origin: std::array::from_fn(|i| origin[i] + self.origin[i]),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Resolved {
    pub(super) world: Transform,
    pub(super) visible: bool,
}

#[derive(Debug, Clone, Copy)]
struct Node {
    parent: Option<usize>,
    authored: Resolved,
    transform_only: bool,
    controlled: bool,
}

#[derive(Debug, Default)]
pub(super) struct Graph {
    nodes: Vec<Option<Node>>,
    // Transform evaluation order only. Rendering keeps the authored order.
    order: Vec<usize>,
}

impl Graph {
    pub(super) fn contains(&self, index: usize) -> bool {
        self.node(index).is_some()
    }

    pub(super) fn is_transform_only(&self, index: usize) -> bool {
        self.node(index).is_some_and(|node| node.transform_only)
    }

    pub(super) fn is_child(&self, index: usize) -> bool {
        self.node(index).is_some_and(|node| node.parent.is_some())
    }

    pub(super) fn has_children(&self, index: usize) -> bool {
        self.nodes
            .iter()
            .flatten()
            .any(|node| node.parent == Some(index))
    }

    pub(super) fn is_dynamic(&self) -> bool {
        self.nodes.iter().flatten().any(|node| node.controlled)
    }

    fn node(&self, index: usize) -> Option<&Node> {
        self.nodes.get(index).and_then(Option::as_ref)
    }

    pub(super) fn resolve(&self, states: &[LayerState]) -> Result<Vec<Option<Resolved>>> {
        self.resolve_nodes(states)
            .map_err(|error| unsupported("hierarchy", format!("{error:#}")))
    }

    fn resolve_nodes(&self, states: &[LayerState]) -> Result<Vec<Option<Resolved>>> {
        let mut result: Vec<Option<Resolved>> = vec![None; self.nodes.len()];
        for &index in &self.order {
            let node = self.nodes[index].context("Scene hierarchy is missing a local transform")?;
            let mut layer = if node.controlled && !states.is_empty() {
                let state = states
                    .iter()
                    .find(|state| state.handle == index + 1)
                    .context("Scene hierarchy is missing a controlled layer state")?;
                Resolved {
                    world: planar_transform(state.origin, state.angles, state.scale)?,
                    visible: state.visible,
                }
            } else {
                node.authored
            };
            if let Some(parent) = node.parent {
                let parent =
                    result[parent].context("Scene hierarchy is missing a resolved parent")?;
                layer.world = parent.world.compose(layer.world);
                layer.visible &= parent.visible;
            }
            ensure!(
                layer
                    .world
                    .x
                    .into_iter()
                    .chain(layer.world.y)
                    .chain(layer.world.origin)
                    .all(f32::is_finite),
                "Scene hierarchy world transform exceeds numeric range"
            );
            result[index] = Some(layer);
        }
        Ok(result)
    }
}

pub(super) fn is_transform_only(object: &Value) -> bool {
    object.as_object().is_some_and(|fields| {
        fields.keys().all(|key| {
            matches!(
                key.as_str(),
                "id" | "name" | "origin" | "angles" | "scale" | "visible" | "parent" | "attachment"
            )
        })
    })
}

fn planar_transform(origin: [f32; 3], angles: [f32; 3], scale: [f32; 3]) -> Result<Transform> {
    let [x, y, z] = origin;
    let [rx, ry, rz] = angles;
    let [sx, sy, _] = scale;
    ensure!(
        z == 0.
            && rx == 0.
            && ry == 0.
            && origin
                .into_iter()
                .chain(angles)
                .chain(scale)
                .all(f32::is_finite),
        "Scene hierarchy requires a finite planar transform"
    );
    let (sin, cos) = rz.sin_cos();
    Ok(Transform {
        x: [cos * sx, sin * sx],
        y: [-sin * sy, cos * sy],
        origin: [x, y],
    })
}

pub(super) fn prepare(
    objects: &[Value],
    controlled: &BTreeSet<usize>,
    canvas: [f32; 2],
) -> Result<Graph> {
    if !objects
        .iter()
        .any(|object| object.get("parent").is_some() || object.get("attachment").is_some())
    {
        return Ok(Graph::default());
    }
    prepare_graph(objects, controlled, canvas)
        .map_err(|error| unsupported("hierarchy", format!("{error:#}")))
}

fn prepare_graph(
    objects: &[Value],
    controlled: &BTreeSet<usize>,
    canvas: [f32; 2],
) -> Result<Graph> {
    let mut ids = BTreeMap::new();
    for (index, object) in objects.iter().enumerate() {
        if let Some(id) = object.get("id") {
            let id = id.as_u64().context("Invalid layer identity")?;
            ensure!(
                ids.insert(id, index).is_none(),
                "Duplicate Scene layer identity"
            );
        }
    }
    let mut members = vec![false; objects.len()];
    let mut parents = vec![None; objects.len()];
    for (index, object) in objects.iter().enumerate() {
        ensure!(
            object
                .get("attachment")
                .is_none_or(|v| v.as_str() == Some("")),
            "Named layer attachments require model support"
        );
        if let Some(parent) = object.get("parent") {
            let id = parent
                .as_u64()
                .filter(|id| *id != 0)
                .context("Scene parent requires a positive layer ID")?;
            let parent = *ids.get(&id).context("Scene parent layer is missing")?;
            parents[index] = Some(parent);
            members[index] = true;
            members[parent] = true;
        }
    }
    let mut nodes = vec![None; objects.len()];
    for (index, member) in members.into_iter().enumerate() {
        if !member {
            continue;
        }
        let object = &objects[index];
        let transform_only = is_transform_only(object);
        ensure!(
            transform_only
                || (object["image"].is_string()
                    && [
                        "text",
                        "particle",
                        "sound",
                        "model",
                        "puppet",
                        "animation",
                        "mesh"
                    ]
                    .iter()
                    .all(|key| object.get(key).is_none())),
            "Scene hierarchy requires planar image or transform layers"
        );
        ensure!(
            !boolean(object, "perspective", false)?
                && !boolean(object, "disablepropagation", false)?,
            "Perspective and propagation overrides are unsupported in Scene hierarchy"
        );
        ensure!(
            !boolean(object, "copybackground", false)?,
            "Background-copy layers are unsupported in Scene hierarchy"
        );
        ensure!(
            (parents[index].is_none() && !transform_only) || object.get("origin").is_some(),
            "Scene transform layer or parented image requires an explicit origin"
        );
        let origin = vector(object, "origin", [canvas[0] / 2., canvas[1] / 2., 0.])?;
        let angles = vector(object, "angles", [0.; 3])?;
        let scale = vector(object, "scale", [1.; 3])?;
        ensure!(
            scale[0] != 0. && scale[1] != 0.,
            "Scene hierarchy requires a nondegenerate planar transform"
        );
        nodes[index] = Some(Node {
            parent: parents[index],
            authored: Resolved {
                world: planar_transform(origin, angles, scale)?,
                visible: boolean(object, "visible", true)?,
            },
            transform_only,
            controlled: controlled.contains(&(index + 1)),
        });
    }

    let mut marks = vec![Visit::Pending; objects.len()];
    let mut order = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        if node.is_some() {
            order_node(index, &parents, &mut marks, &mut order)?;
        }
    }
    let graph = Graph { nodes, order };
    graph.resolve_nodes(&[])?;
    Ok(graph)
}

#[derive(Clone, Copy)]
enum Visit {
    Pending,
    Visiting,
    Done,
}

fn order_node(
    index: usize,
    parents: &[Option<usize>],
    marks: &mut [Visit],
    order: &mut Vec<usize>,
) -> Result<()> {
    match marks[index] {
        Visit::Done => return Ok(()),
        Visit::Visiting => anyhow::bail!("Scene layer hierarchy contains a cycle"),
        Visit::Pending => {}
    }
    marks[index] = Visit::Visiting;
    if let Some(parent) = parents[index] {
        order_node(parent, parents, marks, order)?;
    }
    marks[index] = Visit::Done;
    order.push(index);
    Ok(())
}
