//! The serialized mesh format, shared with the TypeScript app's JSON documents.
//!
//! `MeshData` is used only at the file boundary (save, load, import, export). Editing and undo
//! work on [`EditableMesh`] directly.
//!
//! - Vertex and face ids are file-local. Loading assigns fresh, dense mesh handles in file order;
//!   saving writes each element's mesh index as its id.
//! - Edges are implicit (derived from face loops), except wire edges that border no face.
//! - Each face corner carries its own UV, so UV seams are implicit.
//! - `loops[0]` is the outer boundary, counter-clockwise about the face normal; `loops[1..]` are
//!   holes, clockwise.
//! - Edges are hard unless listed in `soft_edges`.

use std::collections::HashMap;

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

use crate::mesh::{Corner, EditableMesh, MaterialId, MeshError, Shading, VertId};

/// A mesh as stored in documents.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshData {
    /// Vertices.
    pub verts: Vec<VertData>,
    /// Faces.
    pub faces: Vec<FaceData>,
    /// Edges with no face on either side, as vertex id pairs (smaller first).
    pub wire_edges: Vec<[u32; 2]>,
    /// Edges with smoothed shading, as vertex id pairs (smaller first). Older files lack it.
    #[serde(default)]
    pub soft_edges: Vec<[u32; 2]>,
    /// One more than the largest vertex id.
    pub next_vert_id: u32,
    /// One more than the largest face id.
    pub next_face_id: u32,
}

/// A vertex as stored in documents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VertData {
    /// File-local id.
    pub id: u32,
    /// Position, Z-up.
    pub p: [f64; 3],
}

/// A face as stored in documents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceData {
    /// File-local id.
    pub id: u32,
    /// Document material id.
    pub material: u32,
    /// Outer loop, then holes.
    pub loops: Vec<Vec<CornerData>>,
}

/// A face corner as stored in documents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CornerData {
    /// File-local vertex id.
    pub v: u32,
    /// UV, in world units.
    pub uv: [f64; 2],
}

/// Why [`MeshData`] could not be loaded.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum LoadMeshError {
    /// Two vertices share an id.
    #[error("vertex id {0} already in use")]
    DuplicateVertexId(u32),
    /// Two faces share an id.
    #[error("face id {0} already in use")]
    DuplicateFaceId(u32),
    /// A face or edge refers to a vertex id that isn't defined.
    #[error("no vertex with id {0}")]
    UnknownVertex(u32),
    /// A face is not valid in a manifold mesh.
    #[error("invalid face {id}: {source}")]
    InvalidFace {
        /// The face's file id.
        id: u32,
        /// What was wrong.
        source: MeshError,
    },
    /// A wire edge is not valid.
    #[error("invalid wire edge {a}-{b}: {source}")]
    InvalidWireEdge {
        /// First vertex id.
        a: u32,
        /// Second vertex id.
        b: u32,
        /// What was wrong.
        source: MeshError,
    },
    /// A soft edge names two vertices that no edge joins.
    #[error("soft edge {0}-{1} does not exist")]
    UnknownSoftEdge(u32, u32),
}

impl EditableMesh {
    /// Builds a mesh from its stored form. Element order is preserved, as is each loop's
    /// starting corner, so `EditableMesh::from_data(d).to_data()` reproduces `d` when its ids are already
    /// dense and in order.
    ///
    /// # Errors
    ///
    /// [`LoadMeshError`] if the data is not a valid manifold mesh.
    pub fn from_data(data: &MeshData) -> Result<Self, LoadMeshError> {
        let mut mesh = Self::new();
        let mut verts: HashMap<u32, VertId> = HashMap::with_capacity(data.verts.len());
        for v in &data.verts {
            let handle = mesh.add_vertex(DVec3::from(v.p));
            if verts.insert(v.id, handle).is_some() {
                return Err(LoadMeshError::DuplicateVertexId(v.id));
            }
        }
        let vert = |id: u32| {
            verts
                .get(&id)
                .copied()
                .ok_or(LoadMeshError::UnknownVertex(id))
        };

        let mut face_ids: Vec<u32> = data.faces.iter().map(|f| f.id).collect();
        face_ids.sort_unstable();
        if let Some(w) = face_ids.windows(2).find(|w| w[0] == w[1]) {
            return Err(LoadMeshError::DuplicateFaceId(w[0]));
        }
        for f in &data.faces {
            let loops = f
                .loops
                .iter()
                .map(|l| {
                    l.iter()
                        .map(|c| {
                            Ok(Corner {
                                v: vert(c.v)?,
                                uv: DVec2::from(c.uv),
                            })
                        })
                        .collect()
                })
                .collect::<Result<Vec<Vec<Corner>>, LoadMeshError>>()?;
            mesh.add_face(&loops, MaterialId(f.material))
                .map_err(|source| LoadMeshError::InvalidFace { id: f.id, source })?;
        }

        for &[a, b] in &data.wire_edges {
            mesh.add_edge(vert(a)?, vert(b)?)
                .map_err(|source| LoadMeshError::InvalidWireEdge { a, b, source })?;
        }
        for &[a, b] in &data.soft_edges {
            let e = mesh
                .find_edge(vert(a)?, vert(b)?)
                .ok_or(LoadMeshError::UnknownSoftEdge(a, b))?;
            mesh.set_shading(e, Shading::Soft);
        }
        Ok(mesh)
    }

    /// The mesh's stored form, in canonical order: vertices and faces by index, wire and soft
    /// edges sorted.
    pub fn to_data(&self) -> MeshData {
        let verts = self
            .verts()
            .map(|v| VertData {
                id: v.index(),
                p: self.position(v).to_array(),
            })
            .collect();
        let faces = self
            .faces()
            .map(|f| FaceData {
                id: f.index(),
                material: self.material(f).0,
                loops: self
                    .face_corners(f)
                    .iter()
                    .map(|l| {
                        l.iter()
                            .map(|c| CornerData {
                                v: c.v.index(),
                                uv: c.uv.to_array(),
                            })
                            .collect()
                    })
                    .collect(),
            })
            .collect();

        let mut wire_edges = Vec::new();
        let mut soft_edges = Vec::new();
        for e in self.edges() {
            let [a, b] = self.edge_verts(e).map(VertId::index);
            let pair = [a.min(b), a.max(b)];
            if self.is_wire(e) {
                wire_edges.push(pair);
            }
            if self.shading(e) == Shading::Soft {
                soft_edges.push(pair);
            }
        }
        wire_edges.sort_unstable();
        soft_edges.sort_unstable();

        MeshData {
            verts,
            faces,
            wire_edges,
            soft_edges,
            next_vert_id: slot_count(self.vert_slots()),
            next_face_id: slot_count(self.face_slots()),
        }
    }
}

fn slot_count(slots: usize) -> u32 {
    u32::try_from(slots).expect("handles are u32, so slot counts fit")
}
