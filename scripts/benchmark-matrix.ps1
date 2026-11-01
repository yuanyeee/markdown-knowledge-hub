param([int[]]$Counts = @(10000,50000,100000), [int]$DocumentBytes=8192)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
$executable=Join-Path $repo 'target/release/examples/benchmark.exe'
$results=Join-Path $repo 'docs/benchmarks'
New-Item -ItemType Directory -Path $results -Force | Out-Null
foreach($count in $Counts) {
    $output=Join-Path $results "synthetic-$count-$DocumentBytes.json"
    $log=Join-Path $results "synthetic-$count-$DocumentBytes.stderr.log"
    $process=Start-Process -FilePath $executable -ArgumentList @($count,$DocumentBytes) -RedirectStandardOutput $output -RedirectStandardError $log -PassThru -WindowStyle Hidden
    [long]$peak=0
    while(-not $process.HasExited){
        $process.Refresh()
        if($process.WorkingSet64 -gt $peak){$peak=$process.WorkingSet64}
        Start-Sleep -Milliseconds 100
    }
    $process.WaitForExit()
    if($process.ExitCode -ne 0){throw "Benchmark failed: $log"}
    $result=Get-Content -LiteralPath $output -Raw -Encoding utf8 | ConvertFrom-Json
    $result | Add-Member -NotePropertyName sampled_peak_working_set_bytes -NotePropertyValue $peak
    $result | Add-Member -NotePropertyName memory_sampling_interval_ms -NotePropertyValue 100
    $result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $output -Encoding utf8
    Write-Output "$count files: initial=$($result.initial_ms)ms searchP95=$($result.search_p95_ms)ms eventP95=$($result.event_delta_p95_ms)ms sampledPeak=$peak"
}
