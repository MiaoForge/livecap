$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

$tag = $env:GITHUB_REF_NAME
$version = [semver]($tag.Substring(1) -replace '-rc([0-9]+)$', '-rc.$1')
$repo = $env:GITHUB_REPOSITORY
$notes = "docs/release/releasenote-$tag.md"
$manifestPath = Join-Path $env:RUNNER_TEMP 'latest.json'

$releases = @(gh api --paginate "repos/$repo/releases" --jq '.[].tag_name')
if ($tag -in $releases) {
    # A rerun after publication must reuse the manifest for the uploaded installer.
    gh release download $tag --repo $repo --pattern latest.json --output $manifestPath --clobber
} else {
    $installers = @(Get-ChildItem -LiteralPath 'src-tauri/target/release/bundle/nsis' -Filter '*-setup.exe' -File)
    if ($installers.Count -ne 1) { throw 'Expected exactly one Windows installer.' }
    $installer = $installers[0]
    $signaturePath = "$($installer.FullName).sig"
    $signature = (Get-Content -LiteralPath $signaturePath -Raw).Trim()
    if (!$signature) { throw 'The updater signature is empty.' }

    @{
        version = $version.ToString()
        notes = Get-Content -LiteralPath $notes -Raw
        platforms = @{
            'windows-x86_64' = @{
                signature = $signature
                url = "$env:GITHUB_SERVER_URL/$repo/releases/download/$tag/$([uri]::EscapeDataString($installer.Name))"
            }
        }
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $manifestPath

    $flags = @('--verify-tag', '--latest=false')
    if ($version.PreReleaseLabel) { $flags += '--prerelease' }
    gh release create $tag $installer.FullName $signaturePath $manifestPath --repo $repo --title "Livecap $tag" --notes-file $notes @flags
}

# Publish the channel only after the versioned installer and signature are available.
if ('updates' -notin $releases) {
    gh release create updates $manifestPath --repo $repo --target $env:GITHUB_SHA --title 'Livecap update feed' --prerelease --latest=false
} else {
    $assets = @(gh release view updates --repo $repo --json assets --jq '.assets[].name')
    if ('latest.json' -in $assets) {
        $current = gh release download updates --repo $repo --pattern latest.json --output - | ConvertFrom-Json
        if ([semver]$current.version -ge $version) {
            Write-Output "Update feed already points to v$($current.version)."
            exit 0
        }
    }
    gh release upload updates $manifestPath --repo $repo --clobber
}
