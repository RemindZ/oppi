$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$noBuild = $false
$release = $false
$usePiUi = $false
$mock = $false
$runSmoke = $false
$runDogfood = $false
$strictBackgroundLifecycle = $false
$showHelp = $false
$cliArgs = @()
$previousRuntimeLoopMode = $env:OPPI_RUNTIME_LOOP_MODE
$previousNativeShellBin = $env:OPPI_NATIVE_SHELL_BIN
$previousNativeServerBin = $env:OPPI_NATIVE_SERVER_BIN
$previousShellBin = $env:OPPI_SHELL_BIN
$previousServerBin = $env:OPPI_SERVER_BIN

foreach ($arg in $args) {
  switch -Regex ($arg) {
    '^(--|-)?h(elp)?$' { $showHelp = $true; continue }
    '^(--|-)?no-build$' { $noBuild = $true; continue }
    '^(--|-)?release$' { $release = $true; continue }
    '^(--|-)?live$' { $mock = $false; continue }
    '^(--|-)?mock$' { $mock = $true; $cliArgs += "--mock"; continue }
    '^(--|-)?smoke$' { $runSmoke = $true; continue }
    '^(--|-)?dogfood$' { $runDogfood = $true; continue }
    '^(--|-)?strict-background$' { $runDogfood = $true; $strictBackgroundLifecycle = $true; continue }
    '^(--|-)?pi-ui$' { $usePiUi = $true; continue }
    '^(--|-)?stable-ui$' { $usePiUi = $true; continue }
    '^(--|-)?real-ui$' { continue }
    '^(--|-)?proper-ui$' { continue }
    '^(--|-)?native-ui$' { continue }
    '^(--|-)?rust-ui$' { continue }
    '^(--|-)?ratatui$' { continue }
    default { $cliArgs += $arg }
  }
}

if ($showHelp) {
  Write-Host @"
OPPi Rust dev startup

Usage:
  powershell -ExecutionPolicy Bypass -File scripts/rust-dev-startup.ps1 [options] [oppi-shell options]

Default:
  Builds oppi-server, oppi-shell, and @oppiai/cli, then starts the Rust Ratatui UI with ChatGPT/Codex on gpt-5.5.

Options:
  --live                 Use the real provider path. This is the default.
  --mock                 Force local scripted test mode.
  --no-build             Reuse existing target/dist outputs.
  --release              Build/use release Rust binaries.
  --smoke                Run live `oppi tui smoke --json` style startup smoke.
  --dogfood              Run `oppi tui dogfood --mock --json` style dogfood smoke.
  --strict-background    Run dogfood with --require-background-lifecycle.
  --pi-ui, --stable-ui   Launch the stable Pi-powered OPPi UI fallback.
  --no-tui, --line       Forwarded to oppi-shell for plain line mode.

Examples:
  .\scripts\rust-dev-startup.ps1
  .\scripts\rust-dev-startup.ps1 --no-build
  .\scripts\rust-dev-startup.ps1 --provider openai-compatible --model gpt-5.5 --api-key-env OPENAI_API_KEY
  .\scripts\rust-dev-startup.ps1 --mock
  .\scripts\rust-dev-startup.ps1 --smoke
"@
  exit 0
}

function Invoke-Step($Name, $ScriptBlock) {
  Write-Host "==> $Name" -ForegroundColor Cyan
  & $ScriptBlock
  if ($LASTEXITCODE -ne 0) {
    throw "$Name failed with exit code $LASTEXITCODE"
  }
}

Push-Location $repoRoot
try {
  if (-not $noBuild) {
    $cargoArgs = @("build", "-p", "oppi-server", "-p", "oppi-shell")
    if ($release) { $cargoArgs = @("build", "--release", "-p", "oppi-server", "-p", "oppi-shell") }
    Invoke-Step "Build Rust runtime" { cargo @cargoArgs }
    Invoke-Step "Build @oppiai/cli wrapper" { pnpm --filter "@oppiai/cli" build }
  }

  $profileDir = if ($release) { "release" } else { "debug" }
  $exe = if ($IsWindows -or $env:OS -eq "Windows_NT") { ".exe" } else { "" }
  $builtShell = Join-Path $repoRoot "target/$profileDir/oppi-shell$exe"
  $builtServer = Join-Path $repoRoot "target/$profileDir/oppi-server$exe"
  if (Test-Path $builtShell) { $env:OPPI_NATIVE_SHELL_BIN = $builtShell }
  if (Test-Path $builtServer) { $env:OPPI_NATIVE_SERVER_BIN = $builtServer }
  if (Test-Path $builtShell) { $env:OPPI_SHELL_BIN = $builtShell }
  if (Test-Path $builtServer) { $env:OPPI_SERVER_BIN = $builtServer }

  $cli = Join-Path $repoRoot "packages/cli/dist/main.js"
  if (-not (Test-Path $cli)) {
    throw "Missing $cli. Run without --no-build first."
  }
  if (-not $env:OPPI_RUNTIME_LOOP_MODE) {
    $env:OPPI_RUNTIME_LOOP_MODE = "default-with-fallback"
  }

  $hasProviderOrSessionArg = $false
  foreach ($cliArg in $cliArgs) {
    if ($cliArg -eq "--mock" -or
        $cliArg -eq "--provider" -or
        $cliArg.StartsWith("--provider=") -or
        $cliArg -eq "--model" -or
        $cliArg.StartsWith("--model=") -or
        $cliArg -eq "--resume" -or
        $cliArg.StartsWith("--resume=") -or
        $cliArg -eq "--list-sessions" -or
        $cliArg -eq "--sessions") {
      $hasProviderOrSessionArg = $true
      break
    }
  }
  if ($mock -and -not $hasProviderOrSessionArg -and -not $usePiUi) {
    $cliArgs = @("--mock") + $cliArgs
  } elseif (-not $hasProviderOrSessionArg -and -not $usePiUi -and -not $runDogfood) {
    $defaultModel = if ($env:OPPI_RUNTIME_WORKER_MODEL) { $env:OPPI_RUNTIME_WORKER_MODEL } else { "gpt-5.5" }
    $cliArgs = @("--provider", "openai-codex", "--model", $defaultModel) + $cliArgs
  }

  if ($usePiUi) {
    Write-Host "==> Start OPPi with the stable Pi-powered UI fallback" -ForegroundColor Cyan
    node $cli @cliArgs
  } elseif ($runSmoke) {
    Write-Host "==> Run Rust TUI smoke" -ForegroundColor Cyan
    node $cli tui smoke @cliArgs
  } elseif ($runDogfood) {
    if ($strictBackgroundLifecycle -and -not ($cliArgs -contains "--require-background-lifecycle")) {
      $cliArgs += "--require-background-lifecycle"
    }
    Write-Host "==> Run Rust TUI dogfood smoke" -ForegroundColor Cyan
    node $cli tui dogfood @cliArgs
  } else {
    Write-Host "==> Start OPPi with the Rust Ratatui UI" -ForegroundColor Cyan
    Write-Host "    Launch args: $($cliArgs -join ' ')." -ForegroundColor Yellow
    Write-Host "    Use --mock only for local scripted smoke/testing." -ForegroundColor Yellow
    Write-Host "    Fallback: rerun with --pi-ui or pass --no-tui for plain line mode." -ForegroundColor Yellow
    node $cli tui @cliArgs
  }
  exit $LASTEXITCODE
} finally {
  if ($null -eq $previousRuntimeLoopMode) {
    Remove-Item Env:OPPI_RUNTIME_LOOP_MODE -ErrorAction SilentlyContinue
  } else {
    $env:OPPI_RUNTIME_LOOP_MODE = $previousRuntimeLoopMode
  }
  if ($null -eq $previousNativeShellBin) {
    Remove-Item Env:OPPI_NATIVE_SHELL_BIN -ErrorAction SilentlyContinue
  } else {
    $env:OPPI_NATIVE_SHELL_BIN = $previousNativeShellBin
  }
  if ($null -eq $previousNativeServerBin) {
    Remove-Item Env:OPPI_NATIVE_SERVER_BIN -ErrorAction SilentlyContinue
  } else {
    $env:OPPI_NATIVE_SERVER_BIN = $previousNativeServerBin
  }
  if ($null -eq $previousShellBin) {
    Remove-Item Env:OPPI_SHELL_BIN -ErrorAction SilentlyContinue
  } else {
    $env:OPPI_SHELL_BIN = $previousShellBin
  }
  if ($null -eq $previousServerBin) {
    Remove-Item Env:OPPI_SERVER_BIN -ErrorAction SilentlyContinue
  } else {
    $env:OPPI_SERVER_BIN = $previousServerBin
  }
  Pop-Location
}
