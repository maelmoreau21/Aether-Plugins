[CmdletBinding()]
param(
    [ValidatePattern('^[a-z0-9-]+$')][string]$Plugin = 'tmdb',
    [Parameter(Mandatory)][string]$KeyPath,
    [string]$PublisherId = 'local',
    [string]$CorePath = (Join-Path $PSScriptRoot '../../Aether'),
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '../artifacts'),
    [switch]$SkipCoreBuild
)
$ErrorActionPreference = 'Stop'
$pluginRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$coreRoot = (Resolve-Path $CorePath).Path
$privateKey = [IO.Path]::GetFullPath($KeyPath)
if (!(Test-Path -LiteralPath $privateKey -PathType Leaf)) { throw 'Clé privée absente. Créez-la avec media-extension-pack keygen (voir README).' }
if ($PublisherId -cnotmatch '^[a-z0-9._-]{1,80}$') { throw 'Identifiant éditeur invalide.' }
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
[IO.Directory]::CreateDirectory($outputRoot) | Out-Null
$packagePath = Join-Path $outputRoot "$Plugin.package.json"
if (Test-Path -LiteralPath $packagePath) { throw "Le paquet existe déjà : $packagePath. Choisissez un autre OutputDirectory." }
$crate = "aether-plugin-$Plugin"
$exeSuffix = if ($env:OS -eq 'Windows_NT') { '.exe' } else { '' }
Push-Location $pluginRoot
try {
    $previousOptimization = $env:CARGO_PROFILE_RELEASE_OPT_LEVEL
    try {
        $env:CARGO_PROFILE_RELEASE_OPT_LEVEL = 'z'
        & cargo build --release --lib -p $crate --target wasm32-unknown-unknown -j 1
        if ($LASTEXITCODE -ne 0) { throw 'Compilation Wasm échouée. Vérifiez rustup target add wasm32-unknown-unknown.' }
    } finally { $env:CARGO_PROFILE_RELEASE_OPT_LEVEL = $previousOptimization }
    & npm ci --ignore-scripts
    if ($LASTEXITCODE -ne 0) { throw 'Installation des outils Wasm échouée.' }
    & node scripts/optimize-wasm.mjs "target/wasm32-unknown-unknown/release/$($crate.Replace('-', '_')).wasm"
    if ($LASTEXITCODE -ne 0) { throw 'Optimisation Wasm échouée.' }
    & node scripts/test-wasm.cjs target/wasm32-unknown-unknown/release $crate
    if ($LASTEXITCODE -ne 0) { throw 'Validation Wasm échouée.' }
    & cargo build --bin $crate -p $crate -j 1
    if ($LASTEXITCODE -ne 0) { throw 'Compilation du manifeste échouée.' }
    # Read the native process bytes as UTF-8, independently of the Windows console code page.
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = Join-Path $pluginRoot "target/debug/$crate$exeSuffix"
    $start.ArgumentList.Add('--manifest')
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false, $true)
    $process = [Diagnostics.Process]::Start($start)
    try {
        $manifestJson = $process.StandardOutput.ReadToEnd()
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { throw 'Lecture du manifeste échouée.' }
    } finally { $process.Dispose() }
    $source = $manifestJson | ConvertFrom-Json
    if (!$source.permissions -or @($source.permissions).Count -eq 0) { throw 'Le manifeste doit déclarer ses permissions.' }
    $wasmPath = Join-Path $pluginRoot "target/wasm32-unknown-unknown/release/$($crate.Replace('-', '_')).wasm"
    $manifest = [ordered]@{
        apiVersion = 1; id = $source.id; name = $source.name; version = $source.version
        description = $source.description; author = $source.author; license = $source.license
        repository = 'https://github.com/maelmoreau21/Aether-Plugins'
        permissions = [string[]]@($source.permissions); category = $source.category; targetAbi = $source.targetAbi
        httpHosts = @($source.httpHosts | Where-Object { $_ -is [string] -and $_ })
        minServerVersion = $source.minServerVersion; maxServerVersion = $source.maxServerVersion
        checksum = (Get-FileHash -LiteralPath $wasmPath -Algorithm SHA256).Hash.ToLowerInvariant()
        banner = $source.banner; configSchema = $source.configSchema
    }
    $manifestPath = Join-Path $outputRoot "$Plugin.manifest.json"
    [IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 20), [Text.UTF8Encoding]::new($false))
    Push-Location $coreRoot
    try {
        if (!$SkipCoreBuild) {
            & cargo build --locked -p media-poc --bin media-extension-pack --bin media-extension-worker -j 1
            if ($LASTEXITCODE -ne 0) { throw 'Compilation des outils Core échouée.' }
        }
        & (Join-Path $coreRoot "target/debug/media-extension-pack$exeSuffix") pack $manifestPath $wasmPath $privateKey $PublisherId $packagePath
        if ($LASTEXITCODE -ne 0) { throw 'Validation/signature du paquet échouée (taille maximale du module : 256 Kio).' }
    } finally { Pop-Location }
    Write-Output "Paquet à importer dans Administration > Extensions : $packagePath"
} finally { Pop-Location }
