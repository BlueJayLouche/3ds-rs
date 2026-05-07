use crate::Mesh3ds;
use std::collections::HashMap;

/// Buffers produced by [`Mesh3ds::to_flat_buffers`] or [`Mesh3ds::to_smooth_buffers`].
///
/// The tuple layout is `(positions, normals, uvs, indices)`.
pub type MeshBuffers = (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>);

// ── Flat shading ─────────────────────────────────────────────────────────────

pub(crate) fn to_flat(mesh: &Mesh3ds) -> MeshBuffers {
    let face_count = mesh.faces.len();
    let mut positions = Vec::with_capacity(face_count * 3);
    let mut normals = Vec::with_capacity(face_count * 3);
    let mut uvs = Vec::with_capacity(face_count * 3);
    let mut indices = Vec::with_capacity(face_count * 3);

    for (fi, &[a, b, c]) in mesh.faces.iter().enumerate() {
        let va = mesh.vertices[a as usize];
        let vb = mesh.vertices[b as usize];
        let vc = mesh.vertices[c as usize];

        let n = face_normal(va, vb, vc);

        positions.push(va);
        positions.push(vb);
        positions.push(vc);

        normals.push(n);
        normals.push(n);
        normals.push(n);

        if let (Some(ua), Some(ub), Some(uc)) = (
            mesh.uvs.get(a as usize),
            mesh.uvs.get(b as usize),
            mesh.uvs.get(c as usize),
        ) {
            uvs.push(*ua);
            uvs.push(*ub);
            uvs.push(*uc);
        } else {
            uvs.push([0.0, 0.0]);
            uvs.push([0.0, 0.0]);
            uvs.push([0.0, 0.0]);
        }

        let base = (fi * 3) as u32;
        indices.push(base);
        indices.push(base + 1);
        indices.push(base + 2);
    }

    (positions, normals, uvs, indices)
}

// ── Smooth shading ───────────────────────────────────────────────────────────

pub(crate) fn to_smooth(mesh: &Mesh3ds) -> MeshBuffers {
    if mesh.smooth_groups.is_empty() || mesh.smooth_groups.len() != mesh.faces.len() {
        return to_flat(mesh);
    }

    // 1. Precompute area-weighted face normals.
    let face_normals: Vec<[f32; 3]> = mesh
        .faces
        .iter()
        .map(|&[a, b, c]| {
            let va = mesh.vertices[a as usize];
            let vb = mesh.vertices[b as usize];
            let vc = mesh.vertices[c as usize];
            area_weighted_normal(va, vb, vc)
        })
        .collect();

    // 2. Build incident-face list for each vertex.
    //    incident[v] = list of (face_index, corner_index_in_face)
    let mut incident: Vec<Vec<(usize, usize)>> = vec![Vec::new(); mesh.vertices.len()];
    for (fi, &[a, b, c]) in mesh.faces.iter().enumerate() {
        incident[a as usize].push((fi, 0));
        incident[b as usize].push((fi, 1));
        incident[c as usize].push((fi, 2));
    }

    // 3. For each face corner, compute the smoothed normal.
    //    corner_normals[face_index][corner_index] = normal
    let mut corner_normals: Vec<[[f32; 3]; 3]> = vec![[[0.0; 3]; 3]; mesh.faces.len()];

    for refs in &incident {
        if refs.is_empty() {
            continue;
        }

        // Union-find over the incident faces at this vertex.
        let mut uf = UnionFind::new(refs.len());
        for (i, &(f1, _)) in refs.iter().enumerate() {
            let sg1 = mesh.smooth_groups[f1];
            for (j, &(f2, _)) in refs.iter().enumerate().skip(i + 1) {
                let sg2 = mesh.smooth_groups[f2];
                if sg1 & sg2 != 0 {
                    uf.union(i, j);
                }
            }
        }

        // Sum area-weighted normals per component.
        let mut sums: HashMap<usize, [f32; 3]> = HashMap::new();
        for (idx, &(fi, _)) in refs.iter().enumerate() {
            let root = uf.find(idx);
            let n = face_normals[fi];
            let entry = sums.entry(root).or_insert([0.0; 3]);
            *entry = add(*entry, n);
        }

        // Normalize and assign back.
        let mut normalized: HashMap<usize, [f32; 3]> = HashMap::with_capacity(sums.len());
        for (&root, &sum) in &sums {
            normalized.insert(root, normalize(sum));
        }

        for (idx, &(fi, corner)) in refs.iter().enumerate() {
            let root = uf.find(idx);
            corner_normals[fi][corner] = normalized[&root];
        }
    }

    // 4. Build output mesh, deduplicating corners with identical (vertex, normal, uv).
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::with_capacity(mesh.faces.len() * 3);

    // Key: (vertex_index, normal_bits)
    // We quantize normals to f32 bit patterns for exact HashMap equality.
    let mut seen: HashMap<(u16, [u32; 3]), u32> = HashMap::new();

    for (fi, &[a, b, c]) in mesh.faces.iter().enumerate() {
        for (corner, &v) in [a, b, c].iter().enumerate() {
            let n = corner_normals[fi][corner];
            let uv = mesh
                .uvs
                .get(v as usize)
                .copied()
                .unwrap_or([0.0, 0.0]);

            let key = (v, [n[0].to_bits(), n[1].to_bits(), n[2].to_bits()]);

            let idx = if let Some(&idx) = seen.get(&key) {
                idx
            } else {
                let new_idx = positions.len() as u32;
                positions.push(mesh.vertices[v as usize]);
                normals.push(n);
                uvs.push(uv);
                seen.insert(key, new_idx);
                new_idx
            };

            indices.push(idx);
        }
    }

    (positions, normals, uvs, indices)
}

// ── Math helpers ─────────────────────────────────────────────────────────────

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn len_sq(a: [f32; 3]) -> f32 {
    dot(a, a)
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l2 = len_sq(a);
    if l2 > 0.0 {
        scale(a, 1.0 / l2.sqrt())
    } else {
        [0.0, 1.0, 0.0]
    }
}

fn face_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let e1 = sub(b, a);
    let e2 = sub(c, a);
    normalize(cross(e1, e2))
}

fn area_weighted_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let e1 = sub(b, a);
    let e2 = sub(c, a);
    // cross(e1, e2) has magnitude 2*area. We return 0.5*cross so magnitude == area.
    scale(cross(e1, e2), 0.5)
}

// ── Union-Find ───────────────────────────────────────────────────────────────

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    fn union(&mut self, x: usize, y: usize) {
        let px = self.find(x);
        let py = self.find(y);
        if px != py {
            self.parent[px] = py;
        }
    }
}
