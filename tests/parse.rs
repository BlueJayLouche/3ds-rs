use ds3::{parse, MeshBuffers};
use std::fs;

#[test]
fn parse_simple_cube() {
    let bytes = fs::read("tests/fixtures/simple_cube.3ds").expect("fixture missing");
    let scene = parse(&bytes).expect("parse failed");

    assert_eq!(scene.meshes.len(), 1, "expected one mesh");
    let mesh = &scene.meshes[0];

    assert_eq!(mesh.name, "Cube");
    assert_eq!(mesh.vertices.len(), 8);
    assert_eq!(mesh.faces.len(), 12);
    assert_eq!(mesh.uvs.len(), 8);
    assert_eq!(mesh.smooth_groups.len(), 12);
    assert!(mesh.smooth_groups.iter().all(|&g| g == 1));
    assert_eq!(mesh.material, None); // MSH_MAT_GROUP not present in fixture

    assert_eq!(scene.materials, vec!["Default"]);
}

#[test]
fn flat_buffers_unit_normals() {
    let bytes = fs::read("tests/fixtures/simple_cube.3ds").expect("fixture missing");
    let scene = parse(&bytes).expect("parse failed");
    let mesh = &scene.meshes[0];

    let MeshBuffers {
        positions,
        normals,
        uvs,
        indices,
    } = mesh.to_flat_buffers();

    // Flat shading expands to one vertex per face corner.
    assert_eq!(positions.len(), mesh.faces.len() * 3);
    assert_eq!(normals.len(), mesh.faces.len() * 3);
    assert_eq!(uvs.len(), mesh.faces.len() * 3);
    assert_eq!(indices.len(), mesh.faces.len() * 3);

    for n in &normals {
        let len_sq = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
        assert!(
            (len_sq - 1.0).abs() < 1e-5,
            "flat normal not unit length: {:?} (len² = {})",
            n,
            len_sq
        );
    }
}

#[test]
fn smooth_buffers_unit_normals() {
    let bytes = fs::read("tests/fixtures/simple_cube.3ds").expect("fixture missing");
    let scene = parse(&bytes).expect("parse failed");
    let mesh = &scene.meshes[0];

    let MeshBuffers {
        positions,
        normals,
        indices,
        ..
    } = mesh.to_smooth_buffers();

    // With all faces in the same smooth group, vertices should be shared.
    assert!(
        positions.len() < mesh.faces.len() * 3,
        "smooth shading should share vertices, got {} positions",
        positions.len()
    );
    assert_eq!(indices.len(), mesh.faces.len() * 3);

    for n in &normals {
        let len_sq = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
        assert!(
            (len_sq - 1.0).abs() < 1e-5,
            "smooth normal not unit length: {:?} (len² = {})",
            n,
            len_sq
        );
    }
}

#[test]
fn smooth_shares_vertices_across_faces() {
    let bytes = fs::read("tests/fixtures/simple_cube.3ds").expect("fixture missing");
    let scene = parse(&bytes).expect("parse failed");
    let mesh = &scene.meshes[0];

    let flat = mesh.to_flat_buffers();
    let smooth = mesh.to_smooth_buffers();

    // Smooth shading should produce strictly fewer unique vertices than flat.
    assert!(
        smooth.positions.len() < flat.positions.len(),
        "smooth ({}) should have fewer vertices than flat ({})",
        smooth.positions.len(),
        flat.positions.len()
    );
}
