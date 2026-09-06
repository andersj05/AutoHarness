$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$destination = Join-Path $taskRoot 'target/gui-tools/edge'
New-Item -ItemType Directory -Path $destination -Force | Out-Null
$runtimeRoot = Join-Path ${env:ProgramFiles(x86)} 'Microsoft/EdgeWebView/Application'
$runtimeVersion = Get-ChildItem -LiteralPath $runtimeRoot -Directory |
    Where-Object Name -Match '^\d+\.\d+\.\d+\.\d+$' |
    Sort-Object { [version]$_.Name } -Descending |
    Select-Object -First 1 -ExpandProperty Name
if (-not $runtimeVersion) { throw 'A system WebView2 runtime is required' }
$runtimePath = Join-Path $runtimeRoot $runtimeVersion
$archive = Join-Path $destination 'driver.zip'
Invoke-WebRequest -Uri "https://msedgedriver.microsoft.com/$runtimeVersion/edgedriver_win64.zip" -OutFile $archive
Expand-Archive -LiteralPath $archive -DestinationPath $destination -Force
$driver = Join-Path $destination 'msedgedriver.exe'
$signature = Get-AuthenticodeSignature -LiteralPath $driver
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notlike '*Microsoft Corporation*') {
    throw 'Microsoft native driver signature verification failed'
}
Write-Output "Verified native driver for WebView2 $runtimeVersion"
$runtimePath | Set-Content -LiteralPath (Join-Path $destination 'runtime-path.txt') -Encoding utf8
