# ds3

Pure-Rust parser for Autodesk 3D Studio binary (`.3ds`) files.

No dependencies on graphics engines — output is plain `f32` arrays that you
feed into `Mesh::insert_attribute` (Bevy), `glBufferData` (OpenGL), or
whatever renderer you use.

## Quick start

```rust
use ds3::parse;

let scene = parse(&std::fs::read("model.3ds")?)?;
for mesh in &scene.meshes {
    println!("{}: {} verts, {} faces", mesh.name, mesh.vertices.len(), mesh.faces.len());

    // Flat-shaded (one vertex per face corner)
    let (pos, nrm, uv, idx) = mesh.to_flat_buffers();

    // Smooth-shaded (vertices shared across smooth groups)
    let (pos, nrm, uv, idx) = mesh.to_smooth_buffers();
}
```

## Features

| Feature | Description |
|---------|-------------|
| `serde` | Derives `Serialize` / `Deserialize` on all public types |

## Coordinate system

3DS stores vertices in right-handed Y-up. No axis permutation is required.
The `Mesh3ds::transform` matrix is also Y-up.

Some exporters write Z-up regardless of the spec; swap Y/Z after `parse` if you
know your source is Z-up.

## Supported chunks

| Chunk | Parsed | Notes |
|-------|--------|-------|
| `MAIN3DS` (0x4D4D) | ✓ | Root container |
| `EDIT3DS` (0x3D3D) | ✓ | Editor container |
| `NAMED_OBJECT` (0x4000) | ✓ | Object name + children |
| `N_TRI_OBJECT` (0x4100) | ✓ | Triangle mesh container |
| `POINT_ARRAY` (0x4110) | ✓ | Vertex positions |
| `FACE_ARRAY` (0x4120) | ✓ | Face indices + flags |
| `MSH_MAT_GROUP` (0x4130) | ✓ | Material name (first only) |
| `TEX_VERTS` (0x4140) | ✓ | Texture coordinates |
| `SMOOTH_GROUP` (0x4150) | ✓ | Per-face smooth-group bitmask |
| `MESH_MATRIX` (0x4160) | ✓ | Local transform |
| `MAT_ENTRY` / `MAT_NAME` | ✓ | Material names collected in `Scene3ds::materials` |

All other chunks are silently skipped.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
