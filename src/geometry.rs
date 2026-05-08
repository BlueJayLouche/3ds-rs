use crate::chunk::Chunk;
use crate::Error3ds;

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

/// Read a null-terminated ASCII string starting at `offset`.
pub(crate) fn read_cstring(data: &[u8], offset: usize) -> Result<String, Error3ds> {
    let mut end = offset;
    while end < data.len() && data[end] != 0 {
        end += 1;
    }
    // If we hit EOF without null, just take what we have.
    let bytes = &data[offset..end];
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// Parse `POINT_ARRAY` (0x4110): count u16 + count * 3 * f32.
pub(crate) fn read_point_array(data: &[u8], chunk: &Chunk) -> Result<Vec<[f32; 3]>, Error3ds> {
    let mut offset = chunk.data_start;
    let count = read_u16(data, &mut offset)? as usize;
    let mut verts = Vec::with_capacity(count);

    for _ in 0..count {
        let x = read_f32(data, &mut offset)?;
        let y = read_f32(data, &mut offset)?;
        let z = read_f32(data, &mut offset)?;
        verts.push([x, y, z]);
    }

    Ok(verts)
}

/// Parse `FACE_ARRAY` (0x4120): count u16 + count * (3 * u16 + u16 flags).
pub(crate) fn read_face_array(data: &[u8], chunk: &Chunk) -> Result<Vec<[u16; 3]>, Error3ds> {
    let mut offset = chunk.data_start;
    let count = read_u16(data, &mut offset)? as usize;
    let mut faces = Vec::with_capacity(count);

    for _ in 0..count {
        let a = read_u16(data, &mut offset)?;
        let b = read_u16(data, &mut offset)?;
        let c = read_u16(data, &mut offset)?;
        let _flags = read_u16(data, &mut offset)?;
        faces.push([a, b, c]);
    }

    Ok(faces)
}

/// Parse `TEX_VERTS` (0x4140): count u16 + count * 2 * f32.
pub(crate) fn read_tex_verts(data: &[u8], chunk: &Chunk) -> Result<Vec<[f32; 2]>, Error3ds> {
    let mut offset = chunk.data_start;
    let count = read_u16(data, &mut offset)? as usize;
    let mut uvs = Vec::with_capacity(count);

    for _ in 0..count {
        let u = read_f32(data, &mut offset)?;
        let v = read_f32(data, &mut offset)?;
        uvs.push([u, v]);
    }

    Ok(uvs)
}

/// Parse `SMOOTH_GROUP` (0x4150): count * u32 per face.
pub(crate) fn read_smooth_group(data: &[u8], chunk: &Chunk) -> Result<Vec<u32>, Error3ds> {
    let mut offset = chunk.data_start;
    // The count isn't explicitly stored; infer from chunk size.
    let available = chunk.end.saturating_sub(offset);
    let count = available / 4;
    let mut groups = Vec::with_capacity(count);

    for _ in 0..count {
        groups.push(read_u32(data, &mut offset)?);
    }

    Ok(groups)
}

/// Parse `MESH_MATRIX` (0x4160): 4 * 3 f32 row-major.
pub(crate) fn read_mesh_matrix(data: &[u8], chunk: &Chunk) -> Result<[[f32; 3]; 4], Error3ds> {
    let mut offset = chunk.data_start;
    let mut mat = [[0.0f32; 3]; 4];

    for row in &mut mat {
        for col in row {
            *col = read_f32(data, &mut offset)?;
        }
    }

    Ok(mat)
}

/// Parse `MSH_MAT_GROUP` (0x4130): material name (cstring) + face count u16 + face indices.
/// Returns just the material name.
pub(crate) fn read_msh_mat_group_name(
    data: &[u8],
    chunk: &Chunk,
) -> Result<Option<String>, Error3ds> {
    let name = read_cstring(data, chunk.data_start)?;
    if name.is_empty() {
        return Ok(None);
    }
    Ok(Some(name))
}

// ── Little-endian helpers ────────────────────────────────────────────────────

fn read_u16(data: &[u8], offset: &mut usize) -> Result<u16, Error3ds> {
    if *offset + 2 > data.len() {
        return Err(Error3ds::Truncated(data.len()));
    }
    let val = u16::from_le_bytes([data[*offset], data[*offset + 1]]);
    *offset += 2;
    Ok(val)
}

fn read_u32(data: &[u8], offset: &mut usize) -> Result<u32, Error3ds> {
    if *offset + 4 > data.len() {
        return Err(Error3ds::Truncated(data.len()));
    }
    let val = u32::from_le_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
    ]);
    *offset += 4;
    Ok(val)
}

fn read_f32(data: &[u8], offset: &mut usize) -> Result<f32, Error3ds> {
    if *offset + 4 > data.len() {
        return Err(Error3ds::Truncated(data.len()));
    }
    let val = f32::from_le_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
    ]);
    *offset += 4;
    Ok(val)
}
