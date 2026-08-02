using UnrealBuildTool;

public class CodewealdZoneImporter : ModuleRules
{
    public CodewealdZoneImporter(ReadOnlyTargetRules Target) : base(Target)
    {
        PCHUsage = PCHUsageMode.UseExplicitOrSharedPCHs;
        PrivateDependencyModuleNames.AddRange(new[]
        {
            "Core", "CoreUObject", "Engine", "InputCore", "Json", "JsonUtilities",
            "Slate", "SlateCore", "ToolMenus", "DesktopPlatform", "UnrealEd"
        });
    }
}
