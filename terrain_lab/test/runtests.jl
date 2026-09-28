using CodewealdTerrainLab
using JSON3
using Test

function write_f32le(path, values)
    native_words = reinterpret(UInt32, values)
    words = Base.ENDIAN_BOM == 0x04030201 ? native_words : htol.(native_words)
    write(path, reinterpret(UInt8, words))
end

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
        write_f32le(heightfield_path, vec(permutedims(heights)))
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
        @test_throws ErrorException analyze_heightfield(
            heightfield_path,
            mask_path,
            regions_path,
            manifest_path;
            maximum_accessible_grade=NaN,
        )
    end
end

@testset "numerical contracts fail closed" begin
    height = reshape(collect(1.0:9.0), 3, 3)
    @test_throws ErosionWorkerError thermal_erosion(
        height;
        cell_m=1.0,
        talus_degrees=30.0,
        rate=-1.0,
    )
    @test_throws ErosionWorkerError thermal_erosion(
        height;
        cell_m=1.0,
        talus_degrees=90.0,
    )
    @test_throws ErosionWorkerError erode_heightfield(
        ErodeRequest(
            height,
            1.0,
            ErosionProfile("invalid", 0, 0.2, 0.2, 0.5, 0.1, 30.0, 0.1, 0.1),
            nothing,
        ),
    )
    @test_throws ErosionWorkerError flux_field(
        height;
        source=fill(-1.0, size(height)),
    )
end

@testset "little-endian heightfield decoding" begin
    mktempdir() do directory
        path = joinpath(directory, "heightfield.bin")
        write(path, UInt8[0x00, 0x00, 0x80, 0x3f])
        decoded = CodewealdTerrainLab._read_heightfield(path, 1)
        @test decoded == Float32[1.0;;]
    end
end

include(joinpath(@__DIR__, "..", "bin", "lane_overlap_worker.jl"))

@testset "lane worker emits valid JSON" begin
    result = JSON3.read(
        handle(
            "{\"op\":\"lane_overlap\",\"lane\":{\"x0\":0,\"x1\":4,\"y0\":0,\"y1\":4}," *
            "\"footprint\":{\"x0\":2,\"x1\":6,\"y0\":2,\"y1\":6}}",
        ),
    )
    @test result["intersects"] == true
    @test result["overlap_area"] == 4
    @test JSON3.read(failure("probe", "line\nfeed"))["detail"] == "line\nfeed"
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
