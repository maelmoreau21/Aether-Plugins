[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$PackageDirectory,
    [Parameter(Mandatory)][string]$PublicKeyPath,
    [Parameter(Mandatory)][uri]$BaseUrl,
    [Parameter(Mandatory)][string]$OutputPath
)
$ErrorActionPreference = 'Stop'
if ($BaseUrl.Scheme -ne 'https' -or $BaseUrl.Port -ne 443 -or $BaseUrl.UserInfo -or $BaseUrl.Fragment) { throw 'BaseUrl doit être une URL HTTPS publique sans identifiants.' }
$source = (Resolve-Path -LiteralPath $PackageDirectory).Path
$publicKey = [IO.File]::ReadAllText((Resolve-Path -LiteralPath $PublicKeyPath).Path).Trim()
if ([Convert]::FromBase64String($publicKey).Length -ne 32) { throw 'Clé publique Ed25519 invalide.' }
$destination = [IO.Path]::GetFullPath($OutputPath)
if (Test-Path -LiteralPath $destination) { throw 'Le manifeste existe déjà : choisissez un nouveau chemin puis remplacez explicitement celui publié après vérification.' }
$entries = @(Get-ChildItem -LiteralPath $source -Filter '*.package.json' -File | Sort-Object Name | ForEach-Object {
    $package = [IO.File]::ReadAllText($_.FullName) | ConvertFrom-Json
    if (!$package.manifest.id -or !$package.publisherId -or !$package.signature) { throw 'Paquet signé invalide.' }
    [ordered]@{
        manifest = $package.manifest
        publisherId = $package.publisherId
        publicKey = $publicKey
        packageUrl = $BaseUrl.AbsoluteUri.TrimEnd('/') + '/' + [uri]::EscapeDataString($_.Name)
        packageSha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
})
if ($entries.Count -eq 0 -or $entries.Count -gt 100) { throw 'Le dossier doit contenir de 1 à 100 paquets signés.' }
$json = [ordered]@{ apiVersion = 1; extensions = $entries } | ConvertTo-Json -Depth 30
if ([Text.Encoding]::UTF8.GetByteCount($json) -gt 262144) { throw 'Manifeste supérieur à 256 Kio.' }
[IO.File]::WriteAllText($destination, $json, [Text.UTF8Encoding]::new($false))
Write-Output "Manifeste préparé : $destination. Hébergez-le avec les paquets sur votre serveur HTTPS ; aucune publication automatique."
