param(
    [Parameter(Mandatory)][string]$Installer,
    [string]$Output = 'target/gui-evidence/windows'
)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$installRoot = [IO.Path]::GetFullPath((Join-Path $taskRoot 'target/gui-installed'))
$installerPath = (Resolve-Path -LiteralPath $Installer).Path
if (-not $installRoot.StartsWith($taskRoot + [IO.Path]::DirectorySeparatorChar)) {
    throw 'Test installation must remain inside the workspace'
}
$existing = Get-ItemProperty 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/*' -ErrorAction SilentlyContinue |
    Where-Object DisplayName -EQ 'AutoHarness'
foreach ($entry in $existing) {
    if ($entry.InstallLocation.Trim('"').TrimEnd('\') -ne $installRoot.TrimEnd('\')) {
        throw 'An existing AutoHarness installation must not be replaced by this test'
    }
}
function Install-Candidate {
    $result = Start-Process -FilePath $installerPath -ArgumentList @('/S', "/D=$installRoot") -WindowStyle Hidden -Wait -PassThru
    if ($result.ExitCode -ne 0) { throw 'Candidate installation failed' }
    if (-not (Test-Path -LiteralPath (Join-Path $installRoot 'autoharness.exe'))) { throw 'GUI executable missing' }
    if (-not (Test-Path -LiteralPath (Join-Path $installRoot 'ah.exe'))) { throw 'Rollback terminal executable missing' }
}
try {
    Install-Candidate
    $original = (Get-FileHash -LiteralPath (Join-Path $installRoot 'autoharness.exe') -Algorithm SHA256).Hash
    Install-Candidate
    if ((Get-FileHash -LiteralPath (Join-Path $installRoot 'autoharness.exe') -Algorithm SHA256).Hash -ne $original) {
        throw 'Reinstallation changed candidate executable bytes'
    }
    python (Join-Path $PSScriptRoot 'gui_webdriver.py') --binary (Join-Path $installRoot 'autoharness.exe') `
        --driver (Join-Path $taskRoot 'target/gui-tools/bin/tauri-driver.exe') `
        --native-driver (Join-Path $taskRoot 'target/gui-tools/edge/msedgedriver.exe') --output $Output
    if ($LASTEXITCODE -ne 0) { throw 'Installed GUI lifecycle failed' }
} finally {
    $uninstaller = Join-Path $installRoot 'uninstall.exe'
    if (Test-Path -LiteralPath $uninstaller) {
        # NSIS _?= executes the uninstaller in place, so -Wait covers completion.
        $result = Start-Process -FilePath $uninstaller -ArgumentList @('/S', "_?=$installRoot") -WindowStyle Hidden -Wait -PassThru
        if ($result.ExitCode -ne 0) { throw 'Candidate uninstall failed' }
        if (Test-Path -LiteralPath (Join-Path $installRoot 'autoharness.exe')) { throw 'Uninstall retained the GUI executable' }
    }
}
@{ schema_version = 1; status = 'passed'; scenarios = @('install', 'same-version-reinstall', 'uninstall') } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $Output 'installer.json') -Encoding utf8
