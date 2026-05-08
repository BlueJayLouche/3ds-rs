//! Pure-Rust parser for Autodesk 3D Studio binary (`.3ds`) files.
//!
//! # Quick start
//! ```no_run
//! use ds3::parse;
//!
//! let scene = parse(&std::fs::read("model.3ds").unwrap()).unwrap();
//! for mesh in &scene.meshes {
//!     println!("{}: {} vertices, {} faces", mesh.name, mesh.vertices.len(), mesh.faces.len());
//! }
//! ```
//!
//! # Coordinate system
//! 3DS stores vertices in right-handed Y-up (same as Bevy). No axis permutation
//! is needed. The [`Mesh3ds::transform`] matrix is also Y-up.
//!
//! Some exporters write Z-up data regardless of the spec; callers that know their
//! source is Z-up should swap Y/Z themselves after calling [`parse`].

mod chunk;
mod convert;
mod geometry;

use chunk::{walk_chunks, walk_chunks_from, Chunk};
use geometry::*;

// Re-export conversion helpers.
pub use convert::MeshBuffers;

use thiserror::Error;

/// Errors that can occur while parsing a `.3ds` file.
#[derive(Debug, Error)]
pub enum Error3ds {
    /// Data is too short to even read the 6-byte chunk header.
    #[error("data too short (need >= 6 bytes, got {0})")]
    Truncated(usize),
    /// Magic number `MAIN3DS` (0x4D4D) was not found at offset 0.
    #[error("not a 3DS file — expected MAIN3DS (0x4D4D), got 0x{0:04X}")]
    NotA3ds(u16),
    /// A child chunk claims a length that exceeds its parent's bounds.
    #[error("chunk 0x{id:04X} at offset {offset} length {length} exceeds parent bounds")]
    ChunkOverflow {
        /// Chunk ID.
        id: u16,
        /// Byte offset within the slice.
        offset: usize,
        /// Claimed chunk length.
        length: u32,
    },
    /// A walk-from offset falls past the chunk's end boundary.
    #[error("offset {start} is past the end of chunk 0x{id:04X} (end {end})")]
    BadOffset {
        /// Chunk ID of the parent.
        id: u16,
        /// The requested start offset.
        start: usize,
        /// The chunk's end offset.
        end: usize,
    },
}

/// A complete scene read from a `.3ds` file.
#[derive(Debug, Default, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Scene3ds {
    /// All meshes found in the file.
    pub meshes: Vec<Mesh3ds>,
    /// All material names collected from `MAT_ENTRY` chunks.
    pub materials: Vec<String>,
}

/// A single triangle mesh extracted from a `.3ds` file.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mesh3ds {
    /// Object name (from the `NAMED_OBJECT` chunk).
    pub name: String,
    /// Vertex positions.
    pub vertices: Vec<[f32; 3]>,
    /// Face indices into [`vertices`](Self::vertices).
    pub faces: Vec<[u16; 3]>,
    /// Texture coordinates. Length equals [`vertices`](Self::vertices) or zero.
    pub uvs: Vec<[f32; 2]>,
    /// Per-face smooth-group bitmask. Length equals [`faces`](Self::faces) or zero.
    /// `0` means flat-shaded for that face.
    pub smooth_groups: Vec<u32>,
    /// Row-major 4×3 local-to-world transform. Default is identity.
    pub transform: [[f32; 3]; 4],
    /// Material name assigned to this mesh (from the first `MSH_MAT_GROUP`).
    pub material: Option<String>,
}

impl Default for Mesh3ds {
    fn default() -> Self {
        Self {
            name: String::new(),
            vertices: Vec::new(),
            faces: Vec::new(),
            uvs: Vec::new(),
            smooth_groups: Vec::new(),
            transform: [
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0],
            ],
            material: None,
        }
    }
}

impl Mesh3ds {
    /// Flat-shaded buffers: one vertex per face corner (no shared vertices).
    ///
    /// Returns `(positions, normals, uvs, indices)` where `indices` is a
    /// sequential `0..faces*3` index buffer.
    ///
    /// Memory cost is `O(faces)`.
    pub fn to_flat_buffers(&self) -> MeshBuffers {
        convert::to_flat(self)
    }

    /// Smooth-shaded buffers: vertices shared across faces in the same smooth
    /// group. Normals are area-weighted and averaged per vertex group.
    ///
    /// UV seams are preserved — a vertex shared across a UV seam gets duplicated
    /// into two entries with different UVs but the same normal.
    ///
    /// If [`smooth_groups`](Self::smooth_groups) is empty, falls back to flat
    /// shading.
    pub fn to_smooth_buffers(&self) -> MeshBuffers {
        convert::to_smooth(self)
    }
}

/// Parse a `.3ds` file from a byte slice.
///
/// # Errors
///
/// Returns [`Error3ds::Truncated`] if `data` is shorter than 6 bytes, or
/// [`Error3ds::NotA3ds`] if the first chunk is not `MAIN3DS` (0x4D4D).
pub fn parse(data: &[u8]) -> Result<Scene3ds, Error3ds> {
    if data.len() < 6 {
        return Err(Error3ds::Truncated(data.len()));
    }

    let root = Chunk::read_at(data, 0)?;
    if root.id != 0x4D4D {
        return Err(Error3ds::NotA3ds(root.id));
    }

    let mut scene = Scene3ds::default();
    parse_main(data, &root, &mut scene)?;
    Ok(scene)
}

fn parse_main(data: &[u8], chunk: &Chunk, scene: &mut Scene3ds) -> Result<(), Error3ds> {
    for child in walk_chunks(data, chunk)? {
        let child = child?;
        if child.id == 0x3D3D {
            parse_edit(data, &child, scene)?;
        }
    }
    Ok(())
}

fn parse_edit(data: &[u8], chunk: &Chunk, scene: &mut Scene3ds) -> Result<(), Error3ds> {
    for child in walk_chunks(data, chunk)? {
        let child = child?;
        match child.id {
            0x4000 => {
                if let Some(mesh) = parse_named_object(data, &child)? {
                    scene.meshes.push(mesh);
                }
            }
            0xAFFF => {
                if let Some(name) = parse_mat_entry(data, &child)? {
                    scene.materials.push(name);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_named_object(data: &[u8], chunk: &Chunk) -> Result<Option<Mesh3ds>, Error3ds> {
    let name = read_cstring(data, chunk.data_start)?;
    let name_end = chunk.data_start + name.len() + 1; // +1 for null byte

    for child in walk_chunks_from(data, chunk, name_end)? {
        let child = child?;
        if child.id == 0x4100 {
            let mut mesh = parse_tri_object(data, &child)?;
            mesh.name = name;
            return Ok(Some(mesh));
        }
    }
    Ok(None)
}

fn parse_tri_object(data: &[u8], chunk: &Chunk) -> Result<Mesh3ds, Error3ds> {
    let mut mesh = Mesh3ds::default();

    for child in walk_chunks(data, chunk)? {
        let child = child?;
        match child.id {
            0x4110 => mesh.vertices = read_point_array(data, &child)?,
            0x4120 => {
                mesh.faces = read_face_array(data, &child)?;
            }
            0x4130 if mesh.material.is_none() => {
                mesh.material = read_msh_mat_group_name(data, &child)?;
            }
            0x4140 => mesh.uvs = read_tex_verts(data, &child)?,
            0x4150 => mesh.smooth_groups = read_smooth_group(data, &child)?,
            0x4160 => mesh.transform = read_mesh_matrix(data, &child)?,
            _ => {}
        }
    }

    Ok(mesh)
}

fn parse_mat_entry(data: &[u8], chunk: &Chunk) -> Result<Option<String>, Error3ds> {
    for child in walk_chunks(data, chunk)? {
        let child = child?;
        if child.id == 0xA000 {
            return Ok(Some(read_cstring(data, child.data_start)?));
        }
    }
    Ok(None)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    fn write_chunk(id: u16, data: &[u8], buf: &mut Vec<u8>) {
        buf.extend_from_slice(&id.to_le_bytes());
        let len = (6 + data.len()) as u32;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(data);
    }

    fn write_point_array(verts: &[[f32; 3]], buf: &mut Vec<u8>) {
        let mut data = Vec::new();
        data.extend_from_slice(&(verts.len() as u16).to_le_bytes());
        for v in verts {
            for c in v {
                data.extend_from_slice(&c.to_le_bytes());
            }
        }
        write_chunk(0x4110, &data, buf);
    }

    fn write_face_array(faces: &[[u16; 3]], buf: &mut Vec<u8>) {
        let mut data = Vec::new();
        data.extend_from_slice(&(faces.len() as u16).to_le_bytes());
        for f in faces {
            for &idx in f {
                data.extend_from_slice(&idx.to_le_bytes());
            }
            // flags
            data.extend_from_slice(&0u16.to_le_bytes());
        }
        write_chunk(0x4120, &data, buf);
    }

    fn write_tex_verts(uvs: &[[f32; 2]], buf: &mut Vec<u8>) {
        let mut data = Vec::new();
        data.extend_from_slice(&(uvs.len() as u16).to_le_bytes());
        for uv in uvs {
            for c in uv {
                data.extend_from_slice(&c.to_le_bytes());
            }
        }
        write_chunk(0x4140, &data, buf);
    }

    fn write_smooth_groups(groups: &[u32], buf: &mut Vec<u8>) {
        let mut data = Vec::new();
        for g in groups {
            data.extend_from_slice(&g.to_le_bytes());
        }
        write_chunk(0x4150, &data, buf);
    }

    fn write_mesh_matrix(mat: &[[f32; 3]; 4], buf: &mut Vec<u8>) {
        let mut data = Vec::new();
        for row in mat {
            for c in row {
                data.extend_from_slice(&c.to_le_bytes());
            }
        }
        write_chunk(0x4160, &data, buf);
    }

    fn write_msh_mat_group(name: &str, buf: &mut Vec<u8>) {
        let mut data = Vec::new();
        data.extend_from_slice(name.as_bytes());
        data.push(0);
        data.extend_from_slice(&0u16.to_le_bytes()); // face count = 0
        write_chunk(0x4130, &data, buf);
    }

    fn write_mat_entry(name: &str, buf: &mut Vec<u8>) {
        let mut data = Vec::new();
        data.extend_from_slice(name.as_bytes());
        data.push(0);
        let mut entry = Vec::new();
        write_chunk(0xA000, &data, &mut entry);
        write_chunk(0xAFFF, &entry, buf);
    }

    fn build_cube_3ds(smooth: bool) -> Vec<u8> {
        let verts: Vec<[f32; 3]> = vec![
            [0.0, 0.0, 0.0], // 0
            [1.0, 0.0, 0.0], // 1
            [1.0, 1.0, 0.0], // 2
            [0.0, 1.0, 0.0], // 3
            [0.0, 0.0, 1.0], // 4
            [1.0, 0.0, 1.0], // 5
            [1.0, 1.0, 1.0], // 6
            [0.0, 1.0, 1.0], // 7
        ];

        let faces: Vec<[u16; 3]> = vec![
            // front (z=0)
            [0, 1, 2], [0, 2, 3],
            // back (z=1)
            [5, 4, 7], [5, 7, 6],
            // bottom (y=0)
            [4, 5, 1], [4, 1, 0],
            // top (y=1)
            [2, 6, 7], [2, 7, 3],
            // left (x=0)
            [4, 0, 3], [4, 3, 7],
            // right (x=1)
            [1, 5, 6], [1, 6, 2],
        ];

        let uvs: Vec<[f32; 2]> = vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ];

        let mut tri = Vec::new();
        write_point_array(&verts, &mut tri);
        write_face_array(&faces, &mut tri);
        write_tex_verts(&uvs, &mut tri);
        if smooth {
            let groups = vec![1u32; faces.len()];
            write_smooth_groups(&groups, &mut tri);
        }

        let mut named = Vec::new();
        named.extend_from_slice(b"Cube\0");
        write_chunk(0x4100, &tri, &mut named);

        let mut edit = Vec::new();
        write_chunk(0x4000, &named, &mut edit);
        write_mat_entry("Default", &mut edit);

        let mut main = Vec::new();
        write_chunk(0x3D3D, &edit, &mut main);

        let mut file = Vec::new();
        write_chunk(0x4D4D, &main, &mut file);
        file
    }

    #[test]
    fn test_truncated() {
        assert!(matches!(parse(&[]), Err(Error3ds::Truncated(0))));
        assert!(matches!(parse(&[0x4D, 0x4D]), Err(Error3ds::Truncated(2))));
    }

    #[test]
    fn test_not_a_3ds() {
        let mut buf = Vec::new();
        write_chunk(0x1234, &[], &mut buf);
        assert!(matches!(parse(&buf), Err(Error3ds::NotA3ds(0x1234))));
    }

    #[test]
    fn test_parse_cube() {
        let data = build_cube_3ds(false);
        let scene = parse(&data).unwrap();
        assert_eq!(scene.meshes.len(), 1);
        let mesh = &scene.meshes[0];
        assert_eq!(mesh.name, "Cube");
        assert_eq!(mesh.vertices.len(), 8);
        assert_eq!(mesh.faces.len(), 12);
        assert_eq!(mesh.uvs.len(), 8);
        assert!(mesh.smooth_groups.is_empty());
        assert_eq!(scene.materials, vec!["Default"]);
    }

    #[test]
    fn test_flat_buffers() {
        let data = build_cube_3ds(false);
        let scene = parse(&data).unwrap();
        let mesh = &scene.meshes[0];
        let (pos, nrm, uv, idx) = mesh.to_flat_buffers();
        assert_eq!(pos.len(), mesh.faces.len() * 3);
        assert_eq!(nrm.len(), mesh.faces.len() * 3);
        assert_eq!(uv.len(), mesh.faces.len() * 3);
        assert_eq!(idx.len(), mesh.faces.len() * 3);

        // All normals should be unit length
        for n in &nrm {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-5, "normal length = {}", len);
        }
    }

    #[test]
    fn test_smooth_buffers() {
        let data = build_cube_3ds(true);
        let scene = parse(&data).unwrap();
        let mesh = &scene.meshes[0];
        let (pos, nrm, _uv, idx) = mesh.to_smooth_buffers();

        // With all faces in smooth group 1, a cube should share more vertices
        // than flat shading (which has 36 = 12*3).
        assert!(pos.len() < 36, "smooth should share vertices, got {}", pos.len());
        assert_eq!(idx.len(), 36);

        // All normals should be unit length
        for n in &nrm {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-5, "normal length = {}", len);
        }
    }

    #[test]
    fn test_smooth_fallback_when_empty() {
        let data = build_cube_3ds(false);
        let scene = parse(&data).unwrap();
        let mesh = &scene.meshes[0];
        let flat = mesh.to_flat_buffers();
        let smooth = mesh.to_smooth_buffers();
        assert_eq!(flat.0.len(), smooth.0.len());
    }

    #[test]
    fn test_transform_default_identity() {
        let data = build_cube_3ds(false);
        let scene = parse(&data).unwrap();
        let mesh = &scene.meshes[0];
        assert_eq!(mesh.transform[0], [1.0, 0.0, 0.0]);
        assert_eq!(mesh.transform[1], [0.0, 1.0, 0.0]);
        assert_eq!(mesh.transform[2], [0.0, 0.0, 1.0]);
        assert_eq!(mesh.transform[3], [0.0, 0.0, 0.0]);
    }
}
