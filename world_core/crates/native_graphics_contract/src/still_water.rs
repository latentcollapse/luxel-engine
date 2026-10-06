//! CONVERGE-3 W-1: the campaign2 pool as still water.
//!
//! The converge2 pool was a flat disc laid on sloping ground with two emissive
//! rings on top. Still water is painted by the terrain shader instead
//! (`TerrainWetZone::standing_water`): inside a noise-displaced ellipse the
//! ground becomes a flat, dark, smooth surface that reflects the sky, and a
//! band of wet ground fades out around it.
//!
//! A first version carved a bowl into the terrain and filled it with a flat
//! water mesh. On the 2 m terrain grid (the pool's minor radius is 2.7 m) the
//! shoreline came out as 2 m straight segments and the ground near the shore
//! z-fought with the water. A per-pixel outline has neither problem, needs no
//! carving and is grounded by construction.

use crate::{
    BufferPayload, GraphicsContractError, GraphicsScenePacketBody, StandingWater, TerrainPacket, TerrainWetZone,
};

/// The water surface's height: the mean rasterised ground height over the
/// inner 80% of the pool (16 x 16 samples).
fn surface_height(surface: &TerrainSurface, center: [f32; 2], radii: [f32; 2]) -> f32 {
    let (mut sum, mut count) = (0.0f32, 0u32);
    for i in 0..16 {
        for j in 0..16 {
            let (u, v) = ((i as f32 + 0.5) / 8.0 - 1.0, (j as f32 + 0.5) / 8.0 - 1.0);
            if u * u + v * v <= 0.64 {
                sum += surface.height(center[0] + u * radii[0], center[1] + v * radii[1]);
                count += 1;
            }
        }
    }
    sum / count as f32
}

/// The converge2 pool's ellipse (its instance scale), kept so the pool stays
/// where the composition put it.
pub const POOL_RADII_XZ_M: [f32; 2] = [3.8, 2.7];
/// Wet ground around the water: the CALIBRATION-1 wet recipe (albedo x 0.6
/// linear, normals x 0.4, roughness x 0.25), fading out over 0.6 m.
const WET_FALLOFF_M: f32 = 0.6;
/// Still, slightly murky water: a dark body so the reflection carries it.
/// +-0.2 ellipse units of shore wobble at ~2.2 m wavelength: an irregular
/// pool, not a disc (first review: "looks like a large round tree shadow").
const WATER_ALBEDO_RGB: [f32; 3] = [0.018, 0.022, 0.016];
const WATER_ROUGHNESS: f32 = 0.02;
const EDGE_WOBBLE: f32 = 0.2;
const WOBBLE_CYCLES_PER_M: f32 = 0.45;

/// The converge2 pool resources this replaces.
const REPLACED_INSTANCES: [&str; 3] = [
    "campaign2-wet-pool",
    "campaign2-wet-ripple-outer",
    "campaign2-wet-ripple-inner",
];
const REPLACED_MESHES: [&str; 2] = ["campaign2-wet-pool", "campaign2-wet-ripple"];
const REPLACED_MATERIAL: &str = "campaign2-wet";
const REPLACED_TEXTURE: &str = "luxel-campaign2-wet-albedo";

/// The terrain surface exactly as the renderer rasterises it: each grid cell
/// is split along the (col, row) -> (col + 1, row + 1) diagonal
/// (`LavaAdapter._emit_terrain_vertex`), and heights are linear on each
/// triangle. Bilinear interpolation differs from that by up to a quarter of
/// the cell's twist, and nearest-cell lookup by up to half a cell's rise.
#[derive(Clone, Debug)]
pub struct TerrainSurface {
    heights: Vec<f32>,
    resolution: usize,
    width_m: f32,
    length_m: f32,
}

impl TerrainSurface {
    pub fn of(terrain: &TerrainPacket) -> Result<Self, GraphicsContractError> {
        let heights = match &terrain.heights_m.payload {
            BufferPayload::F32(values) => values.clone(),
            _ => {
                return Err(GraphicsContractError::unsupported(
                    "terrain surface sampling requires f32 heights",
                ));
            }
        };
        if terrain.resolution < 2 || heights.len() != terrain.resolution * terrain.resolution {
            return Err(GraphicsContractError::malformed(
                "terrain heights do not match the grid resolution",
            ));
        }
        Ok(Self {
            heights,
            resolution: terrain.resolution,
            width_m: terrain.width_m,
            length_m: terrain.length_m,
        })
    }

    /// A flat surface at `height_m` (for tests of placement code).
    #[cfg(test)]
    pub(crate) fn flat(width_m: f32, length_m: f32, height_m: f32) -> Self {
        Self {
            heights: vec![height_m; 4],
            resolution: 2,
            width_m,
            length_m,
        }
    }

    fn at(&self, column: usize, row: usize) -> f32 {
        self.heights[row * self.resolution + column]
    }

    /// Height of the rasterised surface at world (x, z); clamped to the grid.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let cells = (self.resolution - 1) as f32;
        let (column, row) = ((x / self.width_m + 0.5) * cells, (0.5 - z / self.length_m) * cells);
        let last = cells - 1.0;
        let c0 = column.floor().clamp(0.0, last);
        let r0 = row.floor().clamp(0.0, last);
        let fc = (column - c0).clamp(0.0, 1.0);
        let fr = (row - r0).clamp(0.0, 1.0);
        let (c, r) = (c0 as usize, r0 as usize);
        let h00 = self.at(c, r);
        let h11 = self.at(c + 1, r + 1);
        if fc >= fr {
            let h10 = self.at(c + 1, r);
            h00 + fc * (h10 - h00) + fr * (h11 - h10)
        } else {
            let h01 = self.at(c, r + 1);
            h00 + fr * (h01 - h00) + fc * (h11 - h01)
        }
    }
}

/// Replace the converge2 pool (disc, rings, ripple albedo) with standing water
/// painted by the terrain at the same place and size.
pub fn apply_still_water(body: &mut GraphicsScenePacketBody, center: [f32; 2]) -> Result<(), GraphicsContractError> {
    let surface_y_m = surface_height(&TerrainSurface::of(&body.terrain)?, center, POOL_RADII_XZ_M);
    body.instances
        .retain(|instance| !REPLACED_INSTANCES.contains(&instance.instance_id.as_str()));
    body.meshes
        .retain(|mesh| !REPLACED_MESHES.contains(&mesh.mesh_id.as_str()));
    body.materials
        .retain(|material| material.material_id != REPLACED_MATERIAL);
    body.textures.retain(|texture| texture.texture_id != REPLACED_TEXTURE);
    body.terrain.wet_zone = Some(TerrainWetZone {
        center_xz_m: center,
        radii_xz_m: POOL_RADII_XZ_M,
        falloff_m: WET_FALLOFF_M,
        albedo_scale: 0.6,
        roughness_scale: 0.25,
        normal_scale: 0.4,
        standing_water: Some(StandingWater {
            albedo_rgb: WATER_ALBEDO_RGB,
            roughness: WATER_ROUGHNESS,
            edge_wobble: EDGE_WOBBLE,
            wobble_cycles_per_m: WOBBLE_CYCLES_PER_M,
            surface_y_m,
        }),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_matches_the_rasterised_triangles() {
        // One cell with a twist: bilinear would give 0.25 at the centre, the
        // renderer's (0,0)-(1,1) diagonal gives 0.5 (mean of h00 and h11).
        let surface = TerrainSurface {
            heights: vec![0.0, 0.0, 0.0, 1.0],
            resolution: 2,
            width_m: 2.0,
            length_m: 2.0,
        };
        assert_eq!(surface.height(0.0, 0.0), 0.5);
        // Corners are exact.
        assert_eq!(surface.height(-1.0, 1.0), 0.0);
        assert_eq!(surface.height(1.0, -1.0), 1.0);
        // Either side of the diagonal is linear on its own triangle.
        assert!((surface.height(0.5, 0.5) - 0.25).abs() < 1e-6);
        assert!((surface.height(-0.5, -0.5) - 0.25).abs() < 1e-6);
    }
}
