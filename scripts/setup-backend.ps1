param(
    [switch]$SkipBrowser,
    [string]$EngineRoot,
    [string]$RuntimeRoot
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$backendRoot = Join-Path $projectRoot 'backend'
$engineRoot = if ($EngineRoot) { $EngineRoot } else { Join-Path $backendRoot 'engine' }
$runtimeRoot = if ($RuntimeRoot) { $RuntimeRoot } else { Join-Path $backendRoot '.runtime' }
$pythonRoot = Join-Path $runtimeRoot 'python'
$uvPath = Join-Path $backendRoot 'tools\uv.exe'

if (-not (Test-Path -LiteralPath $uvPath)) {
    throw "随项目提供的 Python 环境工具不存在：$uvPath"
}

New-Item -ItemType Directory -Force -Path $pythonRoot | Out-Null
$env:UV_PYTHON_INSTALL_DIR = $pythonRoot
$env:UV_CACHE_DIR = Join-Path $env:LOCALAPPDATA 'KuaishouCrawler\uv-cache'
$env:PLAYWRIGHT_BROWSERS_PATH = Join-Path $runtimeRoot 'ms-playwright'
New-Item -ItemType Directory -Force -Path $env:UV_CACHE_DIR | Out-Null

Write-Host '正在准备项目专用 Python 3.11…'
& $uvPath python install --install-dir $pythonRoot 3.11
if ($LASTEXITCODE -ne 0) { throw 'Python 运行时安装失败。' }

$python = Get-ChildItem -LiteralPath $pythonRoot -Filter python.exe -File -Recurse |
    Where-Object { $_.FullName -match '\\install\\python\.exe$' } |
    Select-Object -First 1
if (-not $python) {
    $python = Get-ChildItem -LiteralPath $pythonRoot -Filter python.exe -File -Recurse | Select-Object -First 1
}
if (-not $python) { throw '没有找到已安装的 Python 运行时。' }

$venvRoot = Join-Path $runtimeRoot 'venv'
Write-Host '正在创建项目专用虚拟环境…'
& $uvPath venv --python $python.FullName $venvRoot
if ($LASTEXITCODE -ne 0) { throw 'Python 虚拟环境创建失败。' }
$runtimePython = Join-Path $venvRoot 'Scripts\python.exe'
if (-not (Test-Path -LiteralPath $runtimePython)) { throw '没有找到项目虚拟环境的 Python。' }
$runtimeUri = [System.Uri]::new($runtimeRoot.TrimEnd('\') + '\')
$pythonUri = [System.Uri]::new($runtimePython)
$pythonRelativePath = [System.Uri]::UnescapeDataString($runtimeUri.MakeRelativeUri($pythonUri).ToString()).Replace('/', '\')
[System.IO.File]::WriteAllText((Join-Path $runtimeRoot 'python-path.txt'), $pythonRelativePath, [System.Text.UTF8Encoding]::new($false))

Write-Host '正在安装采集引擎依赖到项目目录…'
& $uvPath pip install --quiet --python $runtimePython -r (Join-Path $engineRoot 'requirements.txt')
if ($LASTEXITCODE -ne 0) { throw '采集引擎依赖安装失败。' }

if (-not $SkipBrowser) {
    Write-Host '正在安装项目专用 Chromium…'
    & $runtimePython -m playwright install chromium
    if ($LASTEXITCODE -ne 0) { throw 'Chromium 安装失败。可稍后重试，或使用 --SkipBrowser 跳过。' }
    [System.IO.File]::WriteAllText((Join-Path $runtimeRoot 'browser-installed.flag'), 'ready', [System.Text.UTF8Encoding]::new($false))
}

Write-Host "环境已安装：$runtimePython"
Write-Host "浏览器目录：$env:PLAYWRIGHT_BROWSERS_PATH"
