param(
    [int]$DebounceMs = 700,
    [int]$CrashRestartLimit = 3,
    [int]$CrashRestartWindowSeconds = 30,
    [int]$CrashRestartDelayMs = 750,
    [switch]$KeepExisting,
    [switch]$LayoutDebug,
    [string[]]$ExternalPluginProjects = @(),
    [int]$SmokeTestSeconds = 0
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $scriptDir
Set-Location $root

$stateDir = Join-Path $root "target\dev-watch"
$pidFile = Join-Path $stateDir "runner.pid"
$logDate = Get-Date -Format "yyyy-MM-dd"
$stdoutLog = Join-Path $stateDir "dev-watch.$logDate.out.log"
$stderrLog = Join-Path $stateDir "dev-watch.$logDate.err.log"
$mainExe = Join-Path $root "target\debug\git-agent.exe"
$mergeExe = Join-Path $root "target\debug\git-agent-merge.exe"
$diffExe = Join-Path $root "target\debug\git-agent-diff.exe"
$pluginSourceDir = Join-Path $root "plugins"
$pluginStageDir = Join-Path $root "target\dev-plugins"
if ($ExternalPluginProjects.Count -eq 0) {
    $siblingPlugin = Join-Path (Split-Path -Parent $root) "git-agent-plugin-template"
    if (Test-Path -LiteralPath (Join-Path $siblingPlugin "plugin.json")) { $ExternalPluginProjects = @($siblingPlugin) }
}
$ExternalPluginProjects = @($ExternalPluginProjects | ForEach-Object { [IO.Path]::GetFullPath($_) })

New-Item -ItemType Directory -Force -Path $stateDir | Out-Null

function Add-DevLogLine {
    param(
        [string]$Path,
        [string]$Line
    )

    for ($attempt = 1; $attempt -le 8; $attempt++) {
        $stream = $null
        $writer = $null
        try {
            $stream = [System.IO.File]::Open(
                $Path,
                [System.IO.FileMode]::Append,
                [System.IO.FileAccess]::Write,
                [System.IO.FileShare]::ReadWrite
            )
            $encoding = New-Object System.Text.UTF8Encoding($false)
            $writer = New-Object System.IO.StreamWriter($stream, $encoding)
            $writer.WriteLine($Line)
            $writer.Flush()
            return $true
        }
        catch [System.IO.IOException] {
            if ($attempt -lt 8) {
                Start-Sleep -Milliseconds (25 * $attempt)
            }
        }
        finally {
            if ($writer) {
                $writer.Dispose()
            }
            elseif ($stream) {
                $stream.Dispose()
            }
        }
    }

    Write-Warning "[dev] unable to append log after retries: $Path"
    return $false
}

function Write-DevLog {
    param(
        [string]$Message,
        [ValidateSet("INFO", "WARN", "ERROR")]
        [string]$Level = "INFO"
    )
    $line = "[dev] $(Get-Date -Format o) [$Level] $Message"
    Write-Host $line
    Add-DevLogLine -Path $stdoutLog -Line $line | Out-Null
    if ($Level -eq "ERROR") {
        Add-DevLogLine -Path $stderrLog -Line $line | Out-Null
    }
}

function Stop-ProcessTree {
    param([int]$ProcessId)

    $children = Get-CimInstance Win32_Process -Filter "ParentProcessId=$ProcessId" -ErrorAction SilentlyContinue
    foreach ($child in $children) {
        Stop-ProcessTree -ProcessId $child.ProcessId
    }

    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($process) {
        Stop-Process -Id $ProcessId -Force -ErrorAction SilentlyContinue
    }
}

function Stop-ExistingRunner {
    if ($KeepExisting -or -not (Test-Path $pidFile)) {
        return
    }

    $oldPidText = Get-Content -Path $pidFile -ErrorAction SilentlyContinue | Select-Object -First 1
    [int]$oldPid = 0
    if ([int]::TryParse($oldPidText, [ref]$oldPid) -and $oldPid -gt 0 -and $oldPid -ne $PID) {
        Write-DevLog "stop old dev runner pid=$oldPid"
        Stop-ProcessTree -ProcessId $oldPid
    }

    Remove-Item -Path $pidFile -Force -ErrorAction SilentlyContinue
}

function Stop-MainWindow {
    if ($script:mainProcess -and -not $script:mainProcess.HasExited) {
        Write-DevLog "stop git-agent pid=$($script:mainProcess.Id)"
        Stop-ProcessTree -ProcessId $script:mainProcess.Id
    }

    Get-Process -Name "git-agent" -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $mainExe } |
        ForEach-Object {
            Write-DevLog "stop stray git-agent pid=$($_.Id)"
            Stop-ProcessTree -ProcessId $_.Id
        }
}

function Stop-DevBinaries {
    Stop-MainWindow

    Get-Process -Name "git-agent-merge" -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $mergeExe } |
        Stop-Process -Force -ErrorAction SilentlyContinue
    Get-Process -Name "git-agent-diff" -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $diffExe } |
        Stop-Process -Force -ErrorAction SilentlyContinue
}

function Build-Bins {
    Write-DevLog "cargo build --bins"
    Push-Location $root
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        & cargo build --bins >> $stdoutLog 2>> $stderrLog
        $buildExitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousErrorActionPreference
        Pop-Location
    }

    if ($buildExitCode -ne 0) {
        Write-DevLog "build failed: exit $buildExitCode" -Level "ERROR"
        return $false
    }

    Write-DevLog "built main=$mainExe"
    Write-DevLog "built merge=$mergeExe"
    Write-DevLog "built diff=$diffExe"
    if (-not (Build-Plugins)) {
        return $false
    }
    return $true
}

function Build-Plugins {
    # The application was stopped before Build-Bins, so no staged DLL is loaded.
    $resolvedStage = [System.IO.Path]::GetFullPath($pluginStageDir)
    $expectedStage = [System.IO.Path]::GetFullPath((Join-Path $root "target\dev-plugins"))
    if ($resolvedStage -ne $expectedStage -or -not $resolvedStage.StartsWith([System.IO.Path]::GetFullPath($root) + [System.IO.Path]::DirectorySeparatorChar)) {
        throw "Plugin staging path escaped the workspace: $resolvedStage"
    }
    if (Test-Path -LiteralPath $resolvedStage) {
        Remove-Item -LiteralPath $resolvedStage -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $resolvedStage | Out-Null
    $pluginDirectories = @()
    if (Test-Path -LiteralPath $pluginSourceDir) { $pluginDirectories += @(Get-ChildItem -LiteralPath $pluginSourceDir -Directory | Sort-Object Name) }
    foreach ($project in $ExternalPluginProjects) { $pluginDirectories += Get-Item -LiteralPath $project }
    $pluginIds = @{}
    foreach ($directory in $pluginDirectories) {
        $manifestPath = Join-Path $directory.FullName "plugin.json"
        $cargoPath = Join-Path $directory.FullName "Cargo.toml"
        if (-not (Test-Path -LiteralPath $manifestPath) -or -not (Test-Path -LiteralPath $cargoPath)) {
            continue
        }
        $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        $libraryName = [string]$manifest.library.windows
        if ($manifest.id -notmatch '^[a-zA-Z0-9_.-]+$' -or $manifest.id -in @('.', '..') -or $pluginIds.ContainsKey($manifest.id) -or
            [string]::IsNullOrWhiteSpace($libraryName) -or [System.IO.Path]::GetFileName($libraryName) -ne $libraryName) {
            throw "Invalid or duplicate plugin manifest: $manifestPath"
        }
        $pluginIds[$manifest.id] = $true
        Write-DevLog "build plugin id=$($manifest.id) manifest=$cargoPath"
        $previousPluginErrorPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = "Continue"
            & cargo build --manifest-path $cargoPath --lib --target-dir (Join-Path $root "target") >> $stdoutLog 2>> $stderrLog
            $pluginBuildExitCode = $LASTEXITCODE
        }
        finally { $ErrorActionPreference = $previousPluginErrorPreference }
        if ($pluginBuildExitCode -ne 0) {
            Write-DevLog "plugin build failed id=$($manifest.id)" -Level "ERROR"
            return $false
        }
        $libraryPath = Join-Path (Join-Path $root "target\debug") $libraryName
        if (-not (Test-Path -LiteralPath $libraryPath)) {
            Write-DevLog "plugin library missing: $libraryPath" -Level "ERROR"
            return $false
        }
        $destination = Join-Path $resolvedStage $manifest.id
        New-Item -ItemType Directory -Force -Path $destination | Out-Null
        Copy-Item -LiteralPath $manifestPath -Destination (Join-Path $destination "plugin.json")
        Copy-Item -LiteralPath $libraryPath -Destination (Join-Path $destination $libraryName)
        $assetSource = Join-Path $directory.FullName "assets"
        if (Test-Path -LiteralPath $assetSource) {
            Copy-Item -LiteralPath $assetSource -Destination (Join-Path $destination "assets") -Recurse
        }
        if ($manifest.locales) {
            foreach ($locale in $manifest.locales.PSObject.Properties) {
                $localeSource = [IO.Path]::GetFullPath((Join-Path $directory.FullName $locale.Value))
                $localeTarget = [IO.Path]::GetFullPath((Join-Path $destination $locale.Value))
                if (-not $localeSource.StartsWith($directory.FullName + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
                    -not $localeTarget.StartsWith($destination + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                    throw "Plugin dictionary path escapes plugin directory"
                }
                New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($localeTarget)) | Out-Null
                Copy-Item -LiteralPath $localeSource -Destination $localeTarget
            }
        }
        Write-DevLog "staged plugin id=$($manifest.id) directory=$destination"
    }
    return $true
}

function Start-MainWindow {
    if (-not (Test-Path $mainExe)) {
        Write-DevLog "main exe missing; skip start" -Level "WARN"
        return $null
    }

    if ($LayoutDebug) {
        $env:GIT_AGENT_LAYOUT_DEBUG = "1"
        Write-DevLog "layout debug enabled"
    }
    else {
        Remove-Item Env:\GIT_AGENT_LAYOUT_DEBUG -ErrorAction SilentlyContinue
    }

    $env:GIT_AGENT_PLUGIN_DIR = $pluginStageDir
    Write-DevLog "plugin directory=$env:GIT_AGENT_PLUGIN_DIR"
    Write-DevLog "start $mainExe"
    $process = Start-Process `
        -FilePath $mainExe `
        -WorkingDirectory $root `
        -PassThru
    Write-DevLog "started git-agent pid=$($process.Id)"
    return $process
}

function Test-MainWindowExit {
    if (-not $script:mainProcess) {
        return
    }

    $processId = $script:mainProcess.Id
    $script:mainProcess.Refresh()
    if (-not $script:mainProcess.HasExited) {
        return
    }

    $exitCode = $script:mainProcess.ExitCode
    if ($exitCode -eq 0) {
        Write-DevLog "git-agent exited pid=$processId exit=$exitCode"
    }
    else {
        Write-DevLog "git-agent exited pid=$processId exit=$exitCode" -Level "ERROR"
    }
    $script:mainProcess = $null

    if ($exitCode -eq 0) {
        return
    }

    $now = Get-Date
    $script:crashRestartTimes = @(
        $script:crashRestartTimes | Where-Object {
            ($now - $_).TotalSeconds -lt $CrashRestartWindowSeconds
        }
    )
    if ($script:crashRestartTimes.Count -ge $CrashRestartLimit) {
        Write-DevLog "crash restart suppressed after $CrashRestartLimit failures in ${CrashRestartWindowSeconds}s" -Level "ERROR"
        return
    }

    $script:crashRestartTimes += $now
    $attempt = $script:crashRestartTimes.Count
    Write-DevLog "restart git-agent after crash attempt=$attempt/$CrashRestartLimit delay_ms=$CrashRestartDelayMs" -Level "WARN"
    Start-Sleep -Milliseconds $CrashRestartDelayMs
    $script:mainProcess = Start-MainWindow
}

function Restart-DevApp {
    $script:crashRestartTimes = @()
    Stop-DevBinaries
    if (Build-Bins) {
        $script:mainProcess = Start-MainWindow
    }
}

Stop-ExistingRunner
Set-Content -Path $pidFile -Value $PID

$watcher = New-Object System.IO.FileSystemWatcher
$watcher.Path = $root
$watcher.IncludeSubdirectories = $true
$watcher.EnableRaisingEvents = $true
$watcher.Filter = "*.*"

$externalWatchers = @()
$externalSubscriptions = @()
foreach ($project in $ExternalPluginProjects) {
    $externalWatcher = New-Object System.IO.FileSystemWatcher
    $externalWatcher.Path = $project
    $externalWatcher.IncludeSubdirectories = $true
    $externalWatcher.Filter = "*.*"
    foreach ($eventName in @('Changed','Created','Deleted','Renamed')) {
        $identifier = "GitAgentExternalPlugin-$($externalWatchers.Count)-$eventName"
        $externalSubscriptions += Register-ObjectEvent -InputObject $externalWatcher -EventName $eventName -SourceIdentifier $identifier
    }
    $externalWatcher.EnableRaisingEvents = $true
    $externalWatchers += $externalWatcher
}

$lastRestart = Get-Date "2000-01-01"
$script:mainProcess = $null
$script:crashRestartTimes = @()

try {
    Restart-DevApp
    if ($SmokeTestSeconds -gt 0) {
        if (-not $script:mainProcess) { throw "Development smoke test could not start the application" }
        for ($second = 0; $second -lt $SmokeTestSeconds; $second++) {
            Start-Sleep -Seconds 1
            $script:mainProcess.Refresh()
            if ($script:mainProcess.HasExited) { throw "Development smoke test app exited: $($script:mainProcess.ExitCode)" }
        }
        Write-DevLog "smoke test passed seconds=$SmokeTestSeconds pid=$($script:mainProcess.Id)"
        return
    }
    Write-Host "[dev] watching host sources and external plugin projects. Ctrl+C to stop." -ForegroundColor Green
    Write-Host "[dev] logs: $stdoutLog ; $stderrLog" -ForegroundColor DarkGray

    while ($true) {
        $change = $watcher.WaitForChanged("Changed, Created, Deleted, Renamed", 1000)
        $externalPath = $null
        foreach ($event in @(Get-Event | Where-Object { $_.SourceIdentifier -like 'GitAgentExternalPlugin-*' })) {
            $fullPath = $event.SourceEventArgs.FullPath
            foreach ($project in $ExternalPluginProjects) {
                if ($fullPath.StartsWith($project + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                    $relative = $fullPath.Substring($project.Length + 1) -replace "/", "\"
                    if ($relative -like 'src\*' -or $relative -like 'assets\*' -or $relative -like 'locales\*' -or $relative -like 'vendor\*' -or $relative -in @('plugin.json','Cargo.toml','Cargo.lock')) { $externalPath = $fullPath }
                }
            }
            Remove-Event -EventIdentifier $event.EventIdentifier
        }
        if ($change.TimedOut -and -not $externalPath) {
            Test-MainWindowExit
            continue
        }
        $path = if ($externalPath) { $externalPath } else { $change.Name -replace "/", "\" }
        $isWatched = $externalPath -or
            $path -like "src\*" -or
            $path -like "assets\*" -or
            $path -like "plugins\*" -or
            $path -like "crates\*" -or
            $path -eq "Cargo.toml" -or
            $path -eq "Cargo.lock"

        if (-not $isWatched) {
            continue
        }

        $now = Get-Date
        if (($now - $lastRestart).TotalMilliseconds -lt $DebounceMs) {
            continue
        }

        $lastRestart = $now
        Write-Host "[dev] change: $path" -ForegroundColor DarkGray
        Restart-DevApp
    }
}
finally {
    $watcher.Dispose()
    foreach ($subscription in $externalSubscriptions) { Unregister-Event -SourceIdentifier $subscription.SourceIdentifier -ErrorAction SilentlyContinue }
    foreach ($externalWatcher in $externalWatchers) { $externalWatcher.Dispose() }
    Stop-MainWindow

    $currentPidText = Get-Content -Path $pidFile -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($currentPidText -eq "$PID") {
        Remove-Item -Path $pidFile -Force -ErrorAction SilentlyContinue
    }
}
