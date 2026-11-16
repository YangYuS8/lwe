//! Prepare authored, planar layer placements before resource loading.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde_json::Value;

use super::{boolean, unsupported, vector};

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

#[derive(Clone, Copy, Default)]
pub(super) struct Placement {
    pub(super) member: bool,
    pub(super) transform_only: bool,
    pub(super) world: Option<Transform>,
}

pub(super) fn prepare(
    objects: &[Value],
    controlled: &BTreeSet<usize>,
    canvas: [f32; 2],
) -> Result<Vec<Placement>> {
    if !objects
        .iter()
        .any(|object| object.get("parent").is_some() || object.get("attachment").is_some())
    {
        return Ok(Vec::new());
    }
    prepare_graph(objects, controlled, canvas)
        .map_err(|error| unsupported("hierarchy", format!("{error:#}")))
}

fn prepare_graph(
    objects: &[Value],
    controlled: &BTreeSet<usize>,
    canvas: [f32; 2],
) -> Result<Vec<Placement>> {
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
    let mut placements = vec![Placement::default(); objects.len()];
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
            ensure!(
                boolean(&objects[parent], "visible", true)?,
                "Hidden Scene parent layers are unsupported"
            );
            parents[index] = Some(parent);
            placements[index].member = true;
            placements[parent].member = true;
        }
    }
    let mut local = vec![None; objects.len()];
    for (index, placement) in placements.iter_mut().enumerate() {
        if !placement.member {
            continue;
        }
        let object = &objects[index];
        ensure!(
            !controlled.contains(&(index + 1)),
            "Script-controlled Scene hierarchy is unsupported"
        );
        placement.transform_only = object.as_object().is_some_and(|fields| {
            fields.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "id" | "name"
                        | "origin"
                        | "angles"
                        | "scale"
                        | "visible"
                        | "parent"
                        | "attachment"
                )
            })
        });
        ensure!(
            placement.transform_only
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
            (parents[index].is_none() && !placement.transform_only)
                || object.get("origin").is_some(),
            "Scene transform layer or parented image requires an explicit origin"
        );
        let [x, y, z] = vector(object, "origin", [canvas[0] / 2., canvas[1] / 2., 0.])?;
        let [rx, ry, rz] = vector(object, "angles", [0.; 3])?;
        let [sx, sy, _] = vector(object, "scale", [1.; 3])?;
        ensure!(
            z == 0. && rx == 0. && ry == 0. && sx != 0. && sy != 0.,
            "Scene hierarchy requires a nondegenerate planar transform"
        );
        let (sin, cos) = rz.sin_cos();
        local[index] = Some(Transform {
            x: [cos * sx, sin * sx],
            y: [-sin * sy, cos * sy],
            origin: [x, y],
        });
    }
    let mut visiting = vec![false; objects.len()];
    let mut world = vec![None; objects.len()];
    for (index, parent) in parents.iter().enumerate() {
        if parent.is_some() {
            placements[index].world =
                Some(resolve(index, &parents, &local, &mut visiting, &mut world)?);
        }
    }
    Ok(placements)
}

fn resolve(
    index: usize,
    parents: &[Option<usize>],
    local: &[Option<Transform>],
    visiting: &mut [bool],
    world: &mut [Option<Transform>],
) -> Result<Transform> {
    if let Some(transform) = world[index] {
        return Ok(transform);
    }
    ensure!(!visiting[index], "Scene layer hierarchy contains a cycle");
    visiting[index] = true;
    let mut transform = local[index].context("Scene hierarchy is missing a local transform")?;
    if let Some(parent) = parents[index] {
        transform = resolve(parent, parents, local, visiting, world)?.compose(transform);
    }
    ensure!(
        transform
            .x
            .into_iter()
            .chain(transform.y)
            .chain(transform.origin)
            .all(f32::is_finite),
        "Scene hierarchy world transform exceeds numeric range"
    );
    world[index] = Some(transform);
    visiting[index] = false;
    Ok(transform)
}
