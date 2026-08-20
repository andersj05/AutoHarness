[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateRange(1, [int]::MaxValue)]
    [int]$TargetProcessId,

    [ValidateRange(1, 10000)]
    [int]$Samples = 20,

    [ValidateRange(1, 60000)]
    [int]$IntervalMilliseconds = 250,

    [string]$Output
)

$ErrorActionPreference = 'Stop'
$measurements = [System.Collections.Generic.List[long]]::new()

for ($index = 0; $index -lt $Samples; $index++) {
    $process = Get-Process -Id $TargetProcessId -ErrorAction Stop
    $measurements.Add([long]$process.WorkingSet64)
    if ($index + 1 -lt $Samples) {
        Start-Sleep -Milliseconds $IntervalMilliseconds
    }
}

$ordered = @($measurements | Sort-Object)
$middle = [int][Math]::Floor($ordered.Count / 2)
if ($ordered.Count % 2 -eq 0) {
    $median = [long](($ordered[$middle - 1] + $ordered[$middle]) / 2)
} else {
    $median = [long]$ordered[$middle]
}
$p95Index = [Math]::Max(0, [int][Math]::Ceiling($ordered.Count * 0.95) - 1)
$mean = [long](($measurements | Measure-Object -Average).Average)

$result = [ordered]@{
    schema_version = 1
    metric = 'idle_resident_memory_bytes'
    status = 'measured'
    process_id = $TargetProcessId
    samples = $Samples
    interval_milliseconds = $IntervalMilliseconds
    min = [long]$ordered[0]
    median = $median
    p95 = [long]$ordered[$p95Index]
    mean = $mean
    max = [long]$ordered[$ordered.Count - 1]
    network_requests = 0
    network_latency_included = $false
}

$json = $result | ConvertTo-Json
if ([string]::IsNullOrWhiteSpace($Output)) {
    $json
    return
}

$fullOutput = [System.IO.Path]::GetFullPath($Output)
if (Test-Path -LiteralPath $fullOutput) {
    throw "Output already exists: $fullOutput"
}
$parent = Split-Path -Parent $fullOutput
if (-not [string]::IsNullOrWhiteSpace($parent)) {
    [System.IO.Directory]::CreateDirectory($parent) | Out-Null
}
$encoding = [System.Text.UTF8Encoding]::new($false)
[System.IO.File]::WriteAllText($fullOutput, $json + [Environment]::NewLine, $encoding)
Write-Output "Wrote idle-memory report to $fullOutput"
