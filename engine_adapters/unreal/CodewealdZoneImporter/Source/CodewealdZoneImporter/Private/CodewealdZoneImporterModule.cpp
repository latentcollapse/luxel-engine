#include "CodewealdZoneImporterModule.h"

#include "DesktopPlatformModule.h"
#include "Framework/Application/SlateApplication.h"
#include "HAL/FileManager.h"
#include "IDesktopPlatform.h"
#include "Misc/FileHelper.h"
#include "Misc/MessageDialog.h"
#include "Misc/Paths.h"
#include "Serialization/JsonReader.h"
#include "Serialization/JsonSerializer.h"
#include "ToolMenus.h"

#define LOCTEXT_NAMESPACE "CodewealdZoneImporter"

namespace CodewealdZoneImporter
{
    constexpr TCHAR Schema[] = TEXT("codeweald.unreal-zone-import/v1");
    constexpr int32 RequiredLandscapeResolution = 1009;

    bool ReadObject(const FString& Path, TSharedPtr<FJsonObject>& OutObject, TArray<FString>& OutErrors)
    {
        FString Text;
        if (!FFileHelper::LoadFileToString(Text, *Path))
        {
            OutErrors.Add(FString::Printf(TEXT("Cannot read manifest: %s"), *Path));
            return false;
        }
        const TSharedRef<TJsonReader<>> Reader = TJsonReaderFactory<>::Create(Text);
        if (!FJsonSerializer::Deserialize(Reader, OutObject) || !OutObject.IsValid())
        {
            OutErrors.Add(TEXT("Manifest is not valid JSON."));
            return false;
        }
        return true;
    }

    bool RequireExistingRelativeFile(const FString& Root, const TSharedPtr<FJsonObject>& Object, const FString& Field, TArray<FString>& OutErrors)
    {
        FString Relative;
        if (!Object.IsValid() || !Object->TryGetStringField(Field, Relative) || Relative.IsEmpty())
        {
            OutErrors.Add(FString::Printf(TEXT("Missing %s."), *Field));
            return false;
        }
        const FString Candidate = FPaths::ConvertRelativePathToFull(Root, Relative);
        if (!FPaths::IsUnderDirectory(Candidate, Root) || !FPaths::FileExists(Candidate))
        {
            OutErrors.Add(FString::Printf(TEXT("Missing payload file for %s: %s"), *Field, *Relative));
            return false;
        }
        return true;
    }

    TSharedRef<FJsonObject> MakeReport(const FString& ManifestPath, const FString& ZoneId, const TArray<FString>& Errors)
    {
        TSharedRef<FJsonObject> Report = MakeShared<FJsonObject>();
        Report->SetStringField(TEXT("schema_version"), TEXT("codeweald.unreal-native-preflight/v1"));
        Report->SetStringField(TEXT("manifest_path"), ManifestPath);
        Report->SetStringField(TEXT("zone_id"), ZoneId);
        Report->SetStringField(TEXT("status"), Errors.IsEmpty() ? TEXT("passed") : TEXT("failed"));
        TArray<TSharedPtr<FJsonValue>> ErrorValues;
        for (const FString& Error : Errors)
        {
            ErrorValues.Add(MakeShared<FJsonValueString>(Error));
        }
        Report->SetArrayField(TEXT("errors"), ErrorValues);
        Report->SetStringField(TEXT("next_step"), Errors.IsEmpty()
            ? TEXT("Payload accepted. Run the LandscapeEditor import transaction in an exercised UE5 installation.")
            : TEXT("Correct the manifest or its files; no Unreal assets were created."));
        return Report;
    }
}

void FCodewealdZoneImporterModule::StartupModule()
{
    UToolMenus::RegisterStartupCallback(FSimpleMulticastDelegate::FDelegate::CreateRaw(this, &FCodewealdZoneImporterModule::RegisterMenus));
}

void FCodewealdZoneImporterModule::ShutdownModule()
{
    if (UToolMenus::IsToolMenusAvailable())
    {
        UToolMenus::UnRegisterStartupCallback(this);
        UToolMenus::UnregisterOwner(this);
    }
}

void FCodewealdZoneImporterModule::RegisterMenus()
{
    FToolMenuOwnerScoped OwnerScoped(this);
    UToolMenu* Menu = UToolMenus::Get()->ExtendMenu(TEXT("LevelEditor.MainMenu.Tools"));
    FToolMenuSection& Section = Menu->FindOrAddSection(TEXT("Codeweald"));
    Section.AddMenuEntry(
        TEXT("CodewealdPreflightZoneManifest"),
        LOCTEXT("PreflightZoneManifestLabel", "Codeweald: Preflight Zone Manifest"),
        LOCTEXT("PreflightZoneManifestTooltip", "Validate a generated Codeweald Unreal payload before any Landscape or PCG assets are created."),
        FSlateIcon(),
        FUIAction(FExecuteAction::CreateRaw(this, &FCodewealdZoneImporterModule::ChooseAndPreflightManifest)));
}

void FCodewealdZoneImporterModule::ChooseAndPreflightManifest()
{
    IDesktopPlatform* DesktopPlatform = FDesktopPlatformModule::Get();
    if (!DesktopPlatform)
    {
        FMessageDialog::Open(EAppMsgType::Ok, LOCTEXT("DesktopPlatformUnavailable", "Codeweald could not open the manifest picker in this editor build."));
        return;
    }

    TArray<FString> Files;
    const void* ParentWindow = FSlateApplication::Get().FindBestParentWindowHandleForDialogs(nullptr);
    const bool bSelected = DesktopPlatform->OpenFileDialog(
        ParentWindow, TEXT("Codeweald Unreal Zone Manifest"), FPaths::ProjectDir(), TEXT(""),
        TEXT("Codeweald manifest (*.json)|*.json"), EFileDialogFlags::None, Files);
    if (!bSelected || Files.IsEmpty()) return;

    FString ReportPath;
    TArray<FString> Errors;
    const bool bPassed = PreflightManifest(Files[0], ReportPath, Errors);
    const FText Summary = bPassed
        ? FText::Format(LOCTEXT("PreflightPassed", "Codeweald payload accepted. Report written to:\n{0}\n\nNo Landscape has been created yet."), FText::FromString(ReportPath))
        : FText::Format(LOCTEXT("PreflightFailed", "Codeweald payload rejected. No Unreal assets were created.\n\nReport written to:\n{0}\n\n{1}"), FText::FromString(ReportPath), FText::FromString(FString::Join(Errors, TEXT("\n"))));
    FMessageDialog::Open(EAppMsgType::Ok, Summary);
}

bool FCodewealdZoneImporterModule::PreflightManifest(const FString& ManifestPath, FString& OutReportPath, TArray<FString>& OutErrors) const
{
    using namespace CodewealdZoneImporter;
    const FString AbsoluteManifest = FPaths::ConvertRelativePathToFull(ManifestPath);
    const FString Root = FPaths::GetPath(AbsoluteManifest);
    TSharedPtr<FJsonObject> Manifest;
    FString ZoneId;
    if (ReadObject(AbsoluteManifest, Manifest, OutErrors))
    {
        FString FoundSchema;
        if (!Manifest->TryGetStringField(TEXT("schema_version"), FoundSchema) || FoundSchema != Schema)
            OutErrors.Add(FString::Printf(TEXT("Expected schema %s."), Schema));
        Manifest->TryGetStringField(TEXT("zone_id"), ZoneId);
        if (ZoneId.IsEmpty()) OutErrors.Add(TEXT("Missing zone_id."));

        const TSharedPtr<FJsonObject>* LandscapePointer = nullptr;
        if (!Manifest->TryGetObjectField(TEXT("landscape"), LandscapePointer) || !LandscapePointer || !LandscapePointer->IsValid())
        {
            OutErrors.Add(TEXT("Missing landscape object."));
        }
        else
        {
            const TSharedPtr<FJsonObject>& Landscape = *LandscapePointer;
            double Resolution = 0.0;
            double ZScale = 0.0;
            if (!Landscape->TryGetNumberField(TEXT("resolution"), Resolution) || FMath::RoundToInt(Resolution) != RequiredLandscapeResolution)
                OutErrors.Add(FString::Printf(TEXT("Landscape resolution must be %d."), RequiredLandscapeResolution));
            if (!Landscape->TryGetNumberField(TEXT("z_scale"), ZScale) || ZScale <= 0.0)
                OutErrors.Add(TEXT("Landscape z_scale must be positive."));
            RequireExistingRelativeFile(Root, Landscape, TEXT("heightmap_16"), OutErrors);

            const TSharedPtr<FJsonObject>* LayerSources = nullptr;
            const TSharedPtr<FJsonObject>* Weightmaps = nullptr;
            if (!Landscape->TryGetObjectField(TEXT("layer_sources"), LayerSources) || !LayerSources || !(*LayerSources)->TryGetObjectField(TEXT("weightmaps"), Weightmaps) || !Weightmaps)
            {
                OutErrors.Add(TEXT("Missing landscape layer_sources.weightmaps."));
            }
            else
            {
                for (const TCHAR* Layer : { TEXT("grass"), TEXT("road"), TEXT("rock"), TEXT("snow") })
                    RequireExistingRelativeFile(Root, *Weightmaps, Layer, OutErrors);
            }
        }
    }

    const TSharedRef<FJsonObject> Report = MakeReport(AbsoluteManifest, ZoneId, OutErrors);
    FString ReportText;
    const TSharedRef<TJsonWriter<>> Writer = TJsonWriterFactory<>::Create(&ReportText);
    FJsonSerializer::Serialize(Report, Writer);
    OutReportPath = FPaths::Combine(FPaths::ProjectSavedDir(), TEXT("Codeweald"), FPaths::GetBaseFilename(AbsoluteManifest) + TEXT("_native_preflight.json"));
    IFileManager::Get().MakeDirectory(*FPaths::GetPath(OutReportPath), true);
    if (!FFileHelper::SaveStringToFile(ReportText, *OutReportPath))
    {
        OutErrors.Add(FString::Printf(TEXT("Could not write native preflight report: %s"), *OutReportPath));
    }
    return OutErrors.IsEmpty();
}

#undef LOCTEXT_NAMESPACE

IMPLEMENT_MODULE(FCodewealdZoneImporterModule, CodewealdZoneImporter)
