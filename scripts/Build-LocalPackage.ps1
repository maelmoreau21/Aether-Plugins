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
    & cargo build --release --lib -p $crate --target wasm32-unknown-unknown --config 'profile.release.opt-level=\"z\"' -j 1
    if ($LASTEXITCODE -ne 0) { throw 'Compilation Wasm échouée. Vérifiez rustup target add wasm32-unknown-unknown.' }
    & cargo build --bin $crate -p $crate -j 1
    if ($LASTEXITCODE -ne 0) { throw 'Compilation du manifeste échouée.' }
    $manifestJson = & (Join-Path $pluginRoot "target/debug/$crate$exeSuffix") --manifest
    if ($LASTEXITCODE -ne 0) { throw 'Lecture du manifeste échouée.' }
    $source = ($manifestJson -join "`n") | ConvertFrom-Json
    $wasmPath = Join-Path $pluginRoot "target/wasm32-unknown-unknown/release/$($crate.Replace('-', '_')).wasm"
    $manifest = [ordered]@{
        apiVersion = 1; id = $source.id; name = $source.name; version = $source.version
        description = $source.description; author = $source.author; license = $source.license
        repository = 'https://github.com/maelmoreau21/Aether-Plugins'
        permissions = [string[]]$(if ($Plugin -eq 'avatar') { 'avatars' } else { 'metadata.read', 'metadata.write', 'images' }); category = $source.category; targetAbi = $source.targetAbi
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
