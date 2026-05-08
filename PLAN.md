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

## Public API (current)

```rust
// ── Entry point ────────────────────────────────────────────────────────────
pub fn parse(data: &[u8]) -> Result<Scene3ds, Error3ds>;

// ── Types ──────────────────────────────────────────────────────────────────
pub type Mat4x3 = [[f32; 3]; 4];

pub struct Scene3ds {
    pub meshes:    Vec<Mesh3ds>,
    pub materials: Vec<String>,
}

pub struct Mesh3ds {
    pub name:          String,
    pub vertices:      Vec<[f32; 3]>,
    pub faces:         Vec<[u16; 3]>,
    pub uvs:           Vec<[f32; 2]>,
    pub smooth_groups: Vec<u32>,
    pub transform:     Mat4x3,
    pub material:      Option<String>,
}

pub struct MeshBuffers {
    pub positions: Vec<[f32; 3]>,
    pub normals:   Vec<[f32; 3]>,
    pub uvs:       Vec<[f32; 2]>,
    pub indices:   Vec<u32>,
}

impl Mesh3ds {
    pub fn to_flat_buffers(&self)   -> MeshBuffers;
    pub fn to_smooth_buffers(&self) -> MeshBuffers;
}

// ── Errors ─────────────────────────────────────────────────────────────────
#[derive(Debug, thiserror::Error)]
pub enum Error3ds {
    #[error("data too short (need >= 6 bytes, got {0})")]
    Truncated(usize),
    #[error("not a 3DS file — expected MAIN3DS (0x4D4D), got 0x{0:04X}")]
    NotA3ds(u16),
    #[error("chunk 0x{id:04X} at offset {offset} length {length} exceeds parent bounds")]
    ChunkOverflow { id: u16, offset: usize, length: u32 },
    #[error("offset {start} is past the end of chunk 0x{id:04X} (end {end})")]
    BadOffset { id: u16, start: usize, end: usize },
}
```

`to_flat_buffers` is a drop-in replacement for `stagelx_3ds::to_bevy_buffers`.  
`to_smooth_buffers` is the upgrade path for fixture bodies that look faceted today.

No Bevy dependency anywhere. The return types are plain `[f32; N]` arrays —
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

---

## Implementation phases

### ✅ Phase 1 — Bootstrap

- `cargo new --lib 3ds-rs`
- Parser logic ported from `stagelx-3ds/src/lib.rs`
- `Truncated` and `NotA3ds` error variants
- `parse`, `Scene3ds`, `Mesh3ds`, `to_flat_buffers` exposed
- `Cargo.toml` metadata: `description`, `keywords`, `categories`, `license`, `repository`
- MIT + Apache-2.0 dual licence files

### ✅ Phase 2 — Extended chunk support

- `SMOOTH_GROUP` (0x4150): `Vec<u32>` in `Mesh3ds::smooth_groups`
- `MESH_MATRIX` (0x4160): `Mat4x3` in `Mesh3ds::transform`
- `MAT_ENTRY` / `MAT_NAME` (0xAFFF / 0xA000): `Vec<String>` in `Scene3ds::materials`
- `MSH_MAT_GROUP` (0x4130): per-mesh material name in `Mesh3ds::material`

### ✅ Phase 3 — Smooth normal generation

`to_smooth_buffers` algorithm:
1. Area-weighted face normals via `cross × 0.5`
2. Union-Find over incident faces per vertex, grouped by shared smooth-group bits
3. Accumulated normals normalized per component
4. Output deduplicated on `(vertex_index, normal_bits)`
5. Falls back to flat shading when `smooth_groups` is empty

### ✅ Phase 3.5 — Pre-publish code review

Addressed all reviewer findings before crates.io publish:

- **MeshBuffers**: promoted from opaque 4-tuple to named struct with `positions`,
  `normals`, `uvs`, `indices` fields — prevents positional swap bugs at call sites
- **Mat4x3**: exposed as a named public type alias for `[[f32;3];4]`
- **BadOffset**: new `Error3ds` variant for the walk-from-past-end case in
  `walk_chunks_from`, replacing a misleading `ChunkOverflow` there
- **Chunk min-length guard**: `Chunk::read_at` now rejects `length < 6`
- **ChunkIter DRY**: `ChunkIter::next` delegates to `Chunk::read_at`,
  removing ~20 lines of duplicated header-parsing
- **Flags allocation**: `read_face_array` no longer allocates a `Vec<u16>` for
  face flags that were immediately discarded
- **to_smooth pre-sizing**: output buffers use `Vec::with_capacity(vertices.len())`
- **Docs**: coord-system note de-Bevy-ified; UV-seam docstring corrected;
  `#[allow(dead_code)]` narrowed to the two actually-unused test helpers

### ✅ Phase 4 — Integration tests, fixtures, CI

- `tests/parse.rs`: four integration tests against the real fixture
  - `parse_simple_cube` — mesh/vertex/face/UV/smooth-group/material counts
  - `flat_buffers_unit_normals` — buffer lengths + unit-length assertion
  - `smooth_buffers_unit_normals` — vertex sharing + unit-length assertion
  - `smooth_shares_vertices_across_faces` — smooth < flat vertex count
- `tests/fixtures/simple_cube.3ds` — 383-byte checked-in binary fixture
- GitHub Actions (3 jobs, matrix stable + 1.70):
  - **test**: `cargo test --locked --all-targets` × 2 toolchains,
    `cargo test --locked --all-targets --features serde` × 2 toolchains
  - **lint**: `cargo clippy --locked -- -D warnings` (default + serde),
    `cargo doc --locked --no-deps`
  - **publish-dry-run**: `cargo publish --locked --dry-run`
  - `Swatinem/rust-cache@v2` on all jobs
- Fixed latent bug: optional `serde` dep had `default-features = false`,
  stripping std impls for `String`, `Vec<T>`, and arrays — crate silently
  failed to compile with `--features serde` since the initial scaffold

### 🔲 Phase 5 — Publish

- [ ] `cargo publish` for real

---

## Coordinate system note

3DS stores vertices in right-handed Y-up (the same convention used by glTF and
Bevy). No axis permutation is needed. The `MESH_MATRIX` local transform is also
Y-up.

Some exporters write Z-up data regardless of the spec; callers that know their
source is Z-up should swap Y/Z themselves after calling `parse`.

---

## Relationship to stageLX

Once published to crates.io:

```toml
# stageLX/Cargo.toml
[workspace.dependencies]
stagelx-3ds = { path = "crates/stagelx-3ds" }  # ← replace with:
ds3 = "0.1"
```

`fixture.rs` call sites change from `stagelx_3ds::to_bevy_buffers` to
`ds3::Mesh3ds::to_flat_buffers` — identical signature, just renamed.
`MeshBuffers` fields replace tuple destructuring at each call site.

The internal `crates/stagelx-3ds` crate is then deleted.

---

## File layout

```
3ds-rs/
├── Cargo.toml
├── LICENSE-MIT
├── LICENSE-APACHE
├── README.md
├── PLAN.md
├── src/
│   ├── lib.rs          parse + Scene3ds + Mesh3ds + Error3ds + unit tests
│   ├── chunk.rs        Chunk + ChunkIter + walk_chunks / walk_chunks_from
│   ├── geometry.rs     point/face/uv/smooth/transform parsers
│   └── convert.rs      MeshBuffers + to_flat_buffers + to_smooth_buffers
└── tests/
    ├── parse.rs        integration tests  ← TODO
    └── fixtures/
        └── simple_cube.3ds               ← TODO
```

---

## Open questions

- **MSRV**: 1.70 (same as Bevy 0.18 ecosystem) or lower?
- **`no_std`**: feasible (no heap in the parser itself, only in output Vecs) —
  worth gating behind a feature for embedded use.
