using CodewealdTerrainLab
using JSON3
using Test

@testset "semantic terrain analysis" begin
    mktempdir() do directory
        manifest_path = joinpath(directory, "terrain_manifest.json")
        heightfield_path = joinpath(directory, "heightfield.bin")
        mask_path = joinpath(directory, "mask.bin")
        regions_path = joinpath(directory, "regions.bin")
        write(
            manifest_path,
            JSON3.write(
                Dict(
                    "resolution" => 3,
                    "zone_id" => "test_zone",
                    "zone_spec_sha256" => "a"^64,
                    "world_bounds_m" => Dict("width" => 2.0, "length" => 2.0),
                )
            ),
        )
        heights = Float32[
            0 0 0
            0 0 10
            0 0 10
        ]
        write(heightfield_path, reinterpret(UInt8, vec(permutedims(heights))))
        protected = UInt8[
            0 0 0
            0 0 255
            0 0 255
        ]
        write(mask_path, vec(permutedims(protected)))
        regions = UInt8[
            0 0 0
            0 0 1
            0 0 1
        ]
        write(regions_path, vec(permutedims(regions)))
        report = analyze_heightfield(
            heightfield_path,
            mask_path,
            regions_path,
            manifest_path;
            maximum_accessible_grade=1.0,
            steep_grade=1.0,
            maximum_accessible_steep_fraction=0.0,
        )
        @test report["status"] == "passed"
        @test report["accessible"]["maximum_grade"] == 0.0
        @test report["intentional_relief"]["maximum_grade"] == 10.0
        @test report["protected_relief_fraction"] == round(2 / 9, digits=8)
        @test report["regions"]["protected_relief"]["maximum_grade"] == 10.0
    end
end

@testset "hydrology rejects local uphill reversals" begin
    height = Float32[
        0 0 0 0 0
        0 0 0 0 0
        0 0 8 0 0
        0 0 0 0 0
        0 0 0 0 0
    ]
    centerline = (
        id="ridge_crossing",
        channel_profile="incised_stream",
        points=[[-2.0, 0.0], [0.0, 0.0], [2.0, 0.0]],
    )
    report = CodewealdTerrainLab._hydrology_summary(
        height,
        4.0,
        4.0,
        centerline;
        sample_spacing=1.0,
        uphill_tolerance_m=0.05,
    )
    @test report["uphill_step_fraction"] > 0.08
    @test report["maximum_uphill_step_m"] > 0.35
    @test report["channel_profile"] == "incised_stream"
end
