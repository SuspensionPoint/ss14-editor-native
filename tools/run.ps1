# Runs the editor from a clone of this repository, on Windows.
#
#   .\tools\run.ps1                                        the start screen, to choose a fork
#   .\tools\run.ps1 Maps/_Starlight/Stations/Cork.yml      open a map of upstream/starlight by its path
#   .\tools\run.ps1 -Publish                               publish the engine side and write its code fingerprint
#
# Build the editor first: cd native; cargo build --release -p ss14-editor
#
# From the start screen, the editor makes each fork's pack and textures itself and keeps them
# in its cache. Given a map, it runs on upstream/starlight instead, with a pack and textures
# this script makes once in native/cache. The engine side's `pack` makes the pack and the
# editor's `--bake` makes the textures. Delete them after the fork changes and they are made
# again.
#
# -Publish builds the engine side into src/Editor.Native/bin/r2r, or into r2r-next if an editor
# is open. Next to it, it writes the code fingerprint of upstream/starlight, which the editor
# checks a fork against before opening it. It doesn't start the editor.
param(
    [Parameter(Position = 0)][string]$Map = '',
    [switch]$Publish
)

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path -replace '\\', '/'
$exe = "$root/native/target/release/ss14-editor.exe"
if (-not (Test-Path $exe)) {
    Write-Error "$exe is not built. Run: cd native; cargo build --release -p ss14-editor"
    exit 1
}

$bin = "$root/src/Editor.Native/bin"
$fork = "$root/upstream/starlight"

if ($Publish) {
    # An open editor holds the files of the build it runs on: the new one waits in r2r-next.
    $open = Get-Process ss14-editor -ErrorAction SilentlyContinue
    $target = if ($open) { "$bin/r2r-next" } else { "$bin/r2r" }
    & dotnet publish "$root/src/Editor.Native" -c Release -r win-x64 --self-contained false -p:PublishReadyToRun=true -p:TreatWarningsAsErrors=false -o $target -nologo -v q -clp:ErrorsOnly
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    & $exe --stamp-engine $target $fork
    exit $LASTEXITCODE
}

# An engine build made while an editor was open waits in r2r-next: the open editor holds the
# files of the build it runs on, and Windows will not have them replaced. Put the new build in
# place now, which works once nothing holds the old one (a folder in use cannot be renamed).
if (Test-Path "$bin/r2r-next") {
    Remove-Item -Recurse -Force "$bin/r2r-previous" -ErrorAction SilentlyContinue
    try {
        if (Test-Path "$bin/r2r") { Rename-Item "$bin/r2r" 'r2r-previous' -ErrorAction Stop }
        Rename-Item "$bin/r2r-next" 'r2r' -ErrorAction Stop
        Remove-Item -Recurse -Force "$bin/r2r-previous" -ErrorAction SilentlyContinue
    } catch {
        Write-Error "A newer engine build is waiting in $bin/r2r-next, and an editor that is still open is using the one before it. Close that editor and start this again."
        exit 1
    }
}

if (-not (Test-Path "$bin/r2r/Editor.Native.dll")) {
    Write-Error "The engine side is not published. Run: .\tools\run.ps1 -Publish"
    exit 1
}

if (-not $Map) {
    # The start screen: the engine side alone.
    & $exe "$bin/r2r"
    exit $LASTEXITCODE
}

$pack = "$root/native/cache/kernel-resources.zip"
$textures = "$root/native/cache/textures.sstx"
New-Item -ItemType Directory -Force "$root/native/cache" | Out-Null
if (-not (Test-Path $pack)) {
    Write-Host "Packing the fork's prototypes (once)"
    & dotnet "$bin/r2r/Editor.Native.dll" pack $fork $pack
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
if (-not (Test-Path $textures)) {
    Write-Host "Baking the fork's textures (once)"
    & $exe --bake $fork $textures
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
& $exe "$bin/r2r" $fork $pack $textures "$fork/Resources/$Map"
exit $LASTEXITCODE
