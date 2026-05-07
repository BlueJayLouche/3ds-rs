# 3ds-rs — implementation plan

Pure-Rust `.3ds` (Autodesk 3D Studio binary) parser, publishable to crates.io.
Will replace the vendored `stagelx-3ds` crate in stageLX once stable.

---

## Why a standalone crate

`stagelx-3ds` lives inside the stageLX workspace as a quick bootstrap.  
It works but has no tests, no docs, no smooth-normal path, and no extended chunk
support (transforms, material names, smooth groups).  
A proper crate lets other GDTF / lighting-visualizer toolchains use it.

---

## Public API (target)

```rust
// ── Entry point ────────────────────────────────────────────────────────────
pub fn parse(data: &[u8]) -> Result<Scene3ds, Error3ds>;

// ── Types ──────────────────────────────────────────────────────────────────
pub struct Scene3ds {
    pub meshes: Vec<Mesh3ds>,
}

pub struct Mesh3ds {
    pub name:        String,
    pub vertices:    Vec<[f32; 3]>,
    pub faces:       Vec<[u16; 3]>,   // indices into vertices
    pub uvs:         Vec<[f32; 2]>,   // len == vertices.len() or 0
    pub smooth_groups: Vec<u32>,      // per-face bitmask (0 = flat), len == faces.len() or 0
    pub transform:   [[f32; 3]; 4],   // row-major 4×3 local→world, default = identity
}

impl Mesh3ds {
    // Flat-shaded: expands to one vertex per face corner (no shared vertices).
    // Normals computed from face cross-product.  O(faces) memory.
    pub fn to_flat_buffers(&self)
        -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>);

    // Smooth-shaded: vertices shared across faces in the same smooth group.
    // Normals area-weighted and averaged per vertex group.  Preserves UV seams.
    pub fn to_smooth_buffers(&self)
        -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>);
}

// ── Errors ─────────────────────────────────────────────────────────────────
#[derive(Debug, thiserror::Error)]
pub enum Error3ds {
    #[error("data too short (need ≥ 6 bytes, got {0})")]
    Truncated(usize),
    #[error("not a 3DS file — expected MAIN3DS (0x4D4D), got 0x{0:04X}")]
    NotA3ds(u16),
}
```

`to_flat_buffers` is a drop-in replacement for `stagelx_3ds::to_bevy_buffers`.  
`to_smooth_buffers` is the upgrade path for fixture bodies that look faceted today.

No Bevy dependency anywhere.  The return types are plain `[f32; N]` arrays —
callers pass them to `Mesh::insert_attribute` themselves.

Optional feature `serde` gates `#[derive(Serialize, Deserialize)]` on all public
types (useful for caching parsed scenes to disk).

---

## 3DS chunk reference

Only geometry-relevant chunks are handled; everything else is skipped silently.

| ID     | Name             | Action                                   |
|--------|------------------|------------------------------------------|
| 0x4D4D | MAIN3DS          | top-level container; walk children       |
| 0x3D3D | EDIT3DS          | edit block; walk children                |
| 0x4000 | NAMED_OBJECT     | null-terminated name + walk children     |
| 0x4100 | N_TRI_OBJECT     | triangle mesh; walk children             |
| 0x4110 | POINT_ARRAY      | `count:u16` + `count × 3×f32`            |
| 0x4120 | FACE_ARRAY       | `count:u16` + `count × (3×u16 + u16 flags)` |
| 0x4130 | MSH_MAT_GROUP    | material name + face list (store name only) |
| 0x4140 | TEX_VERTS        | `count:u16` + `count × 2×f32`            |
| 0x4150 | SMOOTH_GROUP     | `count × u32` bitmask per face           |
| 0x4160 | MESH_MATRIX      | `4×3` row-major f32 local transform      |
| 0xAFFF | MAT_ENTRY        | walk children for name                   |
| 0xA000 | MAT_NAME         | null-terminated material name (collect)  |

Chunks not in this table are walked if they are containers (ID < 0x4000 or
known range), otherwise skipped.

---

## Implementation phases

### Phase 1 — Bootstrap (copy + clean from stagelx-3ds)

- `cargo new --lib 3ds-rs`
- Copy parser logic from `stagelx-3ds/src/lib.rs`
- Add `Truncated(usize)` and `NotA3ds(u16)` error variants (improve over current)
- Expose `parse`, `Scene3ds`, `Mesh3ds`, `to_flat_buffers` (rename from `to_bevy_buffers`)
- Add `Cargo.toml` metadata: `description`, `keywords`, `categories`, `license`, `repository`
- Add MIT + Apache-2.0 dual licence files
- **Deliverable**: `cargo publish --dry-run` succeeds; stageLX can swap dependency

### Phase 2 — Extended chunk support

- Parse `SMOOTH_GROUP` (0x4150): store `Vec<u32>` in `Mesh3ds::smooth_groups`
- Parse `MESH_MATRIX` (0x4160): store `[[f32;3];4]` in `Mesh3ds::transform`
- Parse `MAT_ENTRY` / `MAT_NAME` (0xAFFF / 0xA000): collect `Vec<String>` in `Scene3ds::materials`
- Parse `MSH_MAT_GROUP` (0x4130): record per-mesh material name in `Mesh3ds::material`
- **Deliverable**: `Mesh3ds` carries transform + material name; smooth_groups populated

### Phase 3 — Smooth normal generation

`to_smooth_buffers` algorithm:
1. For each face, compute area-weighted normal (`cross × 0.5 * area`)
2. For each vertex, accumulate normals from all faces that share it AND share a
   smooth group bit (bitwise AND of smooth_group masks ≠ 0, or both == 0)
3. Normalize accumulated normals
4. Expand to triangle list, preserving UV seams (a vertex shared across a UV seam
   gets duplicated into two entries with different UVs but the same normal)
5. If `smooth_groups` is empty, fall back to flat shading

- **Deliverable**: fixture bodies render without faceted edges

### Phase 4 — Tests, docs, CI

- Unit tests in `src/lib.rs`:
  - Minimal synthetic .3ds bytes that exercise each chunk
  - `Truncated` and `NotA3ds` error paths
  - `to_flat_buffers` vertex count = `faces.len() × 3`
  - `to_smooth_buffers` on a smooth-grouped cube produces shared vertices
- Integration test in `tests/`:
  - Load `tests/fixtures/simple_cube.3ds` (checked in, public-domain)
  - Assert expected vertex / face counts
  - Assert flat normals are unit length
  - Assert smooth normals are unit length
- Docs: module-level doc comment explaining chunk walk, coordinate system note
  (3DS is right-handed Y-up — same as Bevy — no axis permutation needed)
- CI: GitHub Actions `cargo test`, `cargo clippy`, `cargo doc --no-deps`
- **Deliverable**: `cargo publish` for real

---

## Coordinate system note

3DS stores vertices in right-handed Y-up (same as Bevy).  No axis permutation
is needed.  The `MESH_MATRIX` local transform is also Y-up.

Some exporters write Z-up data regardless of the spec; callers that know their
source is Z-up should swap Y/Z themselves after calling `parse`.

---

## Relationship to stageLX

Once Phase 1 lands on crates.io:

```toml
# stageLX/Cargo.toml
[workspace.dependencies]
stagelx-3ds = { path = "crates/stagelx-3ds" }  # ← replace with:
ds3 = "0.1"
```

`fixture.rs` call sites change from `stagelx_3ds::to_bevy_buffers` to
`ds3::Mesh3ds::to_flat_buffers` — identical signature, just renamed.

The internal `crates/stagelx-3ds` crate is then deleted.

---

## File layout (target)

```
3ds-rs/
├── Cargo.toml
├── LICENSE-MIT
├── LICENSE-APACHE
├── README.md
├── src/
│   ├── lib.rs          parse + Scene3ds + Mesh3ds + error
│   ├── chunk.rs        walk_chunks iterator
│   ├── geometry.rs     point/face/uv/smooth/transform parsers
│   └── convert.rs      to_flat_buffers + to_smooth_buffers
└── tests/
    ├── parse.rs        integration tests
    └── fixtures/
        └── simple_cube.3ds
```

---

## Open questions

- **Crate name**: `ds3`, `threeds`, `tds`, `rs3ds`?  `ds3` is short and available.
- **MSRV**: 1.70 (same as Bevy 0.18 ecosystem) or lower?
- **`no_std`**: feasible (no heap in the parser itself, only in the output Vecs) —
  worth gating behind a feature for embedded use.
