#pragma once

#include "Modules/ModuleManager.h"

/**
 * Native Editor entry point for the Codeweald zone handoff.
 *
 * The module owns the editor-visible manifest validation transaction.  It does
 * not fabricate a Landscape when the payload is invalid, nor claim a Landscape
 * was created until the LandscapeEditor transaction has actually succeeded.
 */
class FCodewealdZoneImporterModule final : public IModuleInterface
{
public:
    virtual void StartupModule() override;
    virtual void ShutdownModule() override;

private:
    void RegisterMenus();
    void ChooseAndPreflightManifest();
    bool PreflightManifest(const FString& ManifestPath, FString& OutReportPath, TArray<FString>& OutErrors) const;
};
