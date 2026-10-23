[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$KeyPath,
    [Parameter(Mandatory)][string]$PublicKeyPath,
    [string]$PublisherId = 'maelmoreau21',
    [uri]$BaseUrl = 'https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/packages/',
    [string]$OutputDirectory = (Join-Path $PSScriptRoot ('../artifacts/repository-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))),
    [string]$CorePath = (Join-Path $PSScriptRoot '../../Aether'),
    [switch]$SkipCoreBuild
)
$ErrorActionPreference = 'Stop'
$pluginRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $outputRoot) { throw 'Destination déjà présente. Générez le nouveau catalogue dans un autre dossier, comparez, puis remplacez explicitement les fichiers publiés.' }
if (!(Test-Path -LiteralPath $KeyPath -PathType Leaf) -or !(Test-Path -LiteralPath $PublicKeyPath -PathType Leaf)) { throw 'Clés de signature privée/publique requises. Voir README.' }
if (!$SkipCoreBuild) {
    Push-Location $CorePath
    try {
        & cargo build --locked -p media-poc --bin media-extension-pack --bin media-extension-worker -j 1
        if ($LASTEXITCODE -ne 0) { throw 'Compilation des outils de signature échouée.' }
    } finally { Pop-Location }
}
Push-Location $pluginRoot
try {
    $metadata = (& cargo metadata --format-version 1 --no-deps | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0) { throw 'Lecture des crates échouée.' }
    $plugins = @($metadata.packages | Where-Object { $_.name.StartsWith('aether-plugin-') -and @($_.targets | Where-Object { $_.kind -contains 'bin' }).Count -gt 0 } | Sort-Object name)
    if (!$plugins.Count) { throw 'Aucun plugin installable.' }
    [IO.Directory]::CreateDirectory((Join-Path $outputRoot 'packages')) | Out-Null
    foreach ($plugin in $plugins) {
        $shortName = $plugin.name.Substring('aether-plugin-'.Length)
        & (Join-Path $PSScriptRoot 'Build-LocalPackage.ps1') -Plugin $shortName -KeyPath $KeyPath -PublisherId $PublisherId -CorePath $CorePath -OutputDirectory (Join-Path $outputRoot 'packages') -SkipCoreBuild
        $packagePath = Join-Path $outputRoot "packages/$shortName.package.json"
        $version = ([IO.File]::ReadAllText($packagePath) | ConvertFrom-Json).manifest.version
        Move-Item -LiteralPath $packagePath -Destination (Join-Path $outputRoot "packages/$shortName-$version.package.json")
    }
    & (Join-Path $PSScriptRoot 'New-RepositoryManifest.ps1') -PackageDirectory (Join-Path $outputRoot 'packages') -PublicKeyPath $PublicKeyPath -BaseUrl $BaseUrl -OutputPath (Join-Path $outputRoot 'manifest.json')
    Copy-Item -LiteralPath $PublicKeyPath -Destination (Join-Path $outputRoot 'publisher-public.txt')
    & node (Join-Path $PSScriptRoot 'test-repository.cjs') $outputRoot $BaseUrl.AbsoluteUri
    if ($LASTEXITCODE -ne 0) { throw 'Vérification cryptographique du catalogue échouée.' }
    Write-Output "Catalogue signé préparé : $outputRoot. Aucun push ou déploiement effectué."
} finally { Pop-Location }
