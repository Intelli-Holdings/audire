# Local dev script: check installer artifact size <= 123 MB.
# Run after `npm run tauri build`.

$limit = 129331200  # 123 MB in bytes

$bundleDir = "src-tauri\target\release\bundle"

$paths = @(
    "$bundleDir\msi",
    "$bundleDir\nsis"
)

$found = @()
foreach ($p in $paths) {
    if (Test-Path $p) {
        $found += Get-ChildItem -Path $p -Recurse -File | Where-Object { $_.Extension -in ".msi", ".exe" }
    }
}

if ($found.Count -eq 0) {
    Write-Host "No installer artifacts found in $bundleDir"
    Write-Host "Run 'npm run tauri build' first."
    exit 1
}

$tooBig = $found | Where-Object { $_.Length -gt $limit }
$found | ForEach-Object { Write-Host ("ARTIFACT: {0} ({1:N0} bytes)" -f $_.FullName, $_.Length) }

if ($tooBig.Count -gt 0) {
    Write-Error "One or more artifacts exceed 123 MB:"
    $tooBig | ForEach-Object { Write-Error ("TOO BIG: {0} ({1:N0} bytes)" -f $_.FullName, $_.Length) }
    exit 1
}

Write-Host "All artifacts are within the 123 MB size limit."
