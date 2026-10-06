//! A small, deterministic backdrop GLB for contract tests: two textured grid
//! tiles in the seat's frame, the shape `tools/build_backdrop.py` emits. Real
//! backdrops are built from a Gaea field; tests must not depend on Gaea.

use serde_json::json;

fn png(side: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut image = image::RgbImage::new(side, side);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let shade = if (x / 2 + y / 2) % 2 == 0 { 1.0 } else { 0.7 };
        *pixel = image::Rgb(rgb.map(|c| (c as f32 * shade) as u8));
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).expect("png encodes");
    bytes.into_inner()
}

/// `tiles` grid meshes of `side` x `side` vertices, each `span_m` wide, laid
/// side by side along +x from the seat, rising to `peak_m` in the middle.
pub fn backdrop_glb(tiles: usize, side: usize, span_m: f32, peak_m: f32) -> Vec<u8> {
    let mut binary: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    let mut materials = Vec::new();
    let mut images = Vec::new();
    let mut textures = Vec::new();
    let mut nodes = Vec::new();

    let mut push_view = |binary: &mut Vec<u8>, data: &[u8], target: Option<u32>| -> usize {
        while binary.len() % 4 != 0 {
            binary.push(0);
        }
        let mut view = json!({"buffer": 0, "byteOffset": binary.len(), "byteLength": data.len()});
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        binary.extend_from_slice(data);
        views.push(view);
        views.len() - 1
    };

    for tile in 0..tiles {
        let (mut positions, mut normals, mut uvs, mut indices) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let step = span_m / (side - 1) as f32;
        for row in 0..side {
            for column in 0..side {
                let x = tile as f32 * span_m + column as f32 * step;
                let z = row as f32 * step - span_m * 0.5;
                let t = (column as f32 / (side - 1) as f32 - 0.5).abs() * 2.0;
                positions.extend([x, peak_m * (1.0 - t), z]);
                normals.extend([0.0f32, 1.0, 0.0]);
                uvs.extend([column as f32 / (side - 1) as f32, row as f32 / (side - 1) as f32]);
            }
        }
        for row in 0..side - 1 {
            for column in 0..side - 1 {
                let a = (row * side + column) as u32;
                let (b, c, d) = (a + 1, a + side as u32, a + side as u32 + 1);
                indices.extend([a, c, b, b, c, d]);
            }
        }
        let bytes = |values: &[f32]| values.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
        let (min, max) = positions.chunks(3).fold(([f32::MAX; 3], [f32::MIN; 3]), |(mut lo, mut hi), p| {
            for axis in 0..3 {
                lo[axis] = lo[axis].min(p[axis]);
                hi[axis] = hi[axis].max(p[axis]);
            }
            (lo, hi)
        });
        let vertex_count = side * side;
        let p = push_view(&mut binary, &bytes(&positions), Some(34962));
        accessors.push(json!({"bufferView": p, "componentType": 5126, "count": vertex_count, "type": "VEC3", "min": min, "max": max}));
        let n = push_view(&mut binary, &bytes(&normals), Some(34962));
        accessors.push(json!({"bufferView": n, "componentType": 5126, "count": vertex_count, "type": "VEC3"}));
        let u = push_view(&mut binary, &bytes(&uvs), Some(34962));
        accessors.push(json!({"bufferView": u, "componentType": 5126, "count": vertex_count, "type": "VEC2"}));
        let index_bytes: Vec<u8> = indices.iter().flat_map(|v| v.to_le_bytes()).collect();
        let i = push_view(&mut binary, &index_bytes, Some(34963));
        accessors.push(json!({"bufferView": i, "componentType": 5125, "count": indices.len(), "type": "SCALAR"}));
        let base = accessors.len() - 4;

        let image_view = push_view(&mut binary, &png(8, [60 + 40 * tile as u8, 70, 50]), None);
        images.push(json!({"bufferView": image_view, "mimeType": "image/png"}));
        textures.push(json!({"sampler": 0, "source": images.len() - 1}));
        materials.push(json!({
            "name": format!("backdrop_t{tile}"),
            "pbrMetallicRoughness": {"baseColorTexture": {"index": textures.len() - 1}, "metallicFactor": 0.0, "roughnessFactor": 0.95}
        }));
        meshes.push(json!({
            "name": format!("backdrop_t{tile}"),
            "primitives": [{"attributes": {"POSITION": base, "NORMAL": base + 1, "TEXCOORD_0": base + 2}, "indices": base + 3, "material": materials.len() - 1}]
        }));
        nodes.push(json!({"name": format!("backdrop_t{tile}"), "mesh": meshes.len() - 1}));
    }
    while binary.len() % 4 != 0 {
        binary.push(0);
    }
    let node_indices: Vec<usize> = (0..nodes.len()).collect();
    let document = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": node_indices}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "images": images,
        "textures": textures,
        "samplers": [{"magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071}],
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": binary.len()}],
    });
    let mut text = serde_json::to_vec(&document).expect("json");
    while text.len() % 4 != 0 {
        text.push(b' ');
    }
    let total = 12 + 8 + text.len() + 8 + binary.len();
    let mut glb = Vec::with_capacity(total);
    glb.extend(0x4654_6C67u32.to_le_bytes());
    glb.extend(2u32.to_le_bytes());
    glb.extend((total as u32).to_le_bytes());
    glb.extend((text.len() as u32).to_le_bytes());
    glb.extend(0x4E4F_534Au32.to_le_bytes());
    glb.extend(text);
    glb.extend((binary.len() as u32).to_le_bytes());
    glb.extend(0x004E_4942u32.to_le_bytes());
    glb.extend(binary);
    glb
}
