$ErrorActionPreference = "Stop"

$cargoToml = Get-Content "Cargo.toml" -Raw
if ($cargoToml -match '(?m)^version\s*=\s*"([^"]+)"') {
    $version = $Matches[1]
} else {
    throw "No se pudo leer la versión desde Cargo.toml"
}

Write-Host "Compilando iPixel 32x32 Controller v$version..."
cargo test
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

cargo build --release --manifest-path "installer_bin/Cargo.toml"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$outName = "iPixel_32x32_Setup_v$version.exe"
Copy-Item "installer_bin\target\release\ipixel_setup.exe" $outName -Force
Write-Host "Instalador generado: $outName"
