//! Native port of `blueprint/src/lib/explorer/layout.mjs`.
//!
//! Deterministic spherical/orb layout math for the local explorer UI. No
//! native equivalent was found (confirmed via grep across
//! engine/crates/membrane-blueprint and engine/crates/membrane-federation
//! for `sphericalLayout`/`projectOrb`/`GOLDEN_ANGLE`/`serveExplorerAsset`) --
//! this is a faithful, standalone port.

use serde_json::{Map, Value};
use std::f64::consts::PI;

fn golden_angle() -> f64 {
    PI * (3.0 - 5f64.sqrt())
}

#[derive(Debug, Clone)]
pub struct LaidOutNode {
    pub fields: Map<String, Value>,
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Mirrors `sphericalLayout(nodes, { radius = 240 })`.
///
/// Nodes are sorted by `String(id).localeCompare` order (mirrored here as a
/// plain UTF-8 byte-order sort, matching the default/"en" collation for the
/// ASCII identifiers this layout is used with) before placement, so layout is
/// deterministic and independent of input order.
pub fn spherical_layout(nodes: &[Value], radius: f64) -> Vec<LaidOutNode> {
    let mut ordered: Vec<&Value> = nodes.iter().collect();
    ordered.sort_by(|a, b| node_id(a).cmp(&node_id(b)));
    let count = ordered.len();
    ordered
        .into_iter()
        .enumerate()
        .map(|(index, node)| {
            let fields = node.as_object().cloned().unwrap_or_default();
            let id = node_id(node);
            if count == 1 {
                return LaidOutNode { fields, id, x: 0.0, y: 0.0, z: 0.0 };
            }
            let y = 1.0 - (index as f64 / (count as f64 - 1.0).max(1.0)) * 2.0;
            let ring = (1.0 - y * y).max(0.0).sqrt();
            let theta = golden_angle() * index as f64;
            LaidOutNode { fields, id, x: theta.cos() * ring * radius, y: y * radius, z: theta.sin() * ring * radius }
        })
        .collect()
}

fn node_id(node: &Value) -> String {
    node.get("id").map(|v| match v { Value::String(s) => s.clone(), other => other.to_string() }).unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct ProjectedNode {
    pub node: LaidOutNode,
    pub screen_x: f64,
    pub screen_y: f64,
    pub scale: f64,
    pub depth: f64,
}

/// Mirrors `projectOrb(nodes, { yaw = 0, pitch = 0, distance = 720 })`.
pub fn project_orb(nodes: &[LaidOutNode], yaw: f64, pitch: f64, distance: f64) -> Vec<ProjectedNode> {
    let (cy, sy, cp, sp) = (yaw.cos(), yaw.sin(), pitch.cos(), pitch.sin());
    nodes
        .iter()
        .map(|node| {
            let x1 = node.x * cy - node.z * sy;
            let z1 = node.x * sy + node.z * cy;
            let y1 = node.y * cp - z1 * sp;
            let z2 = node.y * sp + z1 * cp;
            let scale = distance / (distance + z2).max(1.0);
            ProjectedNode { node: node.clone(), screen_x: x1 * scale, screen_y: y1 * scale, scale, depth: z2 }
        })
        .collect()
}
