//! CONVERGE-3 N-7: scatter regions and the forest.
//!
//! A placement grammar, not an ontology: an area, a spacing, a density field,
//! a slope limit, exclusions and a seed go in; placements conformed to the
//! terrain come out. Everything here is a pure function of its inputs: the
//! same region and seed give the same placements, bit for bit. Randomness is
//! SplitMix64 (no dependency); sampling is Bridson's Poisson-disc.

use crate::still_water::TerrainSurface;

/// SplitMix64 (Steele, Lea, Flood 2014): small, fast and fully deterministic.
#[derive(Clone, Debug)]
pub struct SplitMix64(u64);

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }
}

/// Bridson's Poisson-disc sampling in the rectangle `lo`..`hi`: points no
/// closer than `radius`, seeded. Deterministic: the active list is consumed in
/// the order the generator dictates.
pub fn poisson_disc(lo: [f32; 2], hi: [f32; 2], radius: f32, seed: u64) -> Vec<[f32; 2]> {
    const ATTEMPTS: usize = 30;
    let cell = radius / std::f32::consts::SQRT_2;
    let columns = (((hi[0] - lo[0]) / cell).ceil() as usize).max(1);
    let rows = (((hi[1] - lo[1]) / cell).ceil() as usize).max(1);
    let mut grid: Vec<Option<usize>> = vec![None; columns * rows];
    let mut points: Vec<[f32; 2]> = Vec::new();
    let mut active: Vec<usize> = Vec::new();
    let mut rng = SplitMix64::new(seed);
    let index = |p: [f32; 2]| {
        let c = (((p[0] - lo[0]) / cell) as usize).min(columns - 1);
        let r = (((p[1] - lo[1]) / cell) as usize).min(rows - 1);
        (c, r)
    };
    let first = [rng.range(lo[0], hi[0]), rng.range(lo[1], hi[1])];
    let (c, r) = index(first);
    grid[r * columns + c] = Some(0);
    points.push(first);
    active.push(0);
    while !active.is_empty() {
        let pick = (rng.next_u64() % active.len() as u64) as usize;
        let origin = points[active[pick]];
        let mut found = false;
        for _ in 0..ATTEMPTS {
            let angle = rng.range(0.0, std::f32::consts::TAU);
            let distance = rng.range(radius, 2.0 * radius);
            let candidate = [origin[0] + distance * angle.cos(), origin[1] + distance * angle.sin()];
            if candidate[0] < lo[0] || candidate[0] >= hi[0] || candidate[1] < lo[1] || candidate[1] >= hi[1] {
                continue;
            }
            let (cc, cr) = index(candidate);
            let clear = (cr.saturating_sub(2)..(cr + 3).min(rows)).all(|row| {
                (cc.saturating_sub(2)..(cc + 3).min(columns)).all(|column| match grid[row * columns + column] {
                    Some(k) => {
                        let q = points[k];
                        (q[0] - candidate[0]).powi(2) + (q[1] - candidate[1]).powi(2) >= radius * radius
                    }
                    None => true,
                })
            });
            if clear {
                grid[cr * columns + cc] = Some(points.len());
                active.push(points.len());
                points.push(candidate);
                found = true;
                break;
            }
        }
        if !found {
            active.swap_remove(pick);
        }
    }
    points
}

fn hash2(ix: i32, iz: i32, seed: u32) -> f32 {
    let mut h =
        (ix as u32).wrapping_mul(0x8DA6_B343) ^ (iz as u32).wrapping_mul(0xD816_3841) ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Smooth value noise in [0, 1) with the given wavelength (metres).
pub fn value_noise(x: f32, z: f32, wavelength_m: f32, seed: u32) -> f32 {
    let (fx, fz) = (x / wavelength_m, z / wavelength_m);
    let (ix, iz) = (fx.floor() as i32, fz.floor() as i32);
    let (tx, tz) = (fx - fx.floor(), fz - fz.floor());
    let (sx, sz) = (tx * tx * (3.0 - 2.0 * tx), tz * tz * (3.0 - 2.0 * tz));
    let a = hash2(ix, iz, seed) + (hash2(ix + 1, iz, seed) - hash2(ix, iz, seed)) * sx;
    let b = hash2(ix, iz + 1, seed) + (hash2(ix + 1, iz + 1, seed) - hash2(ix, iz + 1, seed)) * sx;
    a + (b - a) * sz
}

pub fn smoothstep(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Where nothing of a region may stand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Exclusion {
    Disc {
        center: [f32; 2],
        radius: f32,
    },
    Ellipse {
        center: [f32; 2],
        radii: [f32; 2],
    },
    /// A sightline: everything within `half_width` (growing linearly from
    /// `from` to `to`) of the segment from `from` (a camera) to `to` (the
    /// landmark).
    Corridor {
        from: [f32; 2],
        to: [f32; 2],
        half_width_from: f32,
        half_width_to: f32,
    },
}

impl Exclusion {
    pub fn contains(&self, p: [f32; 2]) -> bool {
        match *self {
            Self::Disc { center, radius } => (p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2) < radius * radius,
            Self::Ellipse { center, radii } => {
                ((p[0] - center[0]) / radii[0]).powi(2) + ((p[1] - center[1]) / radii[1]).powi(2) < 1.0
            }
            Self::Corridor {
                from,
                to,
                half_width_from,
                half_width_to,
            } => {
                let d = [to[0] - from[0], to[1] - from[1]];
                let length2 = d[0] * d[0] + d[1] * d[1];
                let t = (((p[0] - from[0]) * d[0] + (p[1] - from[1]) * d[1]) / length2.max(1e-6)).clamp(0.0, 1.0);
                let closest = [from[0] + t * d[0], from[1] + t * d[1]];
                let width = half_width_from + (half_width_to - half_width_from) * t;
                (p[0] - closest[0]).powi(2) + (p[1] - closest[1]).powi(2) < width * width
            }
        }
    }
}

/// How a region's instances look: scale range, how far they tilt off
/// vertical, how deep they sink (per unit scale), and how much their colour
/// varies (value and hue).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub scale: [f32; 2],
    pub tilt_max_deg: f32,
    pub sink_m_per_scale: f32,
    pub value_jitter: f32,
    pub hue_jitter_deg: f32,
}

/// A scatter region: candidates at `spacing_m` (Poisson-disc) inside the
/// rectangle, kept with probability `density(x, z)`, on ground no steeper than
/// `max_slope` (rise over run), outside every exclusion.
pub struct ScatterRegion<'a> {
    pub lo: [f32; 2],
    pub hi: [f32; 2],
    pub spacing_m: f32,
    pub max_slope: f32,
    pub density: &'a dyn Fn(f32, f32) -> f32,
    pub exclusions: &'a [Exclusion],
    pub seed: u64,
}

/// One placed instance: where its base stands, how it is turned and sized,
/// and its colour multiplier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub translation: [f32; 3],
    pub rotation_xyzw: [f32; 4],
    pub scale: f32,
    pub tint_rgb: [f32; 3],
}

/// Ground slope (rise over run) at (x, z), from the rasterised surface.
pub fn slope(surface: &TerrainSurface, x: f32, z: f32) -> f32 {
    let h = 0.5;
    let gx = (surface.height(x + h, z) - surface.height(x - h, z)) / (2.0 * h);
    let gz = (surface.height(x, z + h) - surface.height(x, z - h)) / (2.0 * h);
    (gx * gx + gz * gz).sqrt()
}

fn quaternion_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// A colour multiplier: value `1 + value`, and a hue shift of `hue` radians
/// toward `direction` around the grey axis. The three channel offsets are
/// `sin(hue) cos(direction + 0, 120, 240 degrees)`, which sum to zero, so the
/// shift changes hue without changing brightness.
pub fn tint(value: f32, hue: f32, direction: f32) -> [f32; 3] {
    let third = std::f32::consts::TAU / 3.0;
    [0.0f32, third, 2.0 * third].map(|phase| (1.0 + value) * (1.0 + hue.sin() * (direction + phase).cos()))
}

/// Placements for one region in one style. Candidates are visited in their
/// sampling order, so the result is a pure function of the inputs.
pub fn scatter(region: &ScatterRegion<'_>, surface: &TerrainSurface, style: &Style) -> Vec<Placement> {
    let candidates = poisson_disc(region.lo, region.hi, region.spacing_m, region.seed);
    let mut rng = SplitMix64::new(region.seed ^ 0xA5A5_5A5A_DEAD_BEEF);
    let mut placements = Vec::new();
    for [x, z] in candidates {
        // Draw every random value whether or not the candidate survives, so
        // one candidate's fate never shifts another's.
        let (keep, yaw, scale, tilt_axis, tilt, value, hue, hue_direction) = (
            rng.unit(),
            rng.range(0.0, std::f32::consts::TAU),
            rng.range(style.scale[0], style.scale[1]),
            rng.range(0.0, std::f32::consts::TAU),
            rng.range(0.0, style.tilt_max_deg).to_radians(),
            rng.range(-style.value_jitter, style.value_jitter),
            rng.range(0.0, style.hue_jitter_deg).to_radians(),
            rng.range(0.0, std::f32::consts::TAU),
        );
        if keep >= (region.density)(x, z)
            || region.exclusions.iter().any(|e| e.contains([x, z]))
            || slope(surface, x, z) > region.max_slope
        {
            continue;
        }
        let yaw_q = [0.0, (yaw * 0.5).sin(), 0.0, (yaw * 0.5).cos()];
        let tilt_q = [
            tilt_axis.cos() * (tilt * 0.5).sin(),
            0.0,
            tilt_axis.sin() * (tilt * 0.5).sin(),
            (tilt * 0.5).cos(),
        ];
        let tint = tint(value, hue, hue_direction);
        placements.push(Placement {
            translation: [x, surface.height(x, z) - style.sink_m_per_scale * scale, z],
            rotation_xyzw: quaternion_mul(tilt_q, yaw_q),
            scale,
            tint_rgb: tint,
        });
    }
    placements
}

/// The campaign2 world's N-7 content: a forest on the meadow's edge and the
/// hills, ground cover and rocks around the ruin. `anchor` is the ruin's
/// anchor, `cameras` the authored camera positions (their sightlines to the
/// ruin stay clear of trees), `pool` the still water's centre and radii,
/// `ruin_parts` the footprint discs of the ruin's standing pieces.
pub fn campaign2_layout(
    surface: &TerrainSurface,
    world_half_extent: [f32; 2],
    anchor: [f32; 3],
    cameras: &[[f32; 3]],
    pool: ([f32; 2], [f32; 2]),
    ruin_parts: &[([f32; 2], f32)],
) -> crate::kit::ScatteredLayout {
    let hero = [anchor[0], anchor[2]];
    let distance = |x: f32, z: f32| ((x - hero[0]).powi(2) + (z - hero[1]).powi(2)).sqrt();
    let clamp_lo = |p: [f32; 2]| {
        [
            p[0].max(-world_half_extent[0] + 8.0),
            p[1].max(-world_half_extent[1] + 8.0),
        ]
    };
    let clamp_hi = |p: [f32; 2]| {
        [
            p[0].min(world_half_extent[0] - 8.0),
            p[1].min(world_half_extent[1] - 8.0),
        ]
    };
    let pool_scaled = |k: f32| Exclusion::Ellipse {
        center: pool.0,
        radii: [pool.1[0] * k, pool.1[1] * k],
    };
    let ruin_discs = |grow: f32| {
        ruin_parts.iter().map(move |(c, r)| Exclusion::Disc {
            center: *c,
            radius: r + grow,
        })
    };

    // Forest: everything the cameras look toward (they all look from the
    // +x/+z quadrant across the ruin), a clearing around the ruin that ramps
    // from nothing at 14 m to full density at 30 m, clumps and glades from
    // noise at a 55 m wavelength, and a sightline corridor from every camera
    // to the ruin (audit section 7.4: density with hierarchy).
    let mut tree_exclusions: Vec<Exclusion> = cameras
        .iter()
        .flat_map(|c| {
            [
                Exclusion::Corridor {
                    from: [c[0], c[2]],
                    to: hero,
                    half_width_from: 3.0,
                    half_width_to: 8.0,
                },
                Exclusion::Disc {
                    center: [c[0], c[2]],
                    radius: 9.0,
                },
            ]
        })
        .collect();
    tree_exclusions.push(pool_scaled(1.4));
    // Stands, not an orchard: near-full density inside the clumps, a few
    // stragglers in the glades (first pass at 5.5 m and a soft mask read as
    // open woodland of identical trees).
    let forest_density = |x: f32, z: f32| {
        let clumps = smoothstep(0.34, 0.48, value_noise(x, z, 55.0, 3));
        smoothstep(14.0, 30.0, distance(x, z)) * (0.04 + 0.96 * clumps)
    };
    let trees = scatter(
        &ScatterRegion {
            lo: clamp_lo([hero[0] - 400.0, hero[1] - 400.0]),
            hi: clamp_hi([hero[0] + 35.0, hero[1] + 40.0]),
            spacing_m: 3.5,
            max_slope: 0.55,
            density: &forest_density,
            exclusions: &tree_exclusions,
            seed: 0x7EE5_0001,
        },
        surface,
        &Style {
            scale: [1.0, 1.6],
            tilt_max_deg: 3.0,
            sink_m_per_scale: 0.0,
            value_jitter: 0.08,
            hue_jitter_deg: 4.0,
        },
    );

    // Ground cover: denser near the ruin's foot, thinning out by 22 m; never
    // in the water or inside the standing pieces.
    let mut cover_exclusions: Vec<Exclusion> = cameras
        .iter()
        .map(|c| Exclusion::Disc {
            center: [c[0], c[2]],
            radius: 2.5,
        })
        .collect();
    cover_exclusions.push(pool_scaled(1.15));
    cover_exclusions.extend(ruin_discs(0.0));
    // Patches, not a polka-dot field: ferns gather where a 7 m noise is high,
    // more of them near the ruin's foot.
    let fern_density = |x: f32, z: f32| {
        let patch = smoothstep(0.42, 0.62, value_noise(x, z, 9.0, 5));
        (0.03 + 0.9 * patch) * (1.0 - 0.6 * smoothstep(8.0, 24.0, distance(x, z)))
    };
    let ferns = scatter(
        &ScatterRegion {
            lo: [hero[0] - 24.0, hero[1] - 24.0],
            hi: [hero[0] + 24.0, hero[1] + 24.0],
            spacing_m: 1.1,
            max_slope: 0.6,
            density: &fern_density,
            exclusions: &cover_exclusions,
            seed: 0xFE24_0002,
        },
        surface,
        &Style {
            scale: [0.75, 1.25],
            tilt_max_deg: 6.0,
            sink_m_per_scale: 0.0,
            value_jitter: 0.08,
            hue_jitter_deg: 5.0,
        },
    );

    // Rocks: sparse, a little denser near the ruin, sunk 18 cm x scale like
    // the authored ones, tilted up to 14 degrees.
    let mut rock_exclusions: Vec<Exclusion> = cameras
        .iter()
        .map(|c| Exclusion::Disc {
            center: [c[0], c[2]],
            radius: 4.0,
        })
        .collect();
    rock_exclusions.push(pool_scaled(1.1));
    rock_exclusions.extend(ruin_discs(0.5));
    let rock_density = |x: f32, z: f32| 0.22 * (1.0 - 0.5 * smoothstep(10.0, 40.0, distance(x, z)));
    let rocks = scatter(
        &ScatterRegion {
            lo: [hero[0] - 40.0, hero[1] - 40.0],
            hi: [hero[0] + 40.0, hero[1] + 40.0],
            spacing_m: 6.0,
            max_slope: 0.6,
            density: &rock_density,
            exclusions: &rock_exclusions,
            seed: 0x50C4_0003,
        },
        surface,
        &Style {
            scale: [0.45, 1.05],
            tilt_max_deg: 14.0,
            sink_m_per_scale: 0.18,
            value_jitter: 0.10,
            hue_jitter_deg: 3.0,
        },
    );
    let rocks = rocks
        .into_iter()
        .enumerate()
        .map(|(i, p)| (if i % 2 == 0 { "rock_a" } else { "rock_b" }, p))
        .collect();
    crate::kit::ScatteredLayout { trees, rocks, ferns }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poisson_disc_is_seeded_and_keeps_its_spacing() {
        let a = poisson_disc([0.0, 0.0], [40.0, 30.0], 2.0, 7);
        let b = poisson_disc([0.0, 0.0], [40.0, 30.0], 2.0, 7);
        let c = poisson_disc([0.0, 0.0], [40.0, 30.0], 2.0, 8);
        assert_eq!(a, b, "same seed, same points");
        assert_ne!(a, c, "another seed, other points");
        assert!(a.len() > 100, "{} points", a.len());
        for (i, p) in a.iter().enumerate() {
            for q in &a[i + 1..] {
                assert!((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) >= 4.0 - 1e-4);
            }
        }
    }

    #[test]
    fn a_corridor_widens_from_camera_to_landmark() {
        let corridor = Exclusion::Corridor {
            from: [0.0, 0.0],
            to: [10.0, 0.0],
            half_width_from: 1.0,
            half_width_to: 3.0,
        };
        assert!(corridor.contains([0.5, 0.9]));
        assert!(!corridor.contains([0.5, 1.2]), "narrow at the camera");
        assert!(corridor.contains([9.5, 2.8]), "wide at the landmark");
        assert!(!corridor.contains([5.0, 2.5]), "half way: 2 m wide");
    }

    #[test]
    fn a_tint_shifts_hue_without_changing_brightness() {
        let t = tint(0.0, 3.0f32.to_radians(), 1.0);
        let mean = (t[0] + t[1] + t[2]) / 3.0;
        assert!((mean - 1.0).abs() < 1e-6, "mean {mean}");
        assert!(t.iter().any(|c| (c - 1.0).abs() > 0.01), "no shift: {t:?}");
        assert!(t.iter().all(|c| (0.94..=1.06).contains(c)), "{t:?}");
        assert_eq!(tint(0.06, 0.0, 0.0), [1.06; 3]);
    }

    #[test]
    fn scatter_honours_density_slope_and_exclusions_and_is_deterministic() {
        let surface = TerrainSurface::flat(200.0, 200.0, 0.0);
        let exclusions = [Exclusion::Disc {
            center: [0.0, 0.0],
            radius: 10.0,
        }];
        let half = |x: f32, _z: f32| if x > 0.0 { 1.0 } else { 0.0 };
        let region = ScatterRegion {
            lo: [-50.0, -50.0],
            hi: [50.0, 50.0],
            spacing_m: 3.0,
            max_slope: 0.5,
            density: &half,
            exclusions: &exclusions,
            seed: 11,
        };
        let style = Style {
            scale: [0.8, 1.2],
            tilt_max_deg: 0.0,
            sink_m_per_scale: 0.1,
            value_jitter: 0.06,
            hue_jitter_deg: 3.0,
        };
        let a = scatter(&region, &surface, &style);
        assert_eq!(a, scatter(&region, &surface, &style));
        assert!(a.len() > 50, "{}", a.len());
        for p in &a {
            let [x, y, z] = p.translation;
            assert!(x > 0.0, "density 0 where x <= 0");
            assert!(x * x + z * z >= 100.0, "inside the exclusion");
            assert!((0.8..=1.2).contains(&p.scale));
            assert!((y + 0.1 * p.scale).abs() < 1e-5, "sunk by 0.1 x scale");
        }
    }
}
