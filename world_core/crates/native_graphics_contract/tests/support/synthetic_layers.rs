//! A small, deterministic, offline terrain layer set for contract tests.
//! Real scans are fetched by tools/fetch_terrain_layers.py; tests must not
//! depend on the network, so they exercise the same builder with generated art.

use wge_native_graphics_contract::{
    LayerCoverage, LayerSource, LayerTextureSizes, MacroRamp, SquareRgba8, TerrainLayerSet,
    build_terrain_layer_set,
};

pub fn checker(side: u32, a: [u8; 4], b: [u8; 4]) -> SquareRgba8 {
    let mut bytes = Vec::with_capacity((side * side * 4) as usize);
    for row in 0..side {
        for column in 0..side {
            bytes.extend(if (row / 4 + column / 4) % 2 == 0 { a } else { b });
        }
    }
    SquareRgba8 { side, bytes }
}

pub fn source(layer_id: &str, tint: [u8; 4], metres_mm: u32, coverage: Option<LayerCoverage>) -> LayerSource {
    LayerSource {
        layer_id: layer_id.into(),
        source: format!("synthetic-{layer_id}"),
        metres_per_repeat_milli: metres_mm,
        normal_scale: 1.0,
        coverage,
        albedo: checker(64, tint, [tint[0] / 2, tint[1] / 2, tint[2] / 2, 255]),
        normal: checker(64, [150, 128, 240, 255], [106, 128, 240, 255]),
        roughness: checker(64, [200, 200, 200, 255], [230, 230, 230, 255]),
        occlusion: checker(64, [255, 255, 255, 255], [180, 180, 180, 255]),
    }
}

pub fn sizes() -> LayerTextureSizes {
    LayerTextureSizes { albedo: 64, normal_gl: 32, roughness: 16, ao: 16 }
}

pub fn synthetic_set() -> TerrainLayerSet {
    build_terrain_layer_set(
        "synthetic-meadow",
        sizes(),
        &[
            source("ground", [90, 120, 60, 255], 2000, None),
            source(
                "dirt",
                [120, 95, 70, 255],
                3150,
                Some(LayerCoverage {
                    slope_bp: None,
                    height_mm: Some([17500, 14500]),
                    macro_ramp: Some(MacroRamp { threshold_bp: 5600, softness_bp: 900 }),
                }),
            ),
            source(
                "rock",
                [110, 110, 105, 255],
                3000,
                Some(LayerCoverage { slope_bp: Some([900, 1600]), height_mm: None, macro_ramp: None }),
            ),
        ],
        32,
        1374839221,
    )
    .expect("synthetic layer set builds")
}
